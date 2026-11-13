//! The search/launcher popup: one input, results from every provider, keyboard
//! driven. Opened fresh on each toggle and destroyed when dismissed.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, ClipboardItem, Context, Entity, InteractiveElement as _,
    IntoElement, KeystrokeEvent, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div,
    point, px, size,
};
use yaltri_core::search::search_all;
use yaltri_core::{ItemAction, SearchItem};
use yaltri_platform::launch;
use yaltri_platform::session::Session;

use crate::daemon::Daemon;

const WIDTH: f32 = 640.;
const INPUT_HEIGHT: f32 = 52.;
const ROW_HEIGHT: f32 = 52.;
const PADDING: f32 = 8.;

pub fn toggle(cx: &mut App) {
    if let Some(handle) = cx.global::<Daemon>().launcher_window.take_if_open(cx) {
        let _ = handle.update(cx, |_, window, cx| close(window, cx));
        return;
    }
    open(cx);
}

trait TakeIfOpen {
    fn take_if_open(&self, cx: &App) -> Option<AnyWindowHandle>;
}

impl TakeIfOpen for Option<AnyWindowHandle> {
    /// The handle, if that window still exists.
    fn take_if_open(&self, cx: &App) -> Option<AnyWindowHandle> {
        self.filter(|h| cx.windows().contains(h))
    }
}

fn open(cx: &mut App) {
    let session = cx.global::<Daemon>().session;
    let options = window_options(session, 0, cx);
    let result = gpui_kit::open_window(options, cx, |window, cx| {
        let view = cx.new(|cx| Launcher::new(window, cx));
        let input = view.read(cx).input.clone();
        input.update(cx, |input, cx| input.focus(window, cx));
        window.activate_window();
        view
    });
    match result {
        Ok((handle, _)) => {
            cx.global_mut::<Daemon>().launcher_window = Some(handle);
            cx.activate(true);
        }
        Err(e) => tracing::error!("opening launcher failed: {e:#}"),
    }
}

fn close(window: &mut Window, cx: &mut App) {
    window.remove_window();
    cx.global_mut::<Daemon>().launcher_window = None;
}

fn window_height(rows: usize) -> f32 {
    INPUT_HEIGHT + PADDING * 2. + rows as f32 * ROW_HEIGHT + if rows > 0 { PADDING } else { 0. }
}

fn window_options(session: Session, rows: usize, cx: &App) -> WindowOptions {
    let window_size = size(px(WIDTH), px(window_height(rows)));
    // Horizontally centered, a quarter of the way down the primary display.
    let mut bounds = Bounds::centered(None, window_size, cx);
    if let Some(display) = cx.primary_display() {
        let area = display.bounds();
        bounds.origin = point(
            area.origin.x + (area.size.width - window_size.width) / 2.,
            area.origin.y + area.size.height / 4.,
        );
    }

    #[allow(unused_mut)]
    let mut kind = WindowKind::PopUp;
    #[cfg(target_os = "linux")]
    if session == Session::WaylandLayerShell {
        use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, LayerShellOptions};
        kind = WindowKind::LayerShell(LayerShellOptions {
            namespace: "yaltri-launcher".into(),
            anchor: Anchor::TOP,
            margin: Some((px(200.), px(0.), px(0.), px(0.))),
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            ..Default::default()
        });
    }
    let _ = session;

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        focus: true,
        show: true,
        kind,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        app_id: Some("yaltri".into()),
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

pub struct Launcher {
    input: Entity<InputState>,
    items: Vec<SearchItem>,
    selected: usize,
    search: Option<Task<()>>,
    /// Becomes true once the window gained focus; losing it afterwards closes
    /// the launcher (but not the initial "not yet active" notification).
    was_active: bool,
    _subscriptions: Vec<Subscription>,
}

impl Launcher {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Uygulama, dosya ara veya hesapla…"));

