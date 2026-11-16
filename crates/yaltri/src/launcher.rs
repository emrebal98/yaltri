//! The search/launcher popup: one input, results from every provider, keyboard
//! driven. Opened fresh on each toggle and destroyed when dismissed.

use std::time::{Duration, Instant};

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Bounds, ClipboardItem, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, KeystrokeEvent, ObjectFit, ParentElement as _, Render, ScrollHandle, StatefulInteractiveElement as _,
    Styled as _, StyledImage as _, Subscription, Task, Window, WindowBackgroundAppearance, WindowBounds, WindowKind,
    WindowOptions, div, img, point, px, size,
};
use yaltri_core::search::search_all;
use yaltri_core::{ItemAction, ItemKind, Scope, SearchItem};
use yaltri_platform::launch;
use yaltri_platform::session::Session;

use crate::daemon::Daemon;
use crate::i18n::{t, tf};
use crate::theme::{self, Palette, R_LG, R_MD, R_SM};
use crate::ui::{self, TextStyle as _};

const WIDTH: f32 = 640.;
const INPUT_HEIGHT: f32 = 52.;
/// Chips (28) plus the row's bottom padding (10).
const CHIPS_HEIGHT: f32 = 38.;
const ROW_HEIGHT: f32 = 52.;
const ROW_GAP: f32 = 2.;
const LIST_PADDING: f32 = 6.;
/// Rows shown before the list scrolls.
const VISIBLE_ROWS: usize = 8;
const NO_RESULTS_HEIGHT: f32 = 136.;
/// Our 1 px card border. On Windows the system draws the border and the
/// rounded corners of the popup instead (see `ui::frame_popup`): a rounded
/// card in a square window would show the window's dark corners.
const BORDER: f32 = if cfg!(windows) { 0. } else { 1. };
/// Margin around the card for its shadow. Zero: a popup window's
/// "transparent" background is drawn dark on Windows, so the card fills the
/// window and the system draws the popup shadow.
const SHADOW_X: f32 = 0.;
const SHADOW_TOP: f32 = 0.;
const SHADOW_BOTTOM: f32 = 0.;

const OPEN_ANIM: Duration = Duration::from_millis(120);
const CLOSE_ANIM: Duration = Duration::from_millis(80);
/// Rotating placeholder: one hint every 3 s; 260 ms in, 220 ms out.
const HINT_PERIOD: Duration = Duration::from_millis(3000);
const HINT_IN: f32 = 260.;
const HINT_OUT: f32 = 220.;
/// `launcher.hintN.*` strings: what to type, including the `/u` `/d` `/o`
/// (TR) or `/a` `/f` `/g` (EN) filter shorthands.
const HINT_COUNT: usize = 8;

pub fn toggle(cx: &mut App) {
    if let Some(handle) = cx.global::<Daemon>().launcher_window.filter(|h| cx.windows().contains(h)) {
        let _ = handle.update(cx, |view, window, cx| {
            if let Ok(view) = view.downcast::<Launcher>() {
                view.update(cx, |this, cx| this.close(window, cx));
            } else {
                window.remove_window();
            }
        });
        return;
    }
    open(cx);
}

