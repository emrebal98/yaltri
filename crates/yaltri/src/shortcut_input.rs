//! Shortcut recorder (PowerToys style): shows the shortcut as key caps; click
//! it and press the new combination. Esc cancels, Backspace/Delete clears.
//! Global hotkeys are suspended while recording so the combination reaches
//! this window instead of triggering a tool.

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, ElementId, EventEmitter, FocusHandle, InteractiveElement as _, IntoElement, KeystrokeEvent,
    Modifiers, ModifiersChangedEvent, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, div, px,
};
use yaltri_platform::keys::{self, SUPER_NAME};

use crate::daemon::Daemon;

/// Emitted when the user recorded or cleared a shortcut.
pub struct ShortcutChanged;

pub struct ShortcutInput {
    id: ElementId,
    value: String,
    recording: bool,
    /// Modifiers currently held while recording, shown live.
    held: Modifiers,
    error: Option<SharedString>,
    focus: FocusHandle,
    intercept: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ShortcutChanged> for ShortcutInput {}

impl ShortcutInput {
    pub fn new(id: impl Into<ElementId>, value: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let subscriptions = vec![
            // Clicking elsewhere ends recording.
            cx.on_blur(&focus, window, |this, _, cx| this.stop(cx)),
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
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if !self.recording {
            return;
        }
        self.recording = false;
        self.intercept = None;
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
        match ks.key.as_str() {
            "escape" if !any_modifier => self.stop(cx),
            "backspace" | "delete" if !any_modifier => {
                self.value.clear();
                self.stop(cx);
                cx.emit(ShortcutChanged);
            }
            key => match keys::spec_from_press(m.control, m.alt, m.shift, m.platform, key) {
                Ok(spec) => {
                    self.value = spec;
                    self.stop(cx);
                    window.blur(cx);
                    cx.emit(ShortcutChanged);
                }
                Err(e) => {
                    self.error = Some(e.message().into());
                    cx.notify();
                }
            },
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
