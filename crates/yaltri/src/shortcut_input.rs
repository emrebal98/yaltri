//! Shortcut recorder (PowerToys style): shows the shortcut as key caps; click
//! it and press the new combination. Esc cancels, Backspace/Delete clears.
//! Global hotkeys are suspended while recording so the combination reaches
//! this window instead of triggering a tool. On Windows and X11 a keyboard
//! grab (`yaltri_platform::keygrab`) takes the keys before the desktop does,
//! so combinations held by other programs (PowerToys' `Win+Shift+C`) or the
//! shell (GNOME's Super) can be recorded too.

use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    BoxShadow, Context, ElementId, EventEmitter, FocusHandle, InteractiveElement as _, IntoElement, KeystrokeEvent,
    Modifiers, ModifiersChangedEvent, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, point, px,
};
use yaltri_platform::keys::{self, RecordError, SUPER_NAME};

use crate::daemon::Daemon;
use crate::i18n::t;
use crate::theme::{self, R_SM};
use crate::ui;

/// Recording ends by itself after this, as a safety net: while recording on
/// Windows and X11 every key press goes to the recorder.
const RECORD_TIMEOUT: Duration = Duration::from_secs(15);

/// How long the window may be inactive before recording stops.
const DEACTIVATION_GRACE: Duration = Duration::from_millis(150);

/// Emitted when the user recorded or cleared a shortcut.
pub struct ShortcutChanged;

/// Result of one key press while recording.
enum Outcome {
    Held(Modifiers),
    Combo(Result<String, RecordError>),
    Cancel,
    Clear,
}

pub struct ShortcutInput {
    id: ElementId,
    value: String,
    recording: bool,
    /// Modifiers currently held while recording, shown live.
    held: Modifiers,
    /// Problem with the last key press while recording.
    error: Option<SharedString>,
    /// Set by the owner when the saved shortcut could not be used: the
    /// field turns red and the owner shows the message.
    conflict: bool,
    focus: FocusHandle,
    intercept: Option<Subscription>,
    /// System-wide key capture plus the task feeding it into this view.
    #[cfg(any(windows, target_os = "linux"))]
    grab: Option<(yaltri_platform::keygrab::KeyGrab, Task<()>)>,
    timeout: Option<Task<()>>,
    /// Pending stop after the window was deactivated.
    deactivated: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ShortcutChanged> for ShortcutInput {}

impl ShortcutInput {
    pub fn new(id: impl Into<ElementId>, value: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let subscriptions = vec![
            // Clicking elsewhere, or switching to another window, ends recording.
            cx.on_blur(&focus, window, |this, _, cx| this.stop(cx)),
            cx.observe_window_activation(window, |this, window, cx| {
                if this.recording && !window.is_window_active() {
                    // Stop only if the window stays inactive: starting the
                    // X11 keyboard grab bounces focus out and straight back in.
                    this.deactivated = Some(cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(DEACTIVATION_GRACE).await;
                        let _ = this.update_in(cx, |this, window, cx| {
                            if !window.is_window_active() {
                                this.stop(cx);
                            }
                        });
                    }));
                }
            }),
            // Never leave hotkeys suspended if the window closes mid-recording.
            cx.on_release(|this, cx| {
                if this.recording {
                    cx.global_mut::<Daemon>().set_hotkeys_suspended(false);
                }
            }),
        ];
        Self {
            id: id.into(),
            value,
            recording: false,
            held: Modifiers::default(),
            error: None,
            conflict: false,
            focus,
            intercept: None,
            #[cfg(any(windows, target_os = "linux"))]
            grab: None,
            timeout: None,
            deactivated: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    /// Replaces the shown shortcut (e.g. when a new one was refused).
    pub fn set_value(&mut self, value: String, cx: &mut Context<Self>) {
        self.value = value;
        cx.notify();
    }

    pub fn is_conflict(&self) -> bool {
        self.conflict
    }

    pub fn set_conflict(&mut self, conflict: bool, cx: &mut Context<Self>) {
        self.conflict = conflict;
        cx.notify();
    }

    /// The last recording problem, shown by the owner under the field.
    pub fn error(&self) -> Option<SharedString> {
        self.error.clone()
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recording {
            return;
        }
        self.recording = true;
        self.error = None;
        self.held = Modifiers::default();
        cx.global_mut::<Daemon>().set_hotkeys_suspended(true);
        self.focus.focus(window, cx);

        // Take every keystroke in this window before normal dispatch, so
        // bindings (Tab, Enter, Escape...) cannot swallow the combination.
        let this_window = window.window_handle();
        let weak = cx.weak_entity();
        self.intercept = Some(cx.intercept_keystrokes(move |event: &KeystrokeEvent, window, cx| {
            if window.window_handle() != this_window {
                return;
            }
            let _ = weak.update(cx, |this, cx| this.on_keystroke(event, window, cx));
            cx.stop_propagation();
        }));
        #[cfg(any(windows, target_os = "linux"))]
        self.start_grab(window, cx);
        self.timeout = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RECORD_TIMEOUT).await;
            let _ = this.update(cx, |this, cx| this.stop(cx));
        }));
        cx.notify();
    }

    #[cfg(any(windows, target_os = "linux"))]
    fn start_grab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use futures::StreamExt as _;
        use yaltri_platform::keygrab::{Grabbed, KeyGrab};

        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Grabbed>();
        let sink = move |event| {
            let _ = tx.unbounded_send(event);
        };
        #[cfg(windows)]
        let grab = KeyGrab::start(sink);
        #[cfg(target_os = "linux")]
        let grab = {
            // Wayland: no grab; the in-window interceptor does what it can.
            let Some(xid) = ui::x11_window(window) else { return };
            KeyGrab::start(xid, sink)
        };
        let grab = match grab {
            Ok(g) => g,
            Err(e) => {
                // The in-window interceptor still works for free combinations.
                tracing::warn!("keyboard grab unavailable: {e:#}");
                return;
            }
        };
        let task = cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = rx.next().await {
                let outcome = match event {
                    Grabbed::Modifiers { ctrl, alt, shift, win } => {
                        Outcome::Held(Modifiers { control: ctrl, alt, shift, platform: win, ..Default::default() })
                    }
                    Grabbed::Combo(result) => Outcome::Combo(result),
                    Grabbed::Cancel => Outcome::Cancel,
                    Grabbed::Clear => Outcome::Clear,
                };
                if this.update_in(cx, |this, window, cx| this.apply(outcome, window, cx)).is_err() {
                    break;
                }
            }
        });
        self.grab = Some((grab, task));
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if !self.recording {
            return;
        }
        self.recording = false;
        self.intercept = None;
        #[cfg(any(windows, target_os = "linux"))]
        {
            self.grab = None; // releases the hook / X11 grab
        }
        self.timeout = None;
        self.deactivated = None;
        self.held = Modifiers::default();
        self.error = None;
        cx.global_mut::<Daemon>().set_hotkeys_suspended(false);
        cx.notify();
    }

    fn on_keystroke(&mut self, event: &KeystrokeEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let m = ks.modifiers;
        let any_modifier = m.control || m.alt || m.shift || m.platform;
        if keys::is_modifier_key(&ks.key) {
            return;
        }
        let outcome = match ks.key.as_str() {
            "escape" if !any_modifier => Outcome::Cancel,
            "backspace" | "delete" if !any_modifier => Outcome::Clear,
            key => Outcome::Combo(keys::spec_from_press(m.control, m.alt, m.shift, m.platform, key)),
        };
        self.apply(outcome, window, cx);
    }

    fn apply(&mut self, outcome: Outcome, window: &mut Window, cx: &mut Context<Self>) {
        if !self.recording {
            return;
        }
        match outcome {
            Outcome::Held(m) => {
                self.held = m;
                cx.notify();
            }
            Outcome::Cancel => self.stop(cx),
            Outcome::Clear => {
                self.value.clear();
                self.stop(cx);
                cx.emit(ShortcutChanged);
            }
            Outcome::Combo(Ok(spec)) => {
                self.value = spec;
                self.stop(cx);
                window.blur(cx);
                cx.emit(ShortcutChanged);
            }
            Outcome::Combo(Err(e)) => {
                self.error = Some(record_error_text(e).into());
                cx.notify();
            }
        }
    }

    fn on_modifiers(&mut self, event: &ModifiersChangedEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.recording {
            self.held = event.modifiers;
            cx.notify();
        }
    }
}