fn open(cx: &mut App) {
    let session = cx.global::<Daemon>().session;
    let options = window_options(session, card_height(Content::Empty), cx);
    let result = gpui_kit::open_window(options, cx, |window, cx| {
        let view = cx.new(|cx| Launcher::new(window, cx));
        let input = view.read(cx).input.clone();
        input.update(cx, |input, cx| input.focus(window, cx));
        window.activate_window();
        ui::focus_popup(window);
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

/// What sits below the chips.
#[derive(Clone, Copy, PartialEq)]
enum Content {
    Empty,
    Rows(usize),
    NoResults,
}

fn list_height(rows: usize) -> f32 {
    let rows = rows.min(VISIBLE_ROWS) as f32;
    LIST_PADDING * 2. + rows * ROW_HEIGHT + (rows - 1.).max(0.) * ROW_GAP
}

/// Card height including its 1 px border.
fn card_height(content: Content) -> f32 {
    let below = match content {
        Content::Empty => 0.,
        Content::Rows(n) => 1. + list_height(n),
        Content::NoResults => 1. + NO_RESULTS_HEIGHT,
    };
    INPUT_HEIGHT + CHIPS_HEIGHT + below + BORDER * 2.
}

fn window_size(card: f32) -> gpui_kit::Size<gpui_kit::Pixels> {
    size(px(WIDTH + SHADOW_X * 2.), px(card + SHADOW_TOP + SHADOW_BOTTOM))
}

fn window_options(session: Session, card: f32, cx: &App) -> WindowOptions {
    let window_size = window_size(card);
    // Centered, the card's top edge a quarter of the way down the display.
    let mut bounds = Bounds::centered(None, window_size, cx);
    if let Some(display) = cx.primary_display() {
        let area = display.bounds();
        bounds.origin = point(
            area.origin.x + (area.size.width - window_size.width) / 2.,
            area.origin.y + area.size.height / 4. - px(SHADOW_TOP),
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
        app_id: Some(yaltri_platform::APP_ID.into()),
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

/// Chip label and the input placeholder inside that filter.
fn scope_label(scope: Scope) -> &'static str {
    match scope {
        Scope::All => t("launcher.scope.all"),
        Scope::Apps => t("launcher.scope.apps"),
        Scope::Files => t("launcher.scope.files"),
        Scope::Games => t("launcher.scope.games"),
    }
}

fn scope_placeholder(scope: Scope) -> &'static str {
    match scope {
        // The rotating hints stand in for it.
        Scope::All => "",
        Scope::Apps => t("launcher.placeholder.apps"),
        Scope::Files => t("launcher.placeholder.files"),
        Scope::Games => t("launcher.placeholder.games"),
    }
}

fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    1. - (1. - t) * (1. - t)
}

pub struct Launcher {
    input: Entity<InputState>,
    /// Theme the system frame was last styled for.
    framed_dark: Option<bool>,
    /// Which kind of results to show; Tab cycles it, `/o ` style prefixes set it.
    scope: Scope,
    items: Vec<SearchItem>,
    /// The query `items` answer (to tell "no results" from "not searched").
    answered: String,
    selected: usize,
    scroll: ScrollHandle,
    search: Option<Task<()>>,
    /// Becomes true once the window gained focus; losing it afterwards closes
    /// the launcher (but not the initial "not yet active" notification).
    was_active: bool,
    opened: Instant,
    closing: Option<Instant>,
    hint: usize,
    hint_since: Instant,
    _hints: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl Launcher {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(scope_placeholder(Scope::All)));

        let on_change = cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            // "/o palw" turns into the Games scope with "palw" in the box.
            let value = this.input.read(cx).value().to_string();
            if let Some((scope, rest)) = Scope::from_prefix(&value) {
                let rest = rest.to_owned();
                this.input.update(cx, |input, cx| input.set_value(rest, window, cx));
                this.set_scope(scope, window, cx);
                return;
            }
            this.run_search(window, cx);
        });

        let on_activation = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.was_active = true;
            } else if this.was_active {
                this.close(window, cx);
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
            let handled = weak.update(cx, |this, cx| this.on_key(event, window, cx)).unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        });

        // Advance the placeholder hint every period; re-render once before
        // the change so the fade-out runs.
        let hints = cx.spawn(async move |this, cx| {
            let fade_out = Duration::from_millis(HINT_OUT as u64);
            loop {
                cx.background_executor().timer(HINT_PERIOD - fade_out).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
                cx.background_executor().timer(fade_out).await;
                let alive = this.update(cx, |this, cx| {
                    this.hint = (this.hint + 1) % HINT_COUNT;
                    this.hint_since = Instant::now();
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        });

        Self {
            input,
            scope: Scope::All,
            items: Vec::new(),
            answered: String::new(),
            selected: 0,
            scroll: ScrollHandle::new(),
            search: None,
            was_active: false,
            opened: Instant::now(),
            closing: None,
            hint: 0,
            hint_since: Instant::now(),
            framed_dark: None,
            _hints: hints,
            _subscriptions: vec![on_change, on_activation, on_key, theme::follow_system(window, cx)],
        }
    }

    /// Fades out, then removes the window.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing.is_some() {
            return;
        }
        self.closing = Some(Instant::now());
        cx.global_mut::<Daemon>().launcher_window = None;
        window.refresh();
        cx.spawn_in(window, async move |_, cx| {
            cx.background_executor().timer(CLOSE_ANIM).await;
            let _ = cx.update(|window, _| window.remove_window());
        })
        .detach();
    }

    fn on_key(&mut self, event: &KeystrokeEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let ks = &event.keystroke;
        match ks.key.as_str() {
            "escape" => self.close(window, cx),
            "up" => self.move_selection(-1, cx),
            "down" => self.move_selection(1, cx),
            "tab" => self.set_scope(self.scope.cycle(!ks.modifiers.shift), window, cx),
            // Backspace in an empty box drops the scope, like deleting a chip.
            "backspace" if self.scope != Scope::All && self.input.read(cx).value().is_empty() => {
                self.set_scope(Scope::All, window, cx)
            }
            // Ctrl+Enter: show the item in its folder instead of opening it.
            "enter" => self.accept(self.selected, ks.modifiers.control, window, cx),
            _ => return false,
        }
        true
    }

    fn set_scope(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        self.scope = scope;
        self.input.update(cx, |input, cx| input.set_placeholder(scope_placeholder(scope), window, cx));
        self.run_search(window, cx);
        cx.notify();
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.items.is_empty() {
            return;
        }
        let len = self.items.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
        self.scroll.scroll_to_item(self.selected);
        cx.notify();
    }

    fn run_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.input.read(cx).value().to_string();
        let daemon = cx.global::<Daemon>();
        let providers = daemon.providers.clone();
        let history = daemon.history.clone();
        let limit = daemon.settings.launcher.max_results;
        let scope = self.scope;
        let background = cx.background_executor().clone();
        // Replacing the task drops (cancels) the previous, now stale search.
        self.search = Some(cx.spawn_in(window, async move |this, cx| {
            let answered = query.clone();
            let items = background
                .spawn(async move {
                    let boost = |item: &SearchItem| {
                        item.action.history_key().map_or(0, |k| history.lock().map_or(0, |h| h.boost(&k)))
                    };
                    search_all(&providers, &query, scope, limit, &boost)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| this.set_items(items, answered, window, cx));
        }));
    }

    fn content(&self) -> Content {
        if !self.items.is_empty() {
            Content::Rows(self.items.len())
        } else if !self.answered.trim().is_empty() {
            Content::NoResults
        } else {
            Content::Empty
        }
    }

    fn set_items(&mut self, items: Vec<SearchItem>, answered: String, window: &mut Window, cx: &mut Context<Self>) {
        self.items = items;
        self.answered = answered;
        self.selected = 0;
        self.scroll.scroll_to_item(0);
        window.resize(window_size(card_height(self.content())));
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
        self.close(window, cx);
    }
}

