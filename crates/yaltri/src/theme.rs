//! Design tokens from `design/yaltri-tokens.json` as GPUI values, picked by
//! the OS light/dark setting. Every window re-renders when it changes.

use std::collections::HashMap;
use std::sync::LazyLock;

use gpui_kit::{App, BoxShadow, Context, Hsla, SharedString, Subscription, Window, WindowAppearance, point, px, rgba};
use serde::Deserialize;

const TOKENS_JSON: &str = include_str!("../../../design/yaltri-tokens.json");

/// Corner radii (`radius` tokens).
pub const R_XS: f32 = 4.;
pub const R_SM: f32 = 6.;
pub const R_MD: f32 = 8.;
pub const R_LG: f32 = 12.;
/// Our own window frame (Linux client decorations).
pub const R_WINDOW: f32 = 10.;

/// The system UI font. Windows 11 ships Segoe UI Variable; GNOME uses
/// Cantarell, Ubuntu its own font, and fontconfig falls back from there.
pub fn ui_font() -> SharedString {
    if cfg!(windows) { "Segoe UI Variable Text".into() } else { "Cantarell".into() }
}

/// Monospace for color values and paths.
pub fn mono_font() -> SharedString {
    if cfg!(windows) { "Cascadia Mono".into() } else { "DejaVu Sans Mono".into() }
}

#[derive(Clone, Debug)]
pub struct Palette {
    pub dark: bool,
    pub background: Hsla,
    pub surface: Hsla,
    pub surface_raised: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub foreground: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
    pub accent_fg: Hsla,
    pub accent_strong: Hsla,
    pub list_hover: Hsla,
    pub list_active: Hsla,
    pub success: Hsla,
    pub danger: Hsla,
    pub danger_bg: Hsla,
    pub toast_bg: Hsla,
    pub toast_fg: Hsla,
    pub shadow_sm: Vec<BoxShadow>,
    pub shadow_md: Vec<BoxShadow>,
    pub shadow_lg: Vec<BoxShadow>,
}

impl Palette {
    /// Scrim behind dialogs.
    pub fn scrim(&self) -> Hsla {
        if self.dark { hex("#00000073") } else { hex("#14161C40") }
    }

    /// Swatch outline that reads on any color.
    pub fn swatch_border(&self) -> Hsla {
        hex("#7F7F7F59")
    }

    /// The recording ring: accent 35 % over the surface.
    pub fn ring(&self) -> Hsla {
        mix(self.surface, self.accent, 0.35)
    }
}

#[derive(Deserialize)]
struct Tokens {
    color: HashMap<String, HashMap<String, String>>,
    shadow: HashMap<String, HashMap<String, Vec<ShadowToken>>>,
}

#[derive(Deserialize)]
struct ShadowToken {
    #[serde(default)]
    y: f32,
    blur: f32,
    color: String,
}

/// `#RRGGBB` or `#RRGGBBAA`.
pub fn hex(s: &str) -> Hsla {
    let digits = s.trim_start_matches('#');
    let value = u32::from_str_radix(digits, 16).unwrap_or(0);
    let rgba_value = if digits.len() == 6 { (value << 8) | 0xFF } else { value };
    rgba(rgba_value).into()
}

/// `a` blended towards `b` by `t` (0 = a, 1 = b), in sRGB like CSS.
pub fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b) = (gpui_kit::Rgba::from(a), gpui_kit::Rgba::from(b));
    gpui_kit::Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
    .into()
}

fn load(mode: &str) -> Palette {
    let tokens: Tokens = serde_json::from_str(TOKENS_JSON).expect("design/yaltri-tokens.json is valid");
    let colors = &tokens.color[mode];
    let c = |name: &str| hex(colors.get(name).unwrap_or_else(|| panic!("missing color token {mode}.{name}")));
    let shadows = &tokens.shadow[mode];
    let s = |name: &str| -> Vec<BoxShadow> {
        shadows[name]
            .iter()
            .map(|t| BoxShadow {
                color: hex(&t.color),
                offset: point(px(0.), px(t.y)),
                blur_radius: px(t.blur),
                spread_radius: px(0.),
                inset: false,
            })
            .collect()
    };
    Palette {
        dark: mode == "dark",
        background: c("background"),
        surface: c("surface"),
        surface_raised: c("surface-raised"),
        border: c("border"),
        border_strong: c("border-strong"),
        foreground: c("foreground"),
        muted: c("muted-foreground"),
        accent: c("accent"),
        accent_fg: c("accent-foreground"),
        accent_strong: c("accent-strong"),
        list_hover: c("list-hover"),
        list_active: c("list-active"),
        success: c("success"),
        danger: c("danger"),
        danger_bg: c("danger-bg"),
        toast_bg: c("toast-bg"),
        toast_fg: c("toast-foreground"),
        shadow_sm: s("sm"),
        shadow_md: s("md"),
        shadow_lg: s("lg"),
    }
}

static DARK: LazyLock<Palette> = LazyLock::new(|| load("dark"));
static LIGHT: LazyLock<Palette> = LazyLock::new(|| load("light"));

pub fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(appearance, WindowAppearance::Dark | WindowAppearance::VibrantDark)
}

pub fn palette(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

/// Palette for the OS appearance as seen by `window`.
pub fn of(window: &Window) -> &'static Palette {
    palette(is_dark(window.appearance()))
}

/// Palette for the OS appearance (outside a window).
pub fn of_app(cx: &App) -> &'static Palette {
    palette(is_dark(cx.window_appearance()))
}

/// Re-renders the view (and refreshes the component theme) when the OS
/// switches between light and dark.
pub fn follow_system<V: 'static>(window: &mut Window, cx: &mut Context<V>) -> Subscription {
    cx.observe_window_appearance(window, |_, _, cx| {
        sync_components(cx);
        cx.notify();
    })
}

/// Points the component library (text input caret, selection, popovers) at
/// our tokens, for the current OS appearance.
pub fn sync_components(cx: &mut App) {
    gpui_kit::component::Theme::sync_system_appearance(None, cx);
    let p = of_app(cx);
    let theme = gpui_kit::component::Theme::global_mut(cx);
    theme.font_family = ui_font();
    theme.mono_font_family = mono_font();
    theme.radius = px(R_SM);
    theme.radius_lg = px(R_LG);
    let colors = &mut theme.colors;
    colors.background = p.background;
    colors.foreground = p.foreground;
    colors.muted_foreground = p.muted;
    colors.border = p.border;
    colors.input = p.border;
    colors.ring = p.accent_strong;
    colors.caret = p.accent_strong;
    colors.selection = p.accent.opacity(0.35);
    colors.popover = p.surface;
    colors.popover_foreground = p.foreground;
    colors.list_hover = p.list_hover;
    colors.list_active = p.list_active;
}