fn record_error_text(e: RecordError) -> &'static str {
    match e {
        RecordError::NeedsModifier => t("shortcut.needs_modifier"),
        RecordError::UnsupportedKey => t("shortcut.unsupported_key"),
    }
}

impl Render for ShortcutInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::of(window);
        let caps: Vec<String> = if self.recording {
            let m = self.held;
            [(m.platform, SUPER_NAME), (m.control, "Ctrl"), (m.alt, "Alt"), (m.shift, "Shift")]
                .into_iter()
                .filter(|(on, _)| *on)
                .map(|(_, name)| name.to_owned())
                .collect()
        } else {
            keys::display_parts(&self.value)
        };
        let conflict = self.conflict && !self.recording;

        let right = if self.recording {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .text_color(p.muted)
                .child(ui::keycap("Esc", 18., p))
                .child(t("shortcut.cancel_hint"))
                .into_any_element()
        } else if conflict {
            ui::icon("circle-alert", 16., p.danger).into_any_element()
        } else {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .text_color(p.muted)
                .child(t("shortcut.click_to_change"))
                .child(ui::icon("pencil", 13., p.muted))
                .into_any_element()
        };

        let ring = BoxShadow {
            color: p.ring(),
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(3.),
            inset: false,
        };

        div()
            .id(self.id.clone())
            .track_focus(&self.focus)
            .on_modifiers_changed(cx.listener(Self::on_modifiers))
            .h(px(36.))
            .flex_1()
            .min_w_0()
            .pl(px(6.))
            .pr(px(10.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(R_SM))
            .border_1()
            .border_color(p.border)
            .cursor_pointer()
            .when(self.recording, |d| d.border_color(p.accent_strong).shadow(vec![ring]))
            .when(conflict, |d| d.border_color(p.danger).bg(p.danger_bg))
            .on_click(cx.listener(|this, _, window, cx| this.start(window, cx)))
            .when(self.recording && caps.is_empty(), |d| {
                d.child(div().size(px(6.)).flex_none().rounded(px(3.)).bg(p.accent))
                    .child(div().text_size(px(13.)).text_color(p.foreground).child(t("shortcut.press_keys")))
            })
            .when(!self.recording && caps.is_empty(), |d| {
                d.child(div().pl(px(4.)).text_size(px(13.)).text_color(p.muted).child(t("shortcut.unassigned")))
            })
            .children(ui::keycaps(caps, 24., p))
            .child(div().flex_1())
            .child(right)
    }
}