const ICON_SIZE: f32 = 28.;

/// The app's own icon when we have one, otherwise a tile with a symbol for
/// the kind of result.
fn item_icon(item: &SearchItem, p: &Palette) -> AnyElement {
    if let Some(path) = &item.icon {
        return img(path.clone()).size(px(ICON_SIZE)).flex_none().object_fit(ObjectFit::Contain).into_any_element();
    }
    let (name, bg, fg) = match item.kind {
        ItemKind::Calc => ("calculator", p.accent, p.accent_fg),
        ItemKind::App => ("app-window", p.surface_raised, p.muted),
        ItemKind::Game => ("gamepad-2", p.surface_raised, p.muted),
        ItemKind::Folder => ("folder", p.surface_raised, p.muted),
        ItemKind::File => ("file", p.surface_raised, p.muted),
    };
    div()
        .size(px(ICON_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(R_SM))
        .bg(bg)
        .child(ui::icon(name, 16., fg))
        .into_any_element()
}

/// Start menu shortcut folders read as "Start menu", not a long path.
fn app_location(raw: &str) -> String {
    if raw.contains("\\Start Menu\\") || raw.contains("/applications") {
        t("launcher.start_menu").to_owned()
    } else {
        raw.to_owned()
    }
}

fn subtitle(item: &SearchItem) -> String {
    match item.kind {
        ItemKind::Calc => tf("launcher.subtitle.calc", &[("expr", &item.subtitle)]),
        ItemKind::App => tf("launcher.subtitle.app", &[("where", &app_location(&item.subtitle))]),
        ItemKind::Game | ItemKind::File => item.subtitle.clone(),
        ItemKind::Folder => tf("launcher.subtitle.folder", &[("path", &item.subtitle)]),
    }
}

fn action_label(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Calc => t("launcher.action.copy"),
        ItemKind::Game => t("launcher.action.launch"),
        _ => t("launcher.action.open"),
    }
}

