//! Shortcut recorder (PowerToys style): shows the shortcut as key caps; click
//! it and press the new combination. Esc cancels, Backspace/Delete clears.
//! Global hotkeys are suspended while recording so the combination reaches
//! this window instead of triggering a tool. On Windows a low-level keyboard
//! hook captures the keys system-wide, so combinations held by other
//! programs (PowerToys' `Win+Shift+C`) can be recorded too.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, ElementId, EventEmitter, FocusHandle, InteractiveElement as _, IntoElement, KeystrokeEvent,
    Modifiers, ModifiersChangedEvent, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, px,
};
use yaltri_platform::keys::{self, RecordError, SUPER_NAME};

use crate::daemon::Daemon;

/// Recording ends by itself after this, as a safety net: while recording on
/// Windows every key press system-wide goes to the recorder.
const RECORD_TIMEOUT: Duration = Duration::from_secs(15);

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
    error: Option<SharedString>,
    focus: FocusHandle,
    intercept: Option<Subscription>,
    /// System-wide key capture plus the task feeding it into this view.
    #[cfg(windows)]
    grab: Option<(yaltri_platform::keygrab::KeyGrab, Task<()>)>,
    timeout: Option<Task<()>>,
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
                if !window.is_window_active() {
                    this.stop(cx);
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
            focus,
            intercept: None,
            #[cfg(windows)]
            grab: None,
            timeout: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
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
        #[cfg(windows)]
        self.start_grab(window, cx);
        self.timeout = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RECORD_TIMEOUT).await;
            let _ = this.update(cx, |this, cx| this.stop(cx));
        }));
        cx.notify();
    }

    #[cfg(windows)]
    fn start_grab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use futures::StreamExt as _;
        use yaltri_platform::keygrab::{Grabbed, KeyGrab};

        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Grabbed>();
        let grab = match KeyGrab::start(move |event| {
            let _ = tx.unbounded_send(event);
        }) {
            Ok(g) => g,
            Err(e) => {
                // The in-window interceptor still works for free combinations.
                tracing::warn!("keyboard hook unavailable: {e:#}");
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
        #[cfg(windows)]
        {
            self.grab = None; // unhooks the keyboard
        }
        self.timeout = None;
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
                self.error = Some(e.message().into());
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

fn key_cap(label: String, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .px_2()
        .py(px(2.))
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .bg(theme.secondary)
        .text_color(theme.secondary_foreground)
        .text_sm()
        .child(label)
}

impl Render for ShortcutInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let caps: Vec<String> = if self.recording {
            let m = self.held;
            [(m.control, "Ctrl"), (m.alt, "Alt"), (m.shift, "Shift"), (m.platform, SUPER_NAME)]
                .into_iter()
                .filter(|(on, _)| *on)
                .map(|(_, name)| name.to_owned())
                .collect()
        } else {
            keys::display_parts(&self.value)
        };
        let placeholder = if self.recording {
            if caps.is_empty() { "Yeni kısayola basın…" } else { "" }
        } else if caps.is_empty() {
            "Atanmamış"
        } else {
            ""
        };
        let hint = if self.recording { "Esc: iptal · Backspace: kaldır" } else { "Değiştirmek için tıklayın" };

        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .id(self.id.clone())
                    .track_focus(&self.focus)
                    .on_modifiers_changed(cx.listener(Self::on_modifiers))
                    .h(px(36.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(if self.recording { theme.ring } else { theme.input })
                    .when(self.recording, |d| d.border_2())
                    .bg(theme.background)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| this.start(window, cx)))
                    .children(caps.into_iter().map(|c| key_cap(c, cx)))
                    .when(!placeholder.is_empty(), |d| {
                        d.child(div().text_sm().text_color(theme.muted_foreground).child(placeholder))
                    })
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(hint)
                            .when(!self.recording, |d| d.child(div().text_size(px(14.)).child(IconName::Pencil))),
                    ),
            )
            .when_some(self.error.clone(), |d, e| d.child(div().text_xs().text_color(theme.danger).child(e)))
    }
}