        let on_change = cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                this.run_search(window, cx);
            }
        });

        let on_activation = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.was_active = true;
            } else if this.was_active {
                close(window, cx);
            }
        });

        // The input consumes up/down/escape/enter itself, so intercept them
        // before normal dispatch while this launcher window is focused.
        let this_window = window.window_handle();
        let weak = cx.weak_entity();
        let on_key = cx.intercept_keystrokes(move |event: &KeystrokeEvent, window, cx| {
            if window.window_handle() != this_window {
                return;
            }
            let handled = weak
                .update(cx, |this, cx| this.on_key(event, window, cx))
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        });

        Self {
            input,
            items: Vec::new(),
            selected: 0,
            search: None,
            was_active: false,
            _subscriptions: vec![on_change, on_activation, on_key],
        }
    }

    fn on_key(&mut self, event: &KeystrokeEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let ks = &event.keystroke;
        match ks.key.as_str() {
            "escape" => close(window, cx),
            "up" => self.move_selection(-1, cx),
            "down" | "tab" => self.move_selection(1, cx),
            // Ctrl+Enter: show the item in its folder instead of opening it.
            "enter" => self.accept(self.selected, ks.modifiers.control, window, cx),
            _ => return false,
        }
        true
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.items.is_empty() {
            return;
        }
        let len = self.items.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
        cx.notify();
    }

    fn run_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.input.read(cx).value().to_string();
        let daemon = cx.global::<Daemon>();
        let providers = daemon.providers.clone();
        let history = daemon.history.clone();
        let limit = daemon.settings.launcher.max_results;
        let background = cx.background_executor().clone();
        // Replacing the task drops (cancels) the previous, now stale search.
        self.search = Some(cx.spawn_in(window, async move |this, cx| {
            let items = background
                .spawn(async move {
                    let boost = |item: &SearchItem| {
                        item.action.history_key().map_or(0, |k| history.lock().map_or(0, |h| h.boost(&k)))
                    };
                    search_all(&providers, &query, limit, &boost)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| this.set_items(items, window, cx));
        }));
    }

    fn set_items(&mut self, items: Vec<SearchItem>, window: &mut Window, cx: &mut Context<Self>) {
        self.items = items;
        self.selected = 0;
        window.resize(size(px(WIDTH), px(window_height(self.items.len()))));
        cx.notify();
    }

    fn accept(&mut self, index: usize, alternate: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.items.get(index).cloned() else {
            return;
        };
        if let Some(key) = item.action.history_key() {
            cx.global::<Daemon>().record_use(&key);
        }
        let result: anyhow::Result<()> = if alternate && let Some(path) = item.action.path() {
            launch::reveal(path)
        } else {
            match &item.action {
                ItemAction::CopyText(text) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                    Ok(())
                }
                ItemAction::Open(path) => open::that_detached(path).map_err(Into::into),
                ItemAction::OpenUrl(url) => open::that_detached(url).map_err(Into::into),
                ItemAction::Run { program, args, terminal } => launch::run(program, args, *terminal),
            }
        };
        if let Err(e) = result {
            tracing::warn!(title = item.title, "action failed: {e:#}");
        }
        close(window, cx);
    }
}

impl Render for Launcher {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let rows = self.items.iter().enumerate().map(|(ix, item)| {
            let selected = ix == self.selected;
            div()
                .id(ix)
                .h(px(ROW_HEIGHT))
                .px_3()
                .flex()
                .flex_col()
                .justify_center()
                .rounded(theme.radius)
                .cursor_pointer()
                .when(selected, |row| row.bg(theme.list_active))
                .when(!selected, |row| row.hover(|row| row.bg(theme.list_hover)))
                .child(div().text_base().text_color(theme.foreground).child(item.title.clone()))
                .child(div().text_xs().text_color(theme.muted_foreground).child(item.subtitle.clone()))
                .on_click(cx.listener(move |this, _, window, cx| this.accept(ix, false, window, cx)))
        });

        div()
            .size_full()
            .p(px(PADDING))
            .flex()
            .flex_col()
            .gap(px(PADDING))
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius_lg)
            .child(
                div()
                    .h(px(INPUT_HEIGHT))
                    .flex()
                    .items_center()
                    .child(Input::new(&self.input).appearance(false).large()),
            )
            .children(rows)
    }
}