impl Launcher {
    fn row(&self, ix: usize, item: &SearchItem, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = ix == self.selected;
        let title = div()
            .ts(14., 20.)
            .font_weight(FontWeight::MEDIUM)
            .text_color(p.foreground)
            .truncate()
            .when(item.kind == ItemKind::Calc, |d| d.font_family(theme::mono_font()))
            .child(item.title.clone());
        div()
            .id(ix)
            .h(px(ROW_HEIGHT))
            .flex_none()
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(12.))
            .rounded(px(R_MD))
            .cursor_pointer()
            .when(selected, |row| row.bg(p.list_active))
            .when(!selected, |row| row.hover(|row| row.bg(p.list_hover)))
            .child(item_icon(item, p))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(title)
                    .child(div().ts(12., 16.).text_color(p.muted).truncate().child(subtitle(item))),
            )
            .when(selected, |row| {
                row.child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child(action_label(item.kind))
                        .child(ui::keycap(ui::icon("corner-down-left", 12., p.foreground), 20., p)),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| this.accept(ix, false, window, cx)))
    }

    fn no_results(&self, p: &Palette) -> impl IntoElement {
        let hint = if self.scope == Scope::All {
            div().child(t("launcher.no_results.hint_all")).into_any_element()
        } else {
            div()
                .flex()
                .flex_wrap()
                .justify_center()
                .items_center()
                .gap(px(4.))
                .child(tf("launcher.no_results.hint_filter", &[("filter", scope_label(self.scope))]))
                .child(ui::keycap("Tab", 18., p))
                .child(t("launcher.no_results.hint_tab"))
                .into_any_element()
        };
        div()
            .h(px(NO_RESULTS_HEIGHT))
            .flex_none()
            .pt(px(28.))
            .px(px(16.))
            .pb(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .size(px(40.))
                    .rounded(px(20.))
                    .bg(p.surface_raised)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(ui::icon("search-x", 20., p.muted)),
            )
            .child(
                div()
                    .ts(14., 20.)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.foreground)
                    .child(tf("launcher.no_results.title", &[("query", self.answered.trim())])),
            )
            .child(div().ts(12., 16.).text_color(p.muted).child(hint))
    }

    /// The rotating "what can I type" placeholder, while the box is empty.
    fn hint_overlay(&self, p: &Palette, window: &mut Window) -> impl IntoElement {
        let elapsed = self.hint_since.elapsed().as_secs_f32() * 1000.;
        let period = HINT_PERIOD.as_secs_f32() * 1000.;
        let (opacity, dy) = if elapsed < HINT_IN {
            let k = ease_out(elapsed / HINT_IN);
            (k, 6. * (1. - k))
        } else if elapsed > period - HINT_OUT {
            let k = ease_out((elapsed - (period - HINT_OUT)) / HINT_OUT);
            (1. - k, -6. * k)
        } else {
            (1., 0.)
        };
        // Animate during the transitions; the hint task wakes us for the
        // fade-out and for the next hint.
        if elapsed < HINT_IN || elapsed > period - HINT_OUT {
            window.request_animation_frame();
        }
        let n = self.hint + 1;
        div()
            .absolute()
            .left(px(16. + 20. + 12.))
            .top(px((INPUT_HEIGHT - 24.) / 2. + dy))
            .ts(17., 24.)
            .text_color(p.muted)
            .opacity(opacity)
            .whitespace_nowrap()
            .flex()
            .gap(px(5.))
            .child(t(&format!("launcher.hint{n}.prefix")))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(t(&format!("launcher.hint{n}.example"))))
    }
}

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::of(window);
        if self.framed_dark != Some(p.dark) {
            self.framed_dark = Some(p.dark);
            ui::frame_popup(window, p);
        }
        ui::round_corners(window, R_LG);

        // Open: fade in and slide down 4 px; close: fade out.
        let (opacity, dy) = match self.closing {
            Some(at) => (1. - (at.elapsed().as_secs_f32() / CLOSE_ANIM.as_secs_f32()).clamp(0., 1.), 0.),
            None => {
                let k = ease_out(self.opened.elapsed().as_secs_f32() / OPEN_ANIM.as_secs_f32());
                (k, -4. * (1. - k))
            }
        };
        if opacity < 1. || dy != 0. {
            window.request_animation_frame();
        }

        let show_hints = self.scope == Scope::All && self.input.read(cx).value().is_empty();
        let hint_overlay = show_hints.then(|| self.hint_overlay(p, window));

        let scope = self.scope;
        let chips = Scope::ALL.into_iter().map(|chip| {
            ui::chip(scope_label(chip), scope_label(chip), chip == scope, p)
                .on_click(cx.listener(move |this, _, window, cx| this.set_scope(chip, window, cx)))
        });
        let chips_row = div()
            .h(px(CHIPS_HEIGHT))
            .flex_none()
            .px(px(12.))
            .pb(px(10.))
            .flex()
            .items_center()
            .gap(px(4.))
            .children(chips)
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .text_color(p.muted)
                    .child(ui::keycap("Tab", 18., p))
                    .child(t("launcher.tab_hint")),
            );

        let input_row = div()
            .relative()
            .h(px(INPUT_HEIGHT))
            .flex_none()
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(ui::mark(20., p.muted, p.accent_strong))
            .child(div().flex_1().min_w_0().child(Input::new(&self.input)
                    .appearance(false)
                    .text_size(px(17.))
                    // Text starts right after the mark, where the hints are.
                    .px_0()))
            .children(hint_overlay);

        let content = self.content();
        let rows: Vec<AnyElement> =
            self.items.iter().enumerate().map(|(ix, item)| self.row(ix, item, p, cx).into_any_element()).collect();
        let below = match content {
            Content::Empty => None,
            Content::Rows(n) => Some(
                div()
                    .id("results")
                    .h(px(list_height(n)))
                    .flex_none()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p(px(LIST_PADDING))
                    .flex()
                    .flex_col()
                    .gap(px(ROW_GAP))
                    .children(rows)
                    .into_any_element(),
            ),
            Content::NoResults => Some(self.no_results(p).into_any_element()),
        };

        let card = div()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(p.surface)
            .when(!cfg!(windows), |card| {
                card.border_1().border_color(p.border).rounded(px(R_LG)).shadow(p.shadow_lg.clone())
            })
            .text_color(p.foreground)
            .font_family(theme::ui_font())
            .child(input_row)
            .child(chips_row)
            .when_some(below, |card, below| card.child(div().h(px(1.)).flex_none().bg(p.border)).child(below));

        // Linux: X11 cuts the window to our rounded frame with a 1-bit shape
        // (`ui::round_corners`), so the card's anti-aliased corner pixels
        // that survive the cut show whatever is under them. On an opaque
        // swapchain (NVIDIA) that is black, which left a dark, jagged arc.
        // Backing the card in its border color keeps the cut edge clean.
        let card = div().when(cfg!(target_os = "linux"), |d| d.bg(p.border)).child(card);

        div().size_full().pt(px(SHADOW_TOP + dy)).px(px(SHADOW_X)).opacity(opacity).child(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hint_has_strings() {
        for n in 1..=HINT_COUNT {
            for part in ["prefix", "example"] {
                let key = format!("launcher.hint{n}.{part}");
                assert_ne!(t(&key), key, "missing {key}");
            }
        }
    }
}
