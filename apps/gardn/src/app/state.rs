use crate::config::{
    Keybinds, NewTerminalCwdConfig, PaneBorderAgentInfoConfig, RightClickPassthroughModifierConfig,
    SoundConfig, StatusIndicatorStyle, TerminalAccent, ThemeConfig, ThemeMode, ToastConfig,
    ToastDelivery,
};
use crate::detect::AgentState;
use crossterm::event::{KeyCode, KeyModifiers, MouseEvent};
use ratatui::layout::{Direction, Rect};
use ratatui::style::Color;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::execution_host::protocol::SessionNamespaceId;
use crate::layout::{PaneId, PaneInfo, SplitBorder};
use crate::selection::Selection;
use crate::terminal_theme::{TerminalTheme, ThemeAppearance};

pub(crate) type InstalledPluginRegistry =
    std::collections::HashMap<String, crate::api::schema::InstalledPluginInfo>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PluginPaneRecord {
    pub plugin_id: String,
    pub entrypoint: String,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingPaneMouseMotion {
    pub ws_idx: usize,
    pub pane_id: PaneId,
    pub inner_rect: Rect,
    pub mouse: MouseEvent,
    pub host_pixels: Option<(u32, u32)>,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingPaneWheel {
    pub ws_idx: usize,
    pub pane_id: PaneId,
    pub inner_rect: Rect,
    pub mouse: MouseEvent,
    pub host_pixels: Option<(u32, u32)>,
    pub up: u32,
    pub down: u32,
    pub left: u32,
    pub right: u32,
}

// ---------------------------------------------------------------------------
// Selection autoscroll types
// ---------------------------------------------------------------------------

/// Direction of automatic scrolling during text selection drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectionAutoscrollDirection {
    Up,
    Down,
}

/// State for automatic scrolling during text selection drag.
///
/// When the cursor hovers in the 1-row hot zone at the top or bottom edge
/// of a pane (or outside the pane), this struct captures the direction and
/// last known mouse position so a recurring 30ms tick can continue scrolling
/// and extending the selection even when the mouse is not moving.
#[derive(Clone, Debug)]
pub(crate) struct SelectionAutoscroll {
    pub direction: SelectionAutoscrollDirection,
    pub last_mouse_screen_col: u16,
    pub last_mouse_screen_row: u16,
    pub inner_rect: Rect,
}

#[derive(Clone)]
pub(crate) struct RightClickPassthroughGesture {
    pub ws_idx: usize,
    pub pane_info: PaneInfo,
    pub modifiers: KeyModifiers,
}
use crate::workspace::Workspace;

use crate::github::GithubRepositoryScope;
static NEXT_GROUP_ID: AtomicU64 = AtomicU64::new(1);

pub const DEFAULT_GROUP_ICON: &str = "☀";
pub const GROUP_ICONS: &[&str] = &[
    "☀", "☁", "☂", "♥", "♪", "⚑", "⚙", "☎", "☄", "☘", "✉", "✿", "✂", "✎", "✚", "⊕", "▥", "⌁",
];

pub(crate) fn generate_group_id() -> String {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or(0);
    let counter = NEXT_GROUP_ID.fetch_add(1, Ordering::Relaxed);
    format!("g{micros:x}{counter:x}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub accent: Option<TerminalAccent>,
    pub default_location: Option<crate::execution_host::ResourceLocation>,
    pub favorite_agent_profile_ids: Vec<String>,
    pub default_agent_profile_id: Option<String>,
    pub github_organization: Option<GithubOrganization>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GithubOrganization(String);

impl GithubOrganization {
    pub fn parse(input: &str) -> Result<Option<Self>, String> {
        let value = input.trim();
        if value.is_empty() {
            return Ok(None);
        }
        let valid = (1..=39).contains(&value.len())
            && value
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
            && !value.starts_with('-')
            && !value.ends_with('-')
            && !value.contains("--");
        valid
            .then(|| Self(value.to_string()))
            .ok_or_else(|| {
                "GitHub organization must be 1-39 ASCII letters or numbers separated by single hyphens"
                    .to_string()
            })
            .map(Some)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Group {
    pub fn default_group() -> Self {
        Self {
            id: crate::workspace::DEFAULT_GROUP_ID.to_string(),
            name: "Group 1".to_string(),
            icon: DEFAULT_GROUP_ICON.to_string(),
            accent: None,
            default_location: None,
            favorite_agent_profile_ids: Vec::new(),
            default_agent_profile_id: None,
            github_organization: None,
        }
    }
}

pub fn normalize_group_icon(icon: &str) -> String {
    GROUP_ICONS
        .iter()
        .copied()
        .find(|candidate| *candidate == icon)
        .unwrap_or(DEFAULT_GROUP_ICON)
        .to_string()
}

// ---------------------------------------------------------------------------
// Theme palette — all UI colors in one place, ready for theming
// ---------------------------------------------------------------------------

/// All colors used by the UI. Derived from a base accent color for now,
/// but structured so a full theme system can replace it later.
#[derive(Clone)]
#[allow(dead_code)] // all fields defined for theming — some used later
pub struct Palette {
    /// Primary accent (highlight, active borders).
    pub accent: Color,
    /// Background for floating panels, overlays, and modals.
    pub panel_bg: Color,
    /// Subtle surface background for selected/focused items.
    pub surface0: Color,
    /// Slightly lighter surface for hover/active states.
    pub surface1: Color,
    /// Very dim surface for separators.
    pub surface_dim: Color,
    /// Muted text (secondary info, numbers).
    pub overlay0: Color,
    /// Slightly brighter overlay text.
    pub overlay1: Color,
    /// Main text color — soft white.
    pub text: Color,
    /// Subdued text (workspace numbers, dim labels).
    pub subtext0: Color,
    /// Branch name / special label color.
    pub mauve: Color,
    /// Done / idle states.
    pub green: Color,
    /// Working / running states.
    pub yellow: Color,
    /// Needs attention / blocked states.
    pub red: Color,
    /// Unseen / done notification accent.
    pub blue: Color,
    /// Notification accent / unseen markers.
    pub teal: Color,
    /// Interrupted / warning states.
    pub peach: Color,
}

impl Palette {
    #[allow(clippy::too_many_arguments)]
    fn catppuccin_palette(
        accent: Color,
        panel_bg: Color,
        surface0: Color,
        surface1: Color,
        surface_dim: Color,
        overlay0: Color,
        overlay1: Color,
        text: Color,
        subtext0: Color,
        mauve: Color,
        green: Color,
        yellow: Color,
        red: Color,
        blue: Color,
        teal: Color,
        peach: Color,
    ) -> Self {
        Self {
            accent,
            panel_bg,
            surface0,
            surface1,
            surface_dim,
            overlay0,
            overlay1,
            text,
            subtext0,
            mauve,
            green,
            yellow,
            red,
            blue,
            teal,
            peach,
        }
    }

    /// Catppuccin Mocha.
    pub fn catppuccin() -> Self {
        Self::catppuccin_palette(
            Self::rgb(137, 180, 250),
            Self::rgb(30, 30, 46),
            Self::rgb(49, 50, 68),
            Self::rgb(69, 71, 90),
            Self::rgb(24, 24, 37),
            Self::rgb(108, 112, 134),
            Self::rgb(127, 132, 156),
            Self::rgb(205, 214, 244),
            Self::rgb(166, 173, 200),
            Self::rgb(203, 166, 247),
            Self::rgb(166, 227, 161),
            Self::rgb(249, 226, 175),
            Self::rgb(243, 139, 168),
            Self::rgb(137, 180, 250),
            Self::rgb(148, 226, 213),
            Self::rgb(250, 179, 135),
        )
    }

    /// Catppuccin Latte.
    pub fn catppuccin_light() -> Self {
        Self::catppuccin_palette(
            Self::rgb(30, 102, 245),
            Self::rgb(239, 241, 245),
            Self::rgb(204, 208, 218),
            Self::rgb(188, 192, 204),
            Self::rgb(230, 233, 239),
            Self::rgb(156, 160, 176),
            Self::rgb(140, 143, 161),
            Self::rgb(76, 79, 105),
            Self::rgb(108, 111, 133),
            Self::rgb(136, 57, 239),
            Self::rgb(64, 160, 43),
            Self::rgb(223, 142, 29),
            Self::rgb(210, 15, 57),
            Self::rgb(30, 102, 245),
            Self::rgb(23, 146, 153),
            Self::rgb(254, 100, 11),
        )
    }

    pub fn catppuccin_latte() -> Self {
        Self::catppuccin_light()
    }

    /// Catppuccin Frappé.
    pub fn catppuccin_frappe() -> Self {
        Self::catppuccin_palette(
            Self::rgb(140, 170, 238),
            Self::rgb(48, 52, 70),
            Self::rgb(65, 69, 89),
            Self::rgb(81, 87, 109),
            Self::rgb(41, 44, 60),
            Self::rgb(115, 121, 148),
            Self::rgb(131, 139, 167),
            Self::rgb(198, 208, 245),
            Self::rgb(165, 173, 206),
            Self::rgb(202, 158, 230),
            Self::rgb(166, 209, 137),
            Self::rgb(229, 200, 144),
            Self::rgb(231, 130, 132),
            Self::rgb(140, 170, 238),
            Self::rgb(129, 200, 190),
            Self::rgb(239, 159, 118),
        )
    }

    /// Catppuccin Macchiato.
    pub fn catppuccin_macchiato() -> Self {
        Self::catppuccin_palette(
            Self::rgb(138, 173, 244),
            Self::rgb(36, 39, 58),
            Self::rgb(54, 58, 79),
            Self::rgb(73, 77, 100),
            Self::rgb(30, 32, 48),
            Self::rgb(110, 115, 141),
            Self::rgb(128, 135, 162),
            Self::rgb(202, 211, 245),
            Self::rgb(165, 173, 203),
            Self::rgb(198, 160, 246),
            Self::rgb(166, 218, 149),
            Self::rgb(238, 212, 159),
            Self::rgb(237, 135, 150),
            Self::rgb(138, 173, 244),
            Self::rgb(139, 213, 202),
            Self::rgb(245, 169, 127),
        )
    }

    /// System — respect the host terminal defaults and ANSI palette.
    pub fn system(
        host_theme: TerminalTheme,
        appearance: crate::terminal_theme::ThemeAppearance,
        accent: TerminalAccent,
    ) -> Self {
        let host_fg = host_theme.foreground.map(Self::terminal_color);
        let host_bg = host_theme.background.map(Self::terminal_color);

        let text = host_fg.unwrap_or(Color::Reset);
        let overlay0 = Self::neutral_from_foreground(host_fg, host_bg, appearance, 0.45);
        let overlay1 = Self::neutral_from_foreground(host_fg, host_bg, appearance, 0.20);
        let subtext0 = Self::neutral_from_foreground(host_fg, host_bg, appearance, 0.35);

        Self {
            accent: Self::terminal_palette_color(
                host_theme,
                accent.ansi_index(),
                accent.fallback_color(),
            ),
            panel_bg: Color::Reset,
            surface0: host_bg
                .map(|color| Self::surface_from_background(color, appearance, 0.08))
                .unwrap_or(Color::Reset),
            surface1: host_bg
                .map(|color| Self::surface_from_background(color, appearance, 0.14))
                .unwrap_or(Color::DarkGray),
            surface_dim: host_bg
                .map(|color| Self::surface_from_background(color, appearance, 0.05))
                .unwrap_or(Color::DarkGray),
            overlay0,
            overlay1,
            text,
            subtext0,
            mauve: Self::terminal_palette_color(host_theme, 5, Color::Magenta),
            green: Self::terminal_palette_color(host_theme, 2, Color::Green),
            yellow: Self::terminal_palette_color(host_theme, 3, Color::Yellow),
            red: Self::terminal_palette_color(host_theme, 1, Color::LightRed),
            blue: Self::terminal_palette_color(host_theme, 4, Color::Blue),
            teal: Self::terminal_palette_color(host_theme, 6, Color::Cyan),
            peach: Self::terminal_palette_color(host_theme, 3, Color::Yellow),
        }
    }

    fn terminal_color(color: crate::terminal_theme::RgbColor) -> Color {
        Color::Rgb(color.r, color.g, color.b)
    }

    pub fn theme_accent_color(&self, accent: TerminalAccent) -> Color {
        match accent {
            TerminalAccent::Blue => self.blue,
            TerminalAccent::Magenta => self.mauve,
            TerminalAccent::Cyan => self.teal,
            TerminalAccent::Green => self.green,
            TerminalAccent::Yellow => self.yellow,
            TerminalAccent::Red => self.red,
        }
    }

    fn terminal_palette_color(theme: TerminalTheme, index: usize, fallback: Color) -> Color {
        theme
            .palette
            .get(index)
            .and_then(|color| color.map(Self::terminal_color))
            .unwrap_or(fallback)
    }

    fn neutral_from_foreground(
        foreground: Option<Color>,
        background: Option<Color>,
        appearance: crate::terminal_theme::ThemeAppearance,
        amount_toward_background: f32,
    ) -> Color {
        let Some(Color::Rgb(fr, fg, fb)) = foreground else {
            return match appearance {
                ThemeAppearance::Light => Color::DarkGray,
                ThemeAppearance::Dark => Color::Gray,
            };
        };
        let Some(Color::Rgb(br, bg, bb)) = background else {
            return Color::Rgb(fr, fg, fb);
        };

        let blend = |fg: u8, bg: u8| -> u8 {
            let value = fg as f32 + (bg as f32 - fg as f32) * amount_toward_background;
            value.round().clamp(0.0, 255.0) as u8
        };

        Color::Rgb(blend(fr, br), blend(fg, bg), blend(fb, bb))
    }

    fn surface_from_background(
        color: Color,
        appearance: crate::terminal_theme::ThemeAppearance,
        amount: f32,
    ) -> Color {
        let Color::Rgb(r, g, b) = color else {
            return color;
        };
        let adjust = |channel: u8| -> u8 {
            let channel = channel as f32;
            let value = match appearance {
                ThemeAppearance::Light => channel * (1.0 - amount),
                ThemeAppearance::Dark => channel + (255.0 - channel) * amount,
            };
            value.round().clamp(0.0, 255.0) as u8
        };
        Color::Rgb(adjust(r), adjust(g), adjust(b))
    }
    /// Terminal 16-color theme.
    pub fn terminal() -> Self {
        Self {
            accent: Color::Blue,
            panel_bg: Color::Reset,
            surface0: Color::Reset,
            surface1: Color::DarkGray,
            surface_dim: Color::DarkGray,
            overlay0: Color::Gray,
            overlay1: Color::White,
            text: Color::Reset,
            subtext0: Color::Gray,
            mauve: Color::Gray,
            green: Color::Green,
            yellow: Color::Yellow,
            red: Color::LightRed,
            blue: Color::Blue,
            teal: Color::Cyan,
            peach: Color::Yellow,
        }
    }

    /// Tokyo Night.
    pub fn tokyo_night() -> Self {
        Self::omarchy_palette(
            Self::rgb(122, 162, 247),
            Self::rgb(169, 177, 214),
            Self::rgb(26, 27, 38),
            Self::rgb(50, 52, 74),
            Self::rgb(247, 118, 142),
            Self::rgb(158, 206, 106),
            Self::rgb(224, 175, 104),
            Self::rgb(122, 162, 247),
            Self::rgb(173, 142, 230),
            Self::rgb(68, 157, 171),
            Self::rgb(120, 124, 153),
            Self::rgb(68, 75, 106),
        )
    }

    /// Tokyo Night Day.
    pub fn tokyo_night_light() -> Self {
        Self {
            accent: Color::Rgb(52, 84, 138),
            panel_bg: Color::Rgb(213, 214, 219),
            surface0: Color::Rgb(188, 189, 194),
            surface1: Color::Rgb(172, 173, 178),
            surface_dim: Color::Rgb(203, 204, 209),
            overlay0: Color::Rgb(116, 124, 149),
            overlay1: Color::Rgb(97, 103, 125),
            text: Color::Rgb(52, 59, 88),
            subtext0: Color::Rgb(86, 95, 137),
            mauve: Color::Rgb(90, 74, 120),
            green: Color::Rgb(72, 94, 48),
            yellow: Color::Rgb(143, 94, 21),
            red: Color::Rgb(140, 67, 81),
            blue: Color::Rgb(52, 84, 138),
            teal: Color::Rgb(51, 99, 122),
            peach: Color::Rgb(150, 80, 39),
        }
    }

    pub fn tokyo_night_day() -> Self {
        Self::tokyo_night_light()
    }

    /// Dracula — purple/pink/green.
    pub fn dracula() -> Self {
        Self {
            accent: Color::Rgb(189, 147, 249), // purple
            panel_bg: Color::Rgb(40, 42, 54),
            surface0: Color::Rgb(68, 71, 90),
            surface1: Color::Rgb(98, 114, 164),
            surface_dim: Color::Rgb(40, 42, 54),
            overlay0: Color::Rgb(98, 114, 164),
            overlay1: Color::Rgb(130, 140, 180),
            text: Color::Rgb(248, 248, 242),
            subtext0: Color::Rgb(210, 210, 220),
            mauve: Color::Rgb(255, 121, 198), // pink
            green: Color::Rgb(80, 250, 123),
            yellow: Color::Rgb(241, 250, 140),
            red: Color::Rgb(255, 85, 85),
            blue: Color::Rgb(139, 233, 253), // cyan-ish
            teal: Color::Rgb(139, 233, 253),
            peach: Color::Rgb(255, 184, 108),
        }
    }

    /// Nord.
    pub fn nord() -> Self {
        Self::omarchy_palette(
            Self::rgb(129, 161, 193),
            Self::rgb(216, 222, 233),
            Self::rgb(46, 52, 64),
            Self::rgb(59, 66, 82),
            Self::rgb(191, 97, 106),
            Self::rgb(163, 190, 140),
            Self::rgb(235, 203, 139),
            Self::rgb(129, 161, 193),
            Self::rgb(180, 142, 173),
            Self::rgb(136, 192, 208),
            Self::rgb(229, 233, 240),
            Self::rgb(76, 86, 106),
        )
    }

    /// Gruvbox.
    pub fn gruvbox() -> Self {
        Self::omarchy_palette(
            Self::rgb(125, 174, 163),
            Self::rgb(212, 190, 152),
            Self::rgb(40, 40, 40),
            Self::rgb(60, 56, 54),
            Self::rgb(234, 105, 98),
            Self::rgb(169, 182, 101),
            Self::rgb(216, 166, 87),
            Self::rgb(125, 174, 163),
            Self::rgb(211, 134, 155),
            Self::rgb(137, 180, 130),
            Self::rgb(212, 190, 152),
            Self::rgb(60, 56, 54),
        )
    }

    /// Gruvbox Light.
    pub fn gruvbox_light() -> Self {
        Self {
            accent: Color::Rgb(181, 118, 20),
            panel_bg: Color::Rgb(251, 241, 199),
            surface0: Color::Rgb(235, 219, 178),
            surface1: Color::Rgb(213, 196, 161),
            surface_dim: Color::Rgb(242, 229, 188),
            overlay0: Color::Rgb(146, 131, 116),
            overlay1: Color::Rgb(124, 111, 100),
            text: Color::Rgb(60, 56, 54),
            subtext0: Color::Rgb(80, 73, 69),
            mauve: Color::Rgb(177, 98, 134),
            green: Color::Rgb(121, 116, 14),
            yellow: Color::Rgb(181, 118, 20),
            red: Color::Rgb(157, 0, 6),
            blue: Color::Rgb(7, 102, 120),
            teal: Color::Rgb(66, 123, 88),
            peach: Color::Rgb(175, 58, 3),
        }
    }

    /// One Dark — Atom's classic dark theme.
    pub fn one_dark() -> Self {
        Self {
            accent: Color::Rgb(97, 175, 239), // blue
            panel_bg: Color::Rgb(40, 44, 52),
            surface0: Color::Rgb(44, 49, 58),
            surface1: Color::Rgb(62, 68, 81),
            surface_dim: Color::Rgb(40, 44, 52),
            overlay0: Color::Rgb(92, 99, 112),
            overlay1: Color::Rgb(115, 122, 135),
            text: Color::Rgb(171, 178, 191),
            subtext0: Color::Rgb(150, 156, 168),
            mauve: Color::Rgb(198, 120, 221),
            green: Color::Rgb(152, 195, 121),
            yellow: Color::Rgb(229, 192, 123),
            red: Color::Rgb(224, 108, 117),
            blue: Color::Rgb(97, 175, 239),
            teal: Color::Rgb(86, 182, 194),
            peach: Color::Rgb(209, 154, 102),
        }
    }

    /// Atom One Light.
    pub fn one_light() -> Self {
        Self {
            accent: Color::Rgb(64, 120, 242),
            panel_bg: Color::Rgb(250, 250, 250),
            surface0: Color::Rgb(230, 230, 230),
            surface1: Color::Rgb(210, 210, 210),
            surface_dim: Color::Rgb(238, 238, 238),
            overlay0: Color::Rgb(160, 161, 167),
            overlay1: Color::Rgb(105, 108, 117),
            text: Color::Rgb(56, 58, 66),
            subtext0: Color::Rgb(92, 99, 112),
            mauve: Color::Rgb(166, 38, 164),
            green: Color::Rgb(80, 161, 79),
            yellow: Color::Rgb(193, 132, 1),
            red: Color::Rgb(228, 86, 73),
            blue: Color::Rgb(64, 120, 242),
            teal: Color::Rgb(1, 132, 188),
            peach: Color::Rgb(152, 104, 1),
        }
    }

    /// Solarized Dark — Ethan Schoonover's classic.
    pub fn solarized() -> Self {
        Self {
            accent: Color::Rgb(38, 139, 210), // blue
            panel_bg: Color::Rgb(0, 43, 54),
            surface0: Color::Rgb(7, 54, 66),
            surface1: Color::Rgb(88, 110, 117),
            surface_dim: Color::Rgb(0, 43, 54),
            overlay0: Color::Rgb(88, 110, 117),
            overlay1: Color::Rgb(101, 123, 131),
            text: Color::Rgb(147, 161, 161),
            subtext0: Color::Rgb(131, 148, 150),
            mauve: Color::Rgb(211, 54, 130),
            green: Color::Rgb(133, 153, 0),
            yellow: Color::Rgb(181, 137, 0),
            red: Color::Rgb(220, 50, 47),
            blue: Color::Rgb(38, 139, 210),
            teal: Color::Rgb(42, 161, 152),
            peach: Color::Rgb(203, 75, 22),
        }
    }

    /// Solarized Light.
    pub fn solarized_light() -> Self {
        Self {
            accent: Color::Rgb(38, 139, 210),
            panel_bg: Color::Rgb(253, 246, 227),
            surface0: Color::Rgb(238, 232, 213),
            surface1: Color::Rgb(147, 161, 161),
            surface_dim: Color::Rgb(246, 239, 219),
            overlay0: Color::Rgb(147, 161, 161),
            overlay1: Color::Rgb(101, 123, 131),
            text: Color::Rgb(88, 110, 117),
            subtext0: Color::Rgb(101, 123, 131),
            mauve: Color::Rgb(211, 54, 130),
            green: Color::Rgb(133, 153, 0),
            yellow: Color::Rgb(181, 137, 0),
            red: Color::Rgb(220, 50, 47),
            blue: Color::Rgb(38, 139, 210),
            teal: Color::Rgb(42, 161, 152),
            peach: Color::Rgb(203, 75, 22),
        }
    }

    /// Kanagawa.
    pub fn kanagawa() -> Self {
        Self::omarchy_palette(
            Self::rgb(126, 156, 216),
            Self::rgb(220, 215, 186),
            Self::rgb(31, 31, 40),
            Self::rgb(9, 6, 24),
            Self::rgb(195, 64, 67),
            Self::rgb(118, 148, 106),
            Self::rgb(192, 163, 110),
            Self::rgb(126, 156, 216),
            Self::rgb(149, 127, 184),
            Self::rgb(106, 149, 137),
            Self::rgb(200, 192, 147),
            Self::rgb(114, 113, 105),
        )
    }

    /// Kanagawa Lotus — the light Kanagawa variant.
    pub fn kanagawa_lotus() -> Self {
        Self {
            accent: Color::Rgb(77, 105, 155),
            panel_bg: Color::Rgb(242, 236, 188),
            surface0: Color::Rgb(220, 213, 172),
            surface1: Color::Rgb(201, 203, 209),
            surface_dim: Color::Rgb(213, 206, 163),
            overlay0: Color::Rgb(160, 156, 172),
            overlay1: Color::Rgb(138, 137, 128),
            text: Color::Rgb(84, 84, 100),
            subtext0: Color::Rgb(67, 67, 108),
            mauve: Color::Rgb(98, 76, 131),
            green: Color::Rgb(111, 137, 78),
            yellow: Color::Rgb(119, 113, 63),
            red: Color::Rgb(200, 64, 83),
            blue: Color::Rgb(77, 105, 155),
            teal: Color::Rgb(78, 140, 162),
            peach: Color::Rgb(204, 109, 0),
        }
    }

    /// Rosé Pine — muted, elegant.
    pub fn rose_pine() -> Self {
        Self {
            accent: Color::Rgb(196, 167, 231), // iris
            panel_bg: Color::Rgb(25, 23, 36),
            surface0: Color::Rgb(31, 29, 46),
            surface1: Color::Rgb(38, 35, 58),
            surface_dim: Color::Rgb(38, 35, 58),
            overlay0: Color::Rgb(110, 106, 134),
            overlay1: Color::Rgb(144, 140, 170),
            text: Color::Rgb(224, 222, 244),
            subtext0: Color::Rgb(200, 197, 220),
            mauve: Color::Rgb(196, 167, 231),  // iris
            green: Color::Rgb(49, 116, 143),   // pine
            yellow: Color::Rgb(246, 193, 119), // gold
            red: Color::Rgb(235, 111, 146),    // love
            blue: Color::Rgb(49, 116, 143),    // pine
            teal: Color::Rgb(156, 207, 216),   // foam
            peach: Color::Rgb(234, 154, 151),  // rose
        }
    }

    /// Rose Pine Dawn.
    pub fn rose_pine_dawn() -> Self {
        Self::omarchy_palette(
            Self::rgb(86, 148, 159),
            Self::rgb(87, 82, 121),
            Self::rgb(250, 244, 237),
            Self::rgb(242, 233, 225),
            Self::rgb(180, 99, 122),
            Self::rgb(40, 105, 131),
            Self::rgb(234, 157, 52),
            Self::rgb(86, 148, 159),
            Self::rgb(144, 122, 169),
            Self::rgb(215, 130, 126),
            Self::rgb(87, 82, 121),
            Self::rgb(152, 147, 165),
        )
    }

    /// Vesper — minimal high-contrast monochrome with peach and mint accents.
    pub fn vesper() -> Self {
        Self {
            accent: Color::Rgb(255, 199, 153),
            panel_bg: Color::Rgb(26, 26, 26),
            surface0: Color::Rgb(35, 35, 35),
            surface1: Color::Rgb(40, 40, 40),
            surface_dim: Color::Rgb(16, 16, 16),
            overlay0: Color::Rgb(92, 92, 92),
            overlay1: Color::Rgb(126, 126, 126),
            text: Color::Rgb(255, 255, 255),
            subtext0: Color::Rgb(160, 160, 160),
            mauve: Color::Rgb(255, 209, 168),
            green: Color::Rgb(153, 255, 228),
            yellow: Color::Rgb(255, 199, 153),
            red: Color::Rgb(255, 128, 128),
            blue: Color::Rgb(176, 176, 176),
            teal: Color::Rgb(102, 221, 204),
            peach: Color::Rgb(255, 199, 153),
        }
    }
    fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::Rgb(r, g, b)
    }

    // Monokai variants share the same token layout; named arguments keep
    // the copied upstream palette values auditable.
    #[allow(clippy::too_many_arguments)]
    fn monokai_palette(
        accent: Color,
        panel_bg: Color,
        surface0: Color,
        surface1: Color,
        surface_dim: Color,
        overlay0: Color,
        overlay1: Color,
        text: Color,
        subtext0: Color,
        red: Color,
        green: Color,
        yellow: Color,
        peach: Color,
        mauve: Color,
        teal: Color,
    ) -> Self {
        Self {
            accent,
            panel_bg,
            surface0,
            surface1,
            surface_dim,
            overlay0,
            overlay1,
            text,
            subtext0,
            mauve,
            green,
            yellow,
            red,
            blue: teal,
            teal,
            peach,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn omarchy_palette(
        accent: Color,
        foreground: Color,
        background: Color,
        color0: Color,
        color1: Color,
        color2: Color,
        color3: Color,
        color4: Color,
        color5: Color,
        color6: Color,
        color7: Color,
        color8: Color,
    ) -> Self {
        Self {
            accent,
            panel_bg: background,
            surface0: color0,
            surface1: color8,
            surface_dim: background,
            overlay0: color8,
            overlay1: color7,
            text: foreground,
            subtext0: color7,
            mauve: color5,
            green: color2,
            yellow: color3,
            red: color1,
            blue: color4,
            teal: color6,
            peach: color3,
        }
    }

    /// Monokai Pro.
    pub fn monokai_pro() -> Self {
        Self::monokai_palette(
            Self::rgb(255, 216, 102),
            Self::rgb(34, 31, 34),
            Self::rgb(45, 42, 46),
            Self::rgb(64, 62, 65),
            Self::rgb(25, 24, 26),
            Self::rgb(114, 112, 114),
            Self::rgb(147, 146, 147),
            Self::rgb(252, 252, 250),
            Self::rgb(193, 192, 192),
            Self::rgb(255, 97, 136),
            Self::rgb(169, 220, 118),
            Self::rgb(255, 216, 102),
            Self::rgb(252, 152, 103),
            Self::rgb(171, 157, 242),
            Self::rgb(120, 220, 232),
        )
    }

    /// Monokai Pro Light.
    pub fn monokai_pro_light() -> Self {
        Self::monokai_palette(
            Self::rgb(225, 71, 117),
            Self::rgb(237, 231, 229),
            Self::rgb(250, 244, 242),
            Self::rgb(211, 205, 204),
            Self::rgb(224, 218, 217),
            Self::rgb(165, 159, 160),
            Self::rgb(145, 140, 142),
            Self::rgb(41, 36, 42),
            Self::rgb(112, 107, 110),
            Self::rgb(225, 71, 117),
            Self::rgb(38, 157, 105),
            Self::rgb(204, 122, 10),
            Self::rgb(225, 96, 50),
            Self::rgb(112, 88, 190),
            Self::rgb(28, 140, 168),
        )
    }

    /// Monokai Pro Light Sun.
    pub fn monokai_pro_light_sun() -> Self {
        Self::monokai_palette(
            Self::rgb(206, 71, 112),
            Self::rgb(238, 229, 222),
            Self::rgb(248, 239, 231),
            Self::rgb(210, 201, 196),
            Self::rgb(222, 213, 208),
            Self::rgb(165, 156, 156),
            Self::rgb(146, 137, 138),
            Self::rgb(44, 35, 46),
            Self::rgb(114, 105, 109),
            Self::rgb(206, 71, 112),
            Self::rgb(33, 136, 113),
            Self::rgb(177, 104, 3),
            Self::rgb(212, 87, 43),
            Self::rgb(104, 81, 162),
            Self::rgb(36, 115, 182),
        )
    }

    /// Monokai Pro Spectrum.
    pub fn monokai_pro_spectrum() -> Self {
        Self::monokai_palette(
            Self::rgb(252, 229, 102),
            Self::rgb(25, 25, 25),
            Self::rgb(34, 34, 34),
            Self::rgb(54, 53, 55),
            Self::rgb(19, 19, 19),
            Self::rgb(105, 103, 108),
            Self::rgb(139, 136, 143),
            Self::rgb(247, 241, 255),
            Self::rgb(186, 182, 192),
            Self::rgb(252, 97, 141),
            Self::rgb(123, 216, 143),
            Self::rgb(252, 229, 102),
            Self::rgb(253, 147, 83),
            Self::rgb(148, 138, 227),
            Self::rgb(90, 212, 230),
        )
    }

    /// Monokai Pro Ristretto.
    pub fn monokai_pro_ristretto() -> Self {
        Self::monokai_palette(
            Self::rgb(249, 204, 108),
            Self::rgb(33, 28, 28),
            Self::rgb(44, 37, 37),
            Self::rgb(64, 56, 56),
            Self::rgb(25, 21, 21),
            Self::rgb(114, 105, 106),
            Self::rgb(148, 138, 139),
            Self::rgb(255, 241, 243),
            Self::rgb(195, 183, 184),
            Self::rgb(253, 104, 131),
            Self::rgb(173, 218, 120),
            Self::rgb(249, 204, 108),
            Self::rgb(243, 141, 112),
            Self::rgb(168, 169, 235),
            Self::rgb(133, 218, 204),
        )
    }

    /// Monokai Pro Octagon.
    pub fn monokai_pro_octagon() -> Self {
        Self::monokai_palette(
            Self::rgb(255, 215, 109),
            Self::rgb(30, 31, 43),
            Self::rgb(40, 42, 58),
            Self::rgb(58, 61, 75),
            Self::rgb(22, 24, 33),
            Self::rgb(105, 109, 119),
            Self::rgb(136, 141, 148),
            Self::rgb(234, 242, 241),
            Self::rgb(178, 185, 189),
            Self::rgb(255, 101, 122),
            Self::rgb(186, 215, 97),
            Self::rgb(255, 215, 109),
            Self::rgb(255, 155, 94),
            Self::rgb(195, 154, 201),
            Self::rgb(156, 209, 187),
        )
    }

    /// Monokai Pro Machine.
    pub fn monokai_pro_machine() -> Self {
        Self::monokai_palette(
            Self::rgb(255, 237, 114),
            Self::rgb(29, 37, 40),
            Self::rgb(39, 49, 54),
            Self::rgb(58, 68, 73),
            Self::rgb(22, 27, 30),
            Self::rgb(107, 118, 120),
            Self::rgb(139, 151, 152),
            Self::rgb(242, 255, 252),
            Self::rgb(184, 196, 195),
            Self::rgb(255, 109, 126),
            Self::rgb(162, 229, 123),
            Self::rgb(255, 237, 114),
            Self::rgb(255, 178, 112),
            Self::rgb(186, 160, 248),
            Self::rgb(124, 213, 241),
        )
    }

    /// Monokai Classic.
    pub fn monokai_classic() -> Self {
        Self::monokai_palette(
            Self::rgb(230, 219, 116),
            Self::rgb(29, 30, 25),
            Self::rgb(39, 40, 34),
            Self::rgb(59, 60, 53),
            Self::rgb(22, 22, 19),
            Self::rgb(110, 112, 102),
            Self::rgb(145, 146, 136),
            Self::rgb(253, 255, 241),
            Self::rgb(192, 193, 181),
            Self::rgb(249, 38, 114),
            Self::rgb(166, 226, 46),
            Self::rgb(230, 219, 116),
            Self::rgb(253, 151, 31),
            Self::rgb(174, 129, 255),
            Self::rgb(102, 217, 239),
        )
    }

    /// Omarchy Ethereal.
    pub fn ethereal() -> Self {
        Self::omarchy_palette(
            Self::rgb(125, 130, 217),
            Self::rgb(255, 206, 173),
            Self::rgb(6, 11, 30),
            Self::rgb(60, 72, 109),
            Self::rgb(237, 91, 90),
            Self::rgb(146, 165, 147),
            Self::rgb(233, 187, 79),
            Self::rgb(125, 130, 217),
            Self::rgb(200, 157, 193),
            Self::rgb(163, 191, 209),
            Self::rgb(249, 153, 87),
            Self::rgb(109, 125, 182),
        )
    }

    /// Omarchy Everforest.
    pub fn everforest() -> Self {
        Self::omarchy_palette(
            Self::rgb(127, 187, 179),
            Self::rgb(211, 198, 170),
            Self::rgb(45, 53, 59),
            Self::rgb(71, 82, 88),
            Self::rgb(230, 126, 128),
            Self::rgb(167, 192, 128),
            Self::rgb(219, 188, 127),
            Self::rgb(127, 187, 179),
            Self::rgb(214, 153, 182),
            Self::rgb(131, 192, 146),
            Self::rgb(211, 198, 170),
            Self::rgb(71, 82, 88),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn flexoki_palette(
        accent: Color,
        panel_bg: Color,
        surface0: Color,
        surface1: Color,
        surface_dim: Color,
        overlay0: Color,
        overlay1: Color,
        text: Color,
        subtext0: Color,
        mauve: Color,
        green: Color,
        yellow: Color,
        red: Color,
        blue: Color,
        teal: Color,
        peach: Color,
    ) -> Self {
        Self {
            accent,
            panel_bg,
            surface0,
            surface1,
            surface_dim,
            overlay0,
            overlay1,
            text,
            subtext0,
            mauve,
            green,
            yellow,
            red,
            blue,
            teal,
            peach,
        }
    }

    /// Flexoki Light.
    pub fn flexoki_light() -> Self {
        Self::flexoki_palette(
            Self::rgb(36, 131, 123),
            Self::rgb(255, 252, 240),
            Self::rgb(230, 228, 217),
            Self::rgb(206, 205, 195),
            Self::rgb(242, 240, 229),
            Self::rgb(183, 181, 172),
            Self::rgb(111, 110, 105),
            Self::rgb(16, 15, 15),
            Self::rgb(111, 110, 105),
            Self::rgb(94, 64, 157),
            Self::rgb(102, 128, 11),
            Self::rgb(173, 131, 1),
            Self::rgb(175, 48, 41),
            Self::rgb(32, 94, 166),
            Self::rgb(36, 131, 123),
            Self::rgb(188, 82, 21),
        )
    }

    /// Flexoki.
    pub fn flexoki() -> Self {
        Self::flexoki_palette(
            Self::rgb(58, 169, 159),
            Self::rgb(16, 15, 15),
            Self::rgb(40, 39, 38),
            Self::rgb(64, 62, 60),
            Self::rgb(28, 27, 26),
            Self::rgb(87, 86, 83),
            Self::rgb(135, 133, 128),
            Self::rgb(206, 205, 195),
            Self::rgb(135, 133, 128),
            Self::rgb(139, 126, 200),
            Self::rgb(135, 154, 57),
            Self::rgb(208, 162, 21),
            Self::rgb(209, 77, 65),
            Self::rgb(67, 133, 190),
            Self::rgb(58, 169, 159),
            Self::rgb(218, 112, 44),
        )
    }

    /// Omarchy Hackerman.
    pub fn hackerman() -> Self {
        Self::omarchy_palette(
            Self::rgb(130, 251, 156),
            Self::rgb(221, 247, 255),
            Self::rgb(11, 12, 22),
            Self::rgb(62, 64, 88),
            Self::rgb(80, 248, 114),
            Self::rgb(79, 232, 143),
            Self::rgb(80, 247, 212),
            Self::rgb(130, 157, 212),
            Self::rgb(134, 167, 223),
            Self::rgb(124, 248, 247),
            Self::rgb(133, 225, 251),
            Self::rgb(106, 110, 149),
        )
    }

    /// Omarchy Last Horizon.
    pub fn last_horizon() -> Self {
        Self::omarchy_palette(
            Self::rgb(181, 151, 144),
            Self::rgb(250, 252, 251),
            Self::rgb(12, 11, 12),
            Self::rgb(12, 11, 12),
            Self::rgb(195, 139, 123),
            Self::rgb(135, 169, 176),
            Self::rgb(107, 94, 115),
            Self::rgb(181, 151, 144),
            Self::rgb(196, 216, 226),
            Self::rgb(165, 160, 182),
            Self::rgb(207, 211, 205),
            Self::rgb(88, 78, 81),
        )
    }

    /// Omarchy Lumon.
    pub fn lumon() -> Self {
        Self::omarchy_palette(
            Self::rgb(139, 201, 235),
            Self::rgb(214, 226, 238),
            Self::rgb(22, 36, 45),
            Self::rgb(27, 45, 64),
            Self::rgb(77, 134, 176),
            Self::rgb(94, 149, 188),
            Self::rgb(111, 164, 201),
            Self::rgb(111, 184, 227),
            Self::rgb(139, 201, 235),
            Self::rgb(180, 228, 246),
            Self::rgb(214, 226, 238),
            Self::rgb(48, 72, 96),
        )
    }

    /// Omarchy Matte Black.
    pub fn matte_black() -> Self {
        Self::omarchy_palette(
            Self::rgb(230, 142, 13),
            Self::rgb(190, 190, 190),
            Self::rgb(18, 18, 18),
            Self::rgb(51, 51, 51),
            Self::rgb(211, 95, 95),
            Self::rgb(255, 193, 7),
            Self::rgb(185, 28, 28),
            Self::rgb(230, 142, 13),
            Self::rgb(211, 95, 95),
            Self::rgb(190, 190, 190),
            Self::rgb(190, 190, 190),
            Self::rgb(138, 138, 141),
        )
    }

    /// Omarchy Miasma.
    pub fn miasma() -> Self {
        Self::omarchy_palette(
            Self::rgb(120, 130, 75),
            Self::rgb(194, 194, 176),
            Self::rgb(34, 34, 34),
            Self::rgb(0, 0, 0),
            Self::rgb(104, 87, 66),
            Self::rgb(95, 135, 95),
            Self::rgb(179, 109, 67),
            Self::rgb(120, 130, 75),
            Self::rgb(187, 119, 68),
            Self::rgb(201, 165, 84),
            Self::rgb(215, 196, 131),
            Self::rgb(102, 102, 102),
        )
    }

    /// Omarchy Osaka Jade.
    pub fn osaka_jade() -> Self {
        Self::omarchy_palette(
            Self::rgb(80, 148, 117),
            Self::rgb(193, 196, 151),
            Self::rgb(17, 28, 24),
            Self::rgb(35, 55, 43),
            Self::rgb(255, 83, 69),
            Self::rgb(84, 158, 106),
            Self::rgb(69, 148, 81),
            Self::rgb(80, 148, 117),
            Self::rgb(210, 104, 156),
            Self::rgb(45, 213, 183),
            Self::rgb(246, 245, 221),
            Self::rgb(83, 104, 91),
        )
    }

    /// Omarchy Retro 82.
    pub fn retro_82() -> Self {
        Self::omarchy_palette(
            Self::rgb(250, 169, 104),
            Self::rgb(246, 220, 172),
            Self::rgb(5, 24, 46),
            Self::rgb(48, 52, 66),
            Self::rgb(248, 85, 37),
            Self::rgb(2, 131, 145),
            Self::rgb(233, 123, 60),
            Self::rgb(250, 169, 104),
            Self::rgb(63, 143, 138),
            Self::rgb(140, 191, 184),
            Self::rgb(167, 201, 198),
            Self::rgb(19, 78, 90),
        )
    }

    /// Omarchy Solitude.
    pub fn solitude() -> Self {
        Self::omarchy_palette(
            Self::rgb(121, 129, 134),
            Self::rgb(202, 204, 204),
            Self::rgb(16, 19, 21),
            Self::rgb(16, 19, 21),
            Self::rgb(86, 93, 96),
            Self::rgb(159, 165, 169),
            Self::rgb(217, 219, 220),
            Self::rgb(121, 129, 134),
            Self::rgb(174, 174, 174),
            Self::rgb(112, 112, 112),
            Self::rgb(203, 194, 190),
            Self::rgb(75, 78, 85),
        )
    }

    /// Omarchy Vantablack.
    pub fn vantablack() -> Self {
        Self::omarchy_palette(
            Self::rgb(141, 141, 141),
            Self::rgb(255, 255, 255),
            Self::rgb(0, 0, 0),
            Self::rgb(64, 64, 64),
            Self::rgb(164, 164, 164),
            Self::rgb(182, 182, 182),
            Self::rgb(206, 206, 206),
            Self::rgb(141, 141, 141),
            Self::rgb(155, 155, 155),
            Self::rgb(176, 176, 176),
            Self::rgb(236, 236, 236),
            Self::rgb(92, 92, 92),
        )
    }

    /// Omarchy White.
    pub fn white() -> Self {
        Self::omarchy_palette(
            Self::rgb(110, 110, 110),
            Self::rgb(0, 0, 0),
            Self::rgb(255, 255, 255),
            Self::rgb(192, 192, 192),
            Self::rgb(42, 42, 42),
            Self::rgb(58, 58, 58),
            Self::rgb(74, 74, 74),
            Self::rgb(26, 26, 26),
            Self::rgb(46, 46, 46),
            Self::rgb(62, 62, 62),
            Self::rgb(0, 0, 0),
            Self::rgb(192, 192, 192),
        )
    }

    /// Gardn Day: paper surfaces, Sumi ink, GARDN Green chrome.
    pub fn gardn_day() -> Self {
        Self::catppuccin_palette(
            Self::rgb(11, 90, 60),
            Self::rgb(255, 255, 252),
            Self::rgb(247, 243, 234),
            Self::rgb(223, 217, 205),
            Self::rgb(214, 205, 190),
            Self::rgb(99, 100, 98),
            Self::rgb(80, 82, 80),
            Self::rgb(31, 31, 31),
            Self::rgb(99, 100, 98),
            Self::rgb(155, 79, 115),
            Self::rgb(63, 92, 54),
            Self::rgb(92, 112, 24),
            Self::rgb(176, 58, 32),
            Self::rgb(46, 106, 148),
            Self::rgb(61, 122, 92),
            Self::rgb(155, 79, 115),
        )
    }

    /// Gardn Night: deep green-black, warm paper text, lifted green chrome.
    pub fn gardn_night() -> Self {
        Self::catppuccin_palette(
            Self::rgb(125, 186, 114),
            Self::rgb(7, 26, 19),
            Self::rgb(13, 36, 26),
            Self::rgb(18, 49, 34),
            Self::rgb(36, 74, 55),
            Self::rgb(122, 138, 124),
            Self::rgb(174, 184, 172),
            Self::rgb(247, 243, 234),
            Self::rgb(174, 184, 172),
            Self::rgb(240, 184, 205),
            Self::rgb(125, 186, 114),
            Self::rgb(167, 201, 87),
            Self::rgb(255, 122, 89),
            Self::rgb(126, 182, 217),
            Self::rgb(95, 168, 152),
            Self::rgb(240, 184, 205),
        )
    }

    /// Resolve a theme by name. Returns None for unknown names.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().replace([' ', '_'], "-").as_str() {
            "catppuccin" | "catppuccin-mocha" | "mocha" => Some(Self::catppuccin()),
            "catppuccin-latte" | "latte" | "light" => Some(Self::catppuccin_latte()),
            "catppuccin-frappe" | "frappe" => Some(Self::catppuccin_frappe()),
            "catppuccin-macchiato" | "macchiato" => Some(Self::catppuccin_macchiato()),
            "terminal" => Some(Self::terminal()),
            "tokyo-night" | "tokyonight" => Some(Self::tokyo_night()),
            "tokyo-night-day" | "tokyo-day" | "tokyonight-day" => Some(Self::tokyo_night_day()),
            "dracula" => Some(Self::dracula()),
            "nord" => Some(Self::nord()),
            "gruvbox" | "gruvbox-dark" => Some(Self::gruvbox()),
            "gruvbox-light" => Some(Self::gruvbox_light()),
            "one-dark" | "onedark" => Some(Self::one_dark()),
            "one-light" | "onelight" => Some(Self::one_light()),
            "solarized" | "solarized-dark" => Some(Self::solarized()),
            "solarized-light" => Some(Self::solarized_light()),
            "kanagawa" => Some(Self::kanagawa()),
            "kanagawa-lotus" | "lotus" => Some(Self::kanagawa_lotus()),
            "rose-pine" | "rosepine" => Some(Self::rose_pine()),
            "rose-pine-dawn" | "rosepine-dawn" | "dawn" => Some(Self::rose_pine_dawn()),
            "vesper" => Some(Self::vesper()),
            "monokai-pro" | "monokai" => Some(Self::monokai_pro()),
            "monokai-pro-light" | "monokai-light" => Some(Self::monokai_pro_light()),
            "monokai-pro-light-sun" | "monokai-pro-sun" | "monokai-sun" | "sun" => {
                Some(Self::monokai_pro_light_sun())
            }
            "monokai-pro-spectrum" | "monokai-spectrum" | "spectrum" => {
                Some(Self::monokai_pro_spectrum())
            }
            "monokai-pro-ristretto" | "monokai-ristretto" | "ristretto" => {
                Some(Self::monokai_pro_ristretto())
            }
            "monokai-pro-octagon" | "monokai-octagon" | "octagon" => {
                Some(Self::monokai_pro_octagon())
            }
            "monokai-pro-machine" | "monokai-machine" | "machine" => {
                Some(Self::monokai_pro_machine())
            }
            "monokai-classic" | "classic" => Some(Self::monokai_classic()),
            "ethereal" => Some(Self::ethereal()),
            "everforest" => Some(Self::everforest()),
            "flexoki" => Some(Self::flexoki()),
            "flexoki-light" => Some(Self::flexoki_light()),
            "gardn-day" => Some(Self::gardn_day()),
            "gardn-night" => Some(Self::gardn_night()),
            "hackerman" => Some(Self::hackerman()),
            "last-horizon" => Some(Self::last_horizon()),
            "lumon" => Some(Self::lumon()),
            "matte-black" => Some(Self::matte_black()),
            "miasma" => Some(Self::miasma()),
            "osaka-jade" => Some(Self::osaka_jade()),
            "retro-82" => Some(Self::retro_82()),
            "solitude" => Some(Self::solitude()),
            "vantablack" => Some(Self::vantablack()),
            "white" => Some(Self::white()),
            _ => None,
        }
    }

    pub fn from_theme(
        name: &str,
        appearance: crate::terminal_theme::ThemeAppearance,
    ) -> Option<Self> {
        Self::from_theme_with_terminal(name, appearance, TerminalTheme::default())
    }

    pub fn from_theme_with_terminal_accent(
        name: &str,
        appearance: crate::terminal_theme::ThemeAppearance,
        host_theme: TerminalTheme,
        terminal_accent: TerminalAccent,
    ) -> Option<Self> {
        let theme_name = theme_name_for_appearance(name, appearance)?;
        if theme_name == "system" {
            return Some(Self::system(host_theme, appearance, terminal_accent));
        }
        Self::from_name(theme_name)
    }

    pub fn from_theme_with_terminal(
        name: &str,
        appearance: crate::terminal_theme::ThemeAppearance,
        host_theme: TerminalTheme,
    ) -> Option<Self> {
        Self::from_theme_with_terminal_accent(name, appearance, host_theme, TerminalAccent::Blue)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceCardArea {
    pub ws_idx: usize,
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceGroupHeaderArea {
    pub group_idx: usize,
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceGroupEmptyArea {
    pub group_idx: usize,
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceGroupDropArea {
    pub group_idx: usize,
    pub insert_idx: usize,
    pub rect: Rect,
}

/// Computed view geometry — derived from AppState + terminal size.
/// Updated before each render, consumed by render and mouse handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewLayout {
    Desktop,
    Mobile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum MobileSwitcherLevel {
    #[default]
    Groups,
    Workspaces {
        group_idx: usize,
    },
    Tabs {
        ws_idx: usize,
    },
    Panes {
        ws_idx: usize,
        tab_idx: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextBarTarget {
    Group,
    Workspace,
    Tab,
    Pane,
    /// Trailing per-client tab-control chip (watching / free); rect is the
    /// hit area for later click-to-claim wiring.
    TabControl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContextBarSegment {
    pub(crate) target: ContextBarTarget,
    pub(crate) label: String,
    pub(crate) rect: Rect,
    pub(crate) hit_rect: Option<Rect>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ContextBarView {
    pub(crate) rect: Rect,
    pub(crate) counts: String,
    pub(crate) counts_rect: Rect,
    pub(crate) segments: Vec<ContextBarSegment>,
}

impl ContextBarView {
    pub(crate) fn target_at(&self, col: u16, row: u16) -> Option<ContextBarTarget> {
        self.segments.iter().find_map(|segment| {
            let rect = segment.hit_rect.unwrap_or(segment.rect);
            (col >= rect.x
                && col < rect.x.saturating_add(rect.width)
                && row >= rect.y
                && row < rect.y.saturating_add(rect.height))
            .then_some(segment.target)
        })
    }
}

#[derive(Clone)]
pub struct ViewState {
    pub layout: ViewLayout,
    pub sidebar_rect: Rect,
    pub right_sidebar_rect: Rect,
    pub workspace_card_areas: Vec<WorkspaceCardArea>,
    pub workspace_group_header_areas: Vec<WorkspaceGroupHeaderArea>,
    pub workspace_group_empty_areas: Vec<WorkspaceGroupEmptyArea>,
    pub tab_bar_rect: Rect,
    pub tab_hit_areas: Vec<Rect>,
    pub tab_close_hit_areas: Vec<Rect>,
    pub tab_scroll_left_hit_area: Rect,
    pub tab_scroll_right_hit_area: Rect,
    pub new_tab_hit_area: Rect,
    pub context_bar: ContextBarView,
    pub terminal_area: Rect,
    pub mobile_header_rect: Rect,
    pub toast_hit_area: Rect,
    pub pane_infos: Vec<PaneInfo>,
    pub split_borders: Vec<SplitBorder>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Onboarding,
    ReleaseNotes,
    ProductAnnouncement,
    Navigate,
    Prefix,
    Copy,
    Terminal,
    RenameWorkspace,
    RenameGroup,
    RenameTab,
    RenamePane,
    Resize,
    ConfirmClose,
    ConfirmDeleteGroup,
    ContextMenu,
    Settings,
    GlobalMenu,
    GroupMenu,
    AgentMenu,
    KeybindHelp,
    Navigator,
    CommandPalette,
    AgentProfilePicker,
    GitRepoPicker,
    Github,
    ConfigDiagnostics,
}

impl Mode {
    /// Whether this mode is part of the prefix command/navigation realm.
    /// Text-entry modes deliberately remain outside this allowlist so adding a
    /// new text field cannot silently force the host input source to ASCII.
    pub(crate) fn mouse_motion_changes_view(self) -> bool {
        matches!(
            self,
            Self::GlobalMenu
                | Self::ContextMenu
                | Self::Navigator
                | Self::CommandPalette
                | Self::AgentProfilePicker
                | Self::GitRepoPicker
                | Self::Github
                | Self::GroupMenu
                | Self::AgentMenu
        )
    }

    pub(crate) fn wants_ascii_input(self) -> bool {
        matches!(
            self,
            Mode::Navigate
                | Mode::Prefix
                | Mode::Copy
                | Mode::Resize
                | Mode::ConfirmClose
                | Mode::ConfirmDeleteGroup
                | Mode::ContextMenu
                | Mode::GlobalMenu
                | Mode::GroupMenu
                | Mode::AgentMenu
                | Mode::KeybindHelp
                | Mode::Navigator
                | Mode::ConfigDiagnostics
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NavigatorTarget {
    Group {
        group_idx: usize,
    },
    Workspace {
        ws_idx: usize,
    },
    Tab {
        ws_idx: usize,
        tab_idx: usize,
    },
    Pane {
        ws_idx: usize,
        tab_idx: usize,
        pane_id: PaneId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NavigatorRow {
    pub target: NavigatorTarget,
    pub depth: u8,
    pub label: String,
    pub meta: String,
    pub status: AgentState,
    pub seen: bool,
    pub is_current: bool,
    pub is_group: bool,
    pub is_workspace: bool,
    pub is_tab: bool,
    pub has_children: bool,
    pub expanded: bool,
    pub search_text: String,
    /// Whether this row directly matches the active query or state filter.
    /// Ancestor and cascaded subtree rows remain visible but are dimmed.
    pub matched: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavigatorStateFilter {
    Blocked,
    Working,
    Idle,
    Done,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct NavigatorState {
    pub query: String,
    pub list: ModalListState,
    pub scroll: usize,
    pub search_focused: bool,
    pub state_filter: Option<NavigatorStateFilter>,
    pub expanded_groups: std::collections::HashSet<String>,
    pub expanded_workspaces: std::collections::HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyModeSearchDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CopyModeSearchPrompt {
    pub direction: CopyModeSearchDirection,
    pub query: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CopyModeSearchState {
    pub prompt: Option<CopyModeSearchPrompt>,
    pub query: String,
    pub direction: Option<CopyModeSearchDirection>,
    pub matches: Vec<crate::pane::TerminalTextMatch>,
    pub current: Option<usize>,
    pub geometry: Option<(u16, u16)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CopyModeState {
    pub pane_id: PaneId,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub entry_offset_from_bottom: usize,
    pub entry_max_offset_from_bottom: usize,
    pub selection: Option<CopyModeSelection>,
    pub search: CopyModeSearchState,
}

impl CopyModeState {
    pub(crate) fn new(
        pane_id: PaneId,
        cursor_row: u16,
        cursor_col: u16,
        entry_metrics: Option<crate::pane::ScrollMetrics>,
    ) -> Self {
        let (entry_offset_from_bottom, entry_max_offset_from_bottom) = entry_metrics
            .map(|metrics| (metrics.offset_from_bottom, metrics.max_offset_from_bottom))
            .unwrap_or((0, 0));

        Self {
            pane_id,
            cursor_row,
            cursor_col,
            entry_offset_from_bottom,
            entry_max_offset_from_bottom,
            selection: None,
            search: CopyModeSearchState::default(),
        }
    }

    pub(crate) fn restored_offset_from_bottom(
        &self,
        current_metrics: Option<crate::pane::ScrollMetrics>,
    ) -> usize {
        if self.entry_offset_from_bottom == 0 {
            return 0;
        }

        let Some(metrics) = current_metrics else {
            return self.entry_offset_from_bottom;
        };
        let scrollback_growth = metrics
            .max_offset_from_bottom
            .saturating_sub(self.entry_max_offset_from_bottom);
        self.entry_offset_from_bottom
            .saturating_add(scrollback_growth)
            .min(metrics.max_offset_from_bottom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyModeSelection {
    Character,
    Linewise { anchor_row: u32 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AgentPanelScope {
    #[default]
    CurrentWorkspace,
    CurrentGroup,
    AllWorkspaces,
}

// ---------------------------------------------------------------------------
// Settings UI state
// ---------------------------------------------------------------------------

/// Which settings section is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Theme,
    #[allow(dead_code)] // Legacy standalone tab; settings now groups layout under appearance.
    Layout,
    Sound,
    #[allow(dead_code)] // Legacy standalone tab; settings now groups toasts under notifications.
    Toast,
    PaneLabels,
    Commands,
    Experiments,
    Agents,
    Integrations,
    Connections,
    GroupGeneral,
    GroupDefaults,
    GroupProfiles,
    GroupGithub,
    WorkspaceGeneral,
    WorkspaceGithub,
    About,
}

impl SettingsSection {
    pub const ALL: &[Self] = &[
        Self::Theme,
        Self::Sound,
        Self::PaneLabels,
        Self::Commands,
        Self::Agents,
        Self::Integrations,
        Self::Connections,
        Self::Experiments,
        Self::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Theme => "Appearance",
            Self::Agents => "Agents",
            Self::Layout => "Layout",
            Self::Sound => "Notifications",
            Self::Toast => "Toasts",
            Self::PaneLabels => "Behavior",
            Self::Commands => "Commands",
            Self::Experiments => "Advanced",
            Self::Integrations => "Integrations",
            Self::Connections => "Connections",
            Self::GroupGeneral => "General",
            Self::GroupDefaults => "Space Defaults",
            Self::GroupProfiles => "Agents",
            Self::GroupGithub => "GitHub",
            Self::WorkspaceGeneral => "General",
            Self::WorkspaceGithub => "GitHub",
            Self::About => "About",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SettingsSidebarSelection {
    pub(crate) section: SettingsSection,
    pub(crate) subsection: Option<usize>,
}

impl SettingsSidebarSelection {
    pub(crate) const fn section(section: SettingsSection) -> Self {
        Self {
            section,
            subsection: None,
        }
    }
}

pub const DEFAULT_DARK_THEME_NAME: &str = "catppuccin";
pub const DEFAULT_LIGHT_THEME_NAME: &str = "catppuccin-latte";

/// Legacy theme-family display order used where a single theme override is stored.
pub const THEME_NAMES: &[&str] = &[
    "system",
    DEFAULT_DARK_THEME_NAME,
    "catppuccin-frappe",
    "catppuccin-macchiato",
    "dracula",
    "ethereal",
    "everforest",
    "flexoki",
    "gardn-night",
    "gruvbox",
    "hackerman",
    "kanagawa",
    "last-horizon",
    "lumon",
    "matte-black",
    "miasma",
    "monokai-classic",
    "monokai-pro",
    "monokai-pro-machine",
    "monokai-pro-octagon",
    "monokai-pro-ristretto",
    "monokai-pro-spectrum",
    "nord",
    "one-dark",
    "osaka-jade",
    "retro-82",
    "rose-pine",
    "solarized",
    "solitude",
    "terminal",
    "tokyo-night",
    "vantablack",
    "vesper",
];

/// Built-in concrete themes that can render a light appearance.
pub const LIGHT_THEME_NAMES: &[&str] = &[
    DEFAULT_LIGHT_THEME_NAME,
    "flexoki-light",
    "gardn-day",
    "gruvbox-light",
    "kanagawa-lotus",
    "monokai-pro-light",
    "monokai-pro-light-sun",
    "one-light",
    "rose-pine-dawn",
    "solarized-light",
    "tokyo-night-day",
    "white",
];

/// Built-in concrete themes that can render a dark appearance.
pub const DARK_THEME_NAMES: &[&str] = &[
    DEFAULT_DARK_THEME_NAME,
    "catppuccin-frappe",
    "catppuccin-macchiato",
    "dracula",
    "ethereal",
    "everforest",
    "flexoki",
    "gardn-night",
    "gruvbox",
    "hackerman",
    "kanagawa",
    "last-horizon",
    "lumon",
    "matte-black",
    "miasma",
    "monokai-classic",
    "monokai-pro",
    "monokai-pro-machine",
    "monokai-pro-octagon",
    "monokai-pro-ristretto",
    "monokai-pro-spectrum",
    "nord",
    "one-dark",
    "osaka-jade",
    "retro-82",
    "rose-pine",
    "solarized",
    "solitude",
    "tokyo-night",
    "vantablack",
    "vesper",
];

pub fn normalize_theme_name(name: &str) -> String {
    name.to_lowercase().replace([' ', '_'], "-")
}

pub fn theme_names_for_appearance(appearance: ThemeAppearance) -> &'static [&'static str] {
    match appearance {
        ThemeAppearance::Light => LIGHT_THEME_NAMES,
        ThemeAppearance::Dark => DARK_THEME_NAMES,
    }
}

pub fn default_theme_name_for_appearance(appearance: ThemeAppearance) -> &'static str {
    match appearance {
        ThemeAppearance::Light => DEFAULT_LIGHT_THEME_NAME,
        ThemeAppearance::Dark => DEFAULT_DARK_THEME_NAME,
    }
}

pub fn theme_name_for_appearance(name: &str, appearance: ThemeAppearance) -> Option<&'static str> {
    let normalized = normalize_theme_name(name);
    match appearance {
        ThemeAppearance::Light => match normalized.as_str() {
            "system" => Some("system"),
            "terminal" => Some("terminal"),
            "catppuccin" | "catppuccin-mocha" | "catppuccin-latte" | "latte" | "light"
            | "mocha" => Some("catppuccin-latte"),
            "tokyo-night" | "tokyonight" | "tokyo-night-day" | "tokyo-day" | "tokyonight-day" => {
                Some("tokyo-night-day")
            }
            "gruvbox" | "gruvbox-dark" | "gruvbox-light" => Some("gruvbox-light"),
            "one-dark" | "onedark" | "one-light" | "onelight" => Some("one-light"),
            "solarized" | "solarized-dark" | "solarized-light" => Some("solarized-light"),
            "kanagawa" | "kanagawa-lotus" | "lotus" => Some("kanagawa-lotus"),
            "rose-pine" | "rosepine" | "rose-pine-dawn" | "rosepine-dawn" | "dawn" => {
                Some("rose-pine-dawn")
            }
            "monokai-pro" | "monokai" | "monokai-pro-light" | "monokai-light" => {
                Some("monokai-pro-light")
            }
            "monokai-pro-light-sun" | "monokai-pro-sun" | "monokai-sun" | "sun" => {
                Some("monokai-pro-light-sun")
            }
            "flexoki" | "flexoki-light" => Some("flexoki-light"),
            "gardn-day" | "gardn-night" => Some("gardn-day"),
            "white" => Some("white"),
            "dracula"
            | "nord"
            | "vesper"
            | "catppuccin-frappe"
            | "frappe"
            | "catppuccin-macchiato"
            | "macchiato"
            | "monokai-pro-spectrum"
            | "monokai-spectrum"
            | "spectrum"
            | "monokai-pro-ristretto"
            | "monokai-ristretto"
            | "ristretto"
            | "monokai-pro-octagon"
            | "monokai-octagon"
            | "octagon"
            | "monokai-pro-machine"
            | "monokai-machine"
            | "machine"
            | "monokai-classic"
            | "classic"
            | "ethereal"
            | "everforest"
            | "hackerman"
            | "last-horizon"
            | "lumon"
            | "matte-black"
            | "miasma"
            | "osaka-jade"
            | "retro-82"
            | "solitude"
            | "vantablack" => None,
            _ => None,
        },
        ThemeAppearance::Dark => match normalized.as_str() {
            "system" => Some("system"),
            "terminal" => Some("terminal"),
            "catppuccin" | "catppuccin-mocha" | "mocha" | "catppuccin-latte" | "latte"
            | "light" => Some("catppuccin"),
            "catppuccin-frappe" | "frappe" => Some("catppuccin-frappe"),
            "catppuccin-macchiato" | "macchiato" => Some("catppuccin-macchiato"),
            "tokyo-night" | "tokyonight" | "tokyo-night-day" | "tokyo-day" | "tokyonight-day" => {
                Some("tokyo-night")
            }
            "dracula" => Some("dracula"),
            "nord" => Some("nord"),
            "gruvbox" | "gruvbox-dark" | "gruvbox-light" => Some("gruvbox"),
            "one-dark" | "onedark" | "one-light" | "onelight" => Some("one-dark"),
            "solarized" | "solarized-dark" | "solarized-light" => Some("solarized"),
            "kanagawa" | "kanagawa-lotus" | "lotus" => Some("kanagawa"),
            "rose-pine" | "rosepine" | "rose-pine-dawn" | "rosepine-dawn" | "dawn" => {
                Some("rose-pine")
            }
            "vesper" => Some("vesper"),
            "monokai-pro" | "monokai" | "monokai-pro-light" | "monokai-light" => {
                Some("monokai-pro")
            }
            "monokai-pro-spectrum" | "monokai-spectrum" | "spectrum" => {
                Some("monokai-pro-spectrum")
            }
            "monokai-pro-ristretto" | "monokai-ristretto" | "ristretto" => {
                Some("monokai-pro-ristretto")
            }
            "monokai-pro-octagon" | "monokai-octagon" | "octagon" => Some("monokai-pro-octagon"),
            "monokai-pro-machine" | "monokai-machine" | "machine" => Some("monokai-pro-machine"),
            "monokai-classic" | "classic" => Some("monokai-classic"),
            "ethereal" => Some("ethereal"),
            "everforest" => Some("everforest"),
            "flexoki" | "flexoki-light" => Some("flexoki"),
            "gardn-day" | "gardn-night" => Some("gardn-night"),
            "hackerman" => Some("hackerman"),
            "last-horizon" => Some("last-horizon"),
            "lumon" => Some("lumon"),
            "matte-black" => Some("matte-black"),
            "miasma" => Some("miasma"),
            "osaka-jade" => Some("osaka-jade"),
            "retro-82" => Some("retro-82"),
            "solitude" => Some("solitude"),
            "vantablack" => Some("vantablack"),
            _ => None,
        },
    }
}

pub fn theme_config_names(config: &ThemeConfig) -> (String, String) {
    let light = config
        .light
        .as_deref()
        .or(config.name.as_deref())
        .and_then(|name| theme_name_for_appearance(name, ThemeAppearance::Light))
        .unwrap_or(DEFAULT_LIGHT_THEME_NAME)
        .to_string();
    let dark = config
        .dark
        .as_deref()
        .or(config.name.as_deref())
        .and_then(|name| theme_name_for_appearance(name, ThemeAppearance::Dark))
        .unwrap_or(DEFAULT_DARK_THEME_NAME)
        .to_string();
    (light, dark)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalListState {
    /// Persistent keyboard navigation anchor.
    pub selected: usize,
    hovered: Option<usize>,
    pointer_active: bool,
    engaged: bool,
}

impl ModalListState {
    #[cfg(test)]
    /// Creates a list with its keyboard selection visible.
    pub fn new(selected: usize) -> Self {
        Self {
            selected,
            hovered: None,
            pointer_active: false,
            engaged: true,
        }
    }

    /// Creates a list with no visible selection until it is interacted with.
    pub fn hidden(selected: usize) -> Self {
        Self {
            selected,
            hovered: None,
            pointer_active: true,
            engaged: false,
        }
    }

    /// Returns the row that should be visibly highlighted.
    pub fn visible(&self) -> Option<usize> {
        if self.pointer_active {
            self.hovered
        } else {
            Some(self.selected)
        }
    }

    /// Selects a row through keyboard or click interaction.
    pub fn select(&mut self, index: usize) {
        self.selected = index;
        self.hovered = None;
        self.pointer_active = false;
        self.engaged = true;
    }

    /// Restores the persistent cursor after pointer interaction.
    pub fn show(&mut self) {
        self.select(self.selected);
    }

    /// Restores the cursor only after this list has been interacted with.
    pub fn restore(&mut self) -> bool {
        if !self.engaged {
            return false;
        }
        self.show();
        true
    }

    /// Updates transient pointer highlighting without losing its navigation anchor.
    pub fn hover(&mut self, index: Option<usize>) {
        self.hovered = index;
        self.pointer_active = true;
    }

    /// Hides the current selection while retaining its navigation anchor.
    pub fn hide(&mut self) {
        self.hover(None);
    }

    pub fn is_engaged(&self) -> bool {
        self.engaged
    }

    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        self.visible().is_some()
    }

    pub fn move_prev(&mut self) {
        self.select(self.selected.saturating_sub(1));
    }

    pub fn move_next(&mut self, item_count: usize) {
        if item_count > 0 {
            self.select((self.selected + 1).min(item_count - 1));
        }
    }
}

impl Default for ModalListState {
    fn default() -> Self {
        Self::hidden(0)
    }
}

#[derive(Clone)]
pub struct SettingsState {
    /// Which settings section is active.
    pub section: SettingsSection,
    /// Expanded section in the general settings sidebar.
    pub(crate) sidebar_expanded: Option<SettingsSection>,
    /// Keyboard selection in the general settings sidebar.
    pub(crate) sidebar_selection: SettingsSidebarSelection,
    /// Whether keyboard input targets the general settings sidebar.
    pub(crate) sidebar_focused: bool,
    /// Selected item index within the current section.
    pub list: ModalListState,
    /// Text input row that retains focus independently from pointer hover.
    pub focused_input: Option<usize>,
    /// First visible row for scrollable settings sections.
    pub scroll: usize,
    /// The palette before opening settings (for cancel/restore).
    pub original_palette: Option<Palette>,
    /// The theme name before opening settings.
    pub original_theme: Option<String>,
    /// Pending global theme family while settings is open.
    pub pending_theme_name: Option<String>,
    /// Pending global theme mode while settings is open.
    pub pending_theme_mode: Option<ThemeMode>,
    /// Pending light theme while settings is open.
    pub pending_light_theme_name: Option<String>,
    /// Pending dark theme while settings is open.
    pub pending_dark_theme_name: Option<String>,
    /// Pending terminal light accent while settings is open.
    pub pending_terminal_light_accent: Option<TerminalAccent>,
    /// Pending terminal dark accent while settings is open.
    pub pending_terminal_dark_accent: Option<TerminalAccent>,
    /// Pending sound setting while settings is open.
    pub pending_sound_enabled: Option<bool>,
    /// Pending toast delivery while settings is open.
    pub pending_toast_delivery: Option<ToastDelivery>,
    pub pending_default_shell: Option<String>,
    pub pending_shell_mode: Option<crate::config::ShellModeConfig>,
    pub pending_version_check: Option<bool>,
    pub pending_manifest_check: Option<bool>,
    pub pending_toast_delay: Option<String>,
    pub pending_toast_gardn_position: Option<crate::config::ToastGardnPosition>,
    pub pending_clipboard_toast_enabled: Option<bool>,
    pub pending_clipboard_toast_position: Option<crate::config::ToastClipboardPosition>,
    /// Pending workspace close confirmation setting while settings is open.
    pub pending_confirm_close: Option<bool>,
    /// Pending new-tab naming prompt setting while settings is open.
    pub pending_prompt_new_tab_name: Option<bool>,
    /// Pending counter visibility setting while settings is open.
    pub pending_show_counters: Option<bool>,
    /// Pending pane border visibility while settings is open.
    pub pending_pane_borders: Option<bool>,
    /// Pending pane scrollbar visibility while settings is open.
    pub pending_pane_scrollbars: Option<bool>,
    /// Pending pane gap visibility while settings is open.
    pub pending_pane_gaps: Option<bool>,
    /// Pending single-tab bar hiding while settings is open.
    pub pending_hide_tab_bar_when_single_tab: Option<bool>,
    /// Pending copy-on-select while settings is open.
    pub pending_copy_on_select: Option<bool>,
    /// Pending new-workspace naming prompt while settings is open.
    pub pending_prompt_new_workspace_name: Option<bool>,
    /// Pending right-click passthrough modifier while settings is open.
    pub pending_right_click_passthrough_modifier: Option<RightClickPassthroughModifierConfig>,
    /// Pending new-terminal cwd policy while settings is open.
    pub pending_new_terminal_cwd: Option<NewTerminalCwdConfig>,
    /// Pending mouse wheel scroll amount while settings is open.
    pub pending_mouse_scroll_lines: Option<usize>,
    /// Pending agent-session restore setting while settings is open.
    pub pending_resume_agents_on_restore: Option<bool>,
    /// Pending outer terminal window-title template while settings is open.
    pub pending_window_title: Option<String>,
    /// Pending headless terminal dimensions while settings is open.
    pub pending_headless_cols: Option<String>,
    pub pending_headless_rows: Option<String>,
    /// Pending commands while the Commands settings tab is open.
    pub pending_browser_command: Option<String>,
    pub pending_review_command: Option<String>,
    pub pending_editor_command: Option<String>,
    /// Pending default sidebar width while settings is open.
    pub pending_sidebar_width: Option<u16>,
    /// Pending minimum expanded sidebar width while settings is open.
    pub pending_sidebar_min_width: Option<u16>,
    /// Pending maximum expanded sidebar width while settings is open.
    pub pending_sidebar_max_width: Option<u16>,
    pub pending_sidebar_arrangement: Option<crate::config::SidebarArrangementConfig>,
    pub pending_context_bar_visibility: Option<crate::config::ContextBarVisibilityConfig>,
    /// Pending default expansion state for newly attached clients.
    pub pending_sidebar_initial_state: Option<crate::config::SidebarInitialStateConfig>,
    /// Pending default agent scope for newly attached clients.
    pub pending_sidebar_initial_agent_scope: Option<crate::config::AgentPanelScopeConfig>,
    /// Pending pane-border agent metadata level while settings is open.
    pub pending_pane_border_agent_info: Option<PaneBorderAgentInfoConfig>,
    /// Pending status indicator style while settings is open.
    pub pending_status_indicators: Option<StatusIndicatorStyle>,
    /// Pending macOS prefix input source switching setting while settings is open.
    pub pending_switch_ascii_input_source_in_prefix: Option<bool>,
    /// Checked group accent while group settings is open; hover cursor is separate.
    pub pending_group_accent_choice: Option<Option<TerminalAccent>>,
    /// Pending workspace name while workspace settings is open.
    pub pending_workspace_name: Option<String>,
    /// Pending workspace default directory while workspace settings is open.
    pub pending_workspace_default_cwd: Option<String>,
    /// Pending workspace default execution host while workspace settings is open.
    pub pending_workspace_default_execution_host_id: Option<crate::execution_host::ExecutionHostId>,
    /// Pending workspace GitHub repository scope while workspace settings is open.
    pub pending_workspace_github_scope: Option<GithubRepositoryScope>,
    /// Pending comma-separated selected GitHub repositories.
    pub pending_workspace_github_repositories: Option<String>,
    /// Pending group name while group settings is open.
    pub pending_group_name: Option<String>,
    /// Pending group icon while group settings is open.
    pub pending_group_icon: Option<String>,
    /// Pending GitHub organization while group settings is open.
    pub pending_group_github_organization: Option<String>,

    /// Pending default directory for future spaces while group settings is open.
    pub pending_group_default_directory: Option<String>,
    /// Pending group default execution host while group settings is open.
    pub pending_group_default_execution_host_id: Option<crate::execution_host::ExecutionHostId>,
    /// Custom agent profile id loaded into the editor.
    pub pending_agent_profile_id: Option<String>,
    /// Pending custom agent profile name while settings is open.
    pub pending_agent_profile_name: Option<String>,
    /// Pending custom agent profile kind while settings is open.
    pub pending_agent_profile_kind: Option<crate::agent_profiles::AgentKind>,
    /// Pending custom agent profile command while settings is open.
    pub pending_agent_profile_command: Option<String>,
    pub pending_agent_profile_enabled: Option<bool>,
    /// Active agent family filter in the global agents settings tab.
    pub agent_profile_kind_filter: Option<crate::agent_profiles::AgentKind>,
    /// SSH profile selected for integration inspection and actions; `None` is Local.
    pub integration_host_profile_id: Option<String>,
    /// Connection profile editor draft and related install/forget substate.
    pub connection_editor: Option<ConnectionEditorState>,
    /// Group whose settings are being edited, if settings was opened from a group menu.
    pub group_settings_target: Option<usize>,
    /// True while the group icon picker grid is open in group settings.
    pub group_icon_picker_open: bool,
    /// Workspace whose settings are being edited, if settings was opened from a workspace menu.
    pub workspace_settings_target: Option<usize>,
}

#[derive(Clone)]
pub(crate) enum DragTarget {
    WorkspaceReorder {
        source_ws_idx: usize,
        insert_idx: Option<usize>,
        target_group_idx: Option<usize>,
        indicator_row: Option<u16>,
    },
    GroupReorder {
        source_group_idx: usize,
        insert_idx: Option<usize>,
        indicator_row: Option<u16>,
    },
    TabReorder {
        ws_idx: usize,
        source_tab_idx: usize,
        insert_idx: Option<usize>,
    },
    AgentFollowUp {
        workspace_id: String,
        pane_number: usize,
        drop_indicator_row: Option<u16>,
    },
    WorkspaceListScrollbar {
        grab_row_offset: u16,
    },
    AgentPanelScrollbar {
        grab_row_offset: u16,
    },
    PaneSplit {
        path: Vec<bool>,
        direction: Direction,
        area: Rect,
    },
    PaneScrollbar {
        pane_id: crate::layout::PaneId,
        grab_row_offset: u16,
    },
    ReleaseNotesScrollbar {
        grab_row_offset: u16,
    },
    ProductAnnouncementScrollbar {
        grab_row_offset: u16,
    },
    KeybindHelpScrollbar {
        grab_row_offset: u16,
    },
    SettingsThemeScrollbar {
        grab_row_offset: u16,
    },
    CommandPaletteScrollbar {
        grab_row_offset: u16,
    },
    AgentProfilePickerScrollbar {
        grab_row_offset: u16,
    },
    SidebarDivider,
    RightSidebarDivider,
    SidebarSectionDivider,
}

/// Active mouse drag on a split border or sidebar divider.
#[derive(Clone)]
pub(crate) struct DragState {
    pub target: DragTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CollapsedSidebarHover {
    Group(usize),
    Workspace(usize),
    Agent {
        ws_idx: usize,
        pane_id: crate::layout::PaneId,
    },
    AgentStatus {
        section: String,
    },
}

#[derive(Clone)]
pub(crate) struct WorkspacePressState {
    pub ws_idx: usize,
    pub start_col: u16,
    pub start_row: u16,
}

#[derive(Clone)]
pub(crate) struct GroupPressState {
    pub group_idx: usize,
    pub start_col: u16,
    pub start_row: u16,
}

#[derive(Clone)]
pub(crate) struct TabPressState {
    pub ws_idx: usize,
    pub tab_idx: usize,
    pub start_col: u16,
    pub start_row: u16,
}

#[derive(Clone)]
pub(crate) struct AgentPressState {
    pub workspace_id: String,
    pub pane_number: usize,
    pub start_col: u16,
    pub start_row: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentFollowUpEntry {
    pub workspace_id: String,
    pub pane_number: usize,
    pub added_at_unix_secs: u64,
}

impl AgentFollowUpEntry {
    pub(crate) fn matches(&self, workspace_id: &str, pane_number: usize) -> bool {
        self.workspace_id == workspace_id && self.pane_number == pane_number
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuKind {
    Sidebar {
        group_idx: usize,
    },
    Group {
        group_idx: usize,
        can_delete: bool,
    },
    Workspace {
        ws_idx: usize,
        project_commands: ProjectCommandAvailability,
    },
    Tab {
        ws_idx: usize,
        tab_idx: usize,
    },
    Agent {
        ws_idx: usize,
        pane_id: PaneId,
        in_follow_up: bool,
    },
    NewTabButton {
        ws_idx: usize,
        project_commands: ProjectCommandAvailability,
    },
    Pane {
        ws_idx: usize,
        pane_id: PaneId,
        has_manual_label: bool,
        right_click_passthrough: bool,
    },
}

/// Right-click context menu state.
#[derive(Clone)]
pub struct ContextMenuState {
    pub kind: ContextMenuKind,
    pub x: u16,
    pub y: u16,
    pub list: ModalListState,
}

pub(crate) const ADD_TO_FOLLOW_UP_CONTEXT_ITEM: &str = "add to follow up";
pub(crate) const REMOVE_FROM_FOLLOW_UP_CONTEXT_ITEM: &str = "remove from follow up";

const ADD_TO_FOLLOW_UP_CONTEXT_ITEMS: &[&str] = &[ADD_TO_FOLLOW_UP_CONTEXT_ITEM];
const REMOVE_FROM_FOLLOW_UP_CONTEXT_ITEMS: &[&str] = &[REMOVE_FROM_FOLLOW_UP_CONTEXT_ITEM];

const WORKSPACE_CONTEXT_MENU_ITEMS: [&[&str]; 16] = [
    &[
        "new", "tab", "agent", "---", "manage", "rename", "settings", "---", "danger", "close",
    ],
    &[
        "new", "tab", "agent", "review", "---", "manage", "rename", "settings", "---", "danger",
        "close",
    ],
    &[
        "new", "tab", "agent", "browser", "---", "manage", "rename", "settings", "---", "danger",
        "close",
    ],
    &[
        "new", "tab", "agent", "browser", "review", "---", "manage", "rename", "settings", "---",
        "danger", "close",
    ],
    &[
        "new", "tab", "agent", "editor", "---", "manage", "rename", "settings", "---", "danger",
        "close",
    ],
    &[
        "new", "tab", "agent", "editor", "review", "---", "manage", "rename", "settings", "---",
        "danger", "close",
    ],
    &[
        "new", "tab", "agent", "editor", "browser", "---", "manage", "rename", "settings", "---",
        "danger", "close",
    ],
    &[
        "new", "tab", "agent", "editor", "browser", "review", "---", "manage", "rename",
        "settings", "---", "danger", "close",
    ],
    &[
        "new", "tab", "agent", "github", "---", "manage", "rename", "settings", "---", "danger",
        "close",
    ],
    &[
        "new", "tab", "agent", "review", "github", "---", "manage", "rename", "settings", "---",
        "danger", "close",
    ],
    &[
        "new", "tab", "agent", "browser", "github", "---", "manage", "rename", "settings", "---",
        "danger", "close",
    ],
    &[
        "new", "tab", "agent", "browser", "review", "github", "---", "manage", "rename",
        "settings", "---", "danger", "close",
    ],
    &[
        "new", "tab", "agent", "editor", "github", "---", "manage", "rename", "settings", "---",
        "danger", "close",
    ],
    &[
        "new", "tab", "agent", "editor", "review", "github", "---", "manage", "rename", "settings",
        "---", "danger", "close",
    ],
    &[
        "new", "tab", "agent", "editor", "browser", "github", "---", "manage", "rename",
        "settings", "---", "danger", "close",
    ],
    &[
        "new", "tab", "agent", "editor", "browser", "review", "github", "---", "manage", "rename",
        "settings", "---", "danger", "close",
    ],
];

const NEW_TAB_CONTEXT_MENU_ITEMS: [&[&str]; 16] = [
    &["new", "tab", "agent"],
    &["new", "tab", "agent", "review"],
    &["new", "tab", "agent", "browser"],
    &["new", "tab", "agent", "browser", "review"],
    &["new", "tab", "agent", "editor"],
    &["new", "tab", "agent", "editor", "review"],
    &["new", "tab", "agent", "editor", "browser"],
    &["new", "tab", "agent", "editor", "browser", "review"],
    &["new", "tab", "agent", "github"],
    &["new", "tab", "agent", "review", "github"],
    &["new", "tab", "agent", "browser", "github"],
    &["new", "tab", "agent", "browser", "review", "github"],
    &["new", "tab", "agent", "editor", "github"],
    &["new", "tab", "agent", "editor", "review", "github"],
    &["new", "tab", "agent", "editor", "browser", "github"],
    &[
        "new", "tab", "agent", "editor", "browser", "review", "github",
    ],
];

impl ContextMenuState {
    pub fn items(&self) -> &'static [&'static str] {
        match self.kind {
            ContextMenuKind::Sidebar { .. } => &["new", "space", "group"],
            ContextMenuKind::Group {
                can_delete: true, ..
            } => &[
                "new", "space", "group", "---", "manage", "settings", "---", "danger", "delete",
            ],
            ContextMenuKind::Group {
                can_delete: false, ..
            } => &["new", "space", "group", "---", "manage", "settings"],
            ContextMenuKind::Workspace {
                project_commands, ..
            } => WORKSPACE_CONTEXT_MENU_ITEMS[project_commands.menu_index()],
            ContextMenuKind::Tab { .. } => &["rename", "close", "close other tabs"],
            ContextMenuKind::Agent {
                in_follow_up: false,
                ..
            } => ADD_TO_FOLLOW_UP_CONTEXT_ITEMS,
            ContextMenuKind::Agent {
                in_follow_up: true, ..
            } => REMOVE_FROM_FOLLOW_UP_CONTEXT_ITEMS,
            ContextMenuKind::NewTabButton {
                project_commands, ..
            } => NEW_TAB_CONTEXT_MENU_ITEMS[project_commands.menu_index()],
            ContextMenuKind::Pane {
                has_manual_label: true,
                right_click_passthrough: false,
                ..
            } => &[
                "rename pane",
                "clear pane name",
                "split vertical",
                "split horizontal",
                "zoom",
                "send right-clicks to pane",
                "close pane",
            ],
            ContextMenuKind::Pane {
                has_manual_label: true,
                right_click_passthrough: true,
                ..
            } => &[
                "rename pane",
                "clear pane name",
                "split vertical",
                "split horizontal",
                "zoom",
                "use gardn right-click menu",
                "close pane",
            ],
            ContextMenuKind::Pane {
                has_manual_label: false,
                right_click_passthrough: false,
                ..
            } => &[
                "rename pane",
                "split vertical",
                "split horizontal",
                "zoom",
                "send right-clicks to pane",
                "close pane",
            ],
            ContextMenuKind::Pane {
                has_manual_label: false,
                right_click_passthrough: true,
                ..
            } => &[
                "rename pane",
                "split vertical",
                "split horizontal",
                "zoom",
                "use gardn right-click menu",
                "close pane",
            ],
        }
    }

    pub fn item_is_selectable(&self, idx: usize) -> bool {
        self.items().get(idx).is_some_and(|item| {
            !Self::item_is_separator(item) && !Self::item_is_section_header(item)
        })
    }

    pub fn item_is_separator(item: &str) -> bool {
        item == "---"
    }

    pub fn item_is_section_header(item: &str) -> bool {
        matches!(item, "new" | "manage" | "danger")
    }

    pub fn item_display_label(item: &str) -> &str {
        match item {
            "new" => "New",
            "space" => "Space",
            "group" => "Group",
            "tab" => "Tab",
            "agent" => "Agent",
            "browser" => "Browser",
            "review" => "Review",
            "editor" => "Editor",
            "github" => "GitHub",
            "manage" => "Manage",
            "rename" => "Rename",
            "settings" => "Settings",
            "danger" => "Danger",
            "close" => "Close",
            "delete" => "Delete",
            "close other tabs" => "Close Other Tabs",
            ADD_TO_FOLLOW_UP_CONTEXT_ITEM => "Add to Follow Up",
            REMOVE_FROM_FOLLOW_UP_CONTEXT_ITEM => "Remove from Follow Up",
            "rename pane" => "Rename Pane",
            "clear pane name" => "Clear Pane Name",
            "split vertical" => "Split Vertical",
            "split horizontal" => "Split Horizontal",
            "zoom" => "Zoom",
            "send right-clicks to pane" => "Send Right-Clicks to Pane",
            "use gardn right-click menu" => "Use Gardn Right-Click Menu",
            "close pane" => "Close Pane",
            _ => item,
        }
    }

    pub fn visible_item_range(&self, row_count: usize) -> std::ops::Range<usize> {
        let item_count = self.items().len();
        let visible_count = row_count.min(item_count);
        if visible_count == 0 {
            return 0..0;
        }

        let anchor = self
            .list
            .visible()
            .unwrap_or(self.list.selected)
            .min(item_count - 1);
        let start = anchor
            .saturating_add(1)
            .saturating_sub(visible_count)
            .min(item_count - visible_count);
        start..start + visible_count
    }

    pub fn item_at_visible_row(&self, row: usize, row_count: usize) -> Option<usize> {
        self.visible_item_range(row_count).nth(row)
    }

    pub fn move_prev(&mut self) {
        let current = self.list.selected;
        if current > 0 {
            let mut idx = current - 1;
            loop {
                if self.item_is_selectable(idx) {
                    self.list.select(idx);
                    return;
                }
                if idx == 0 {
                    break;
                }
                idx -= 1;
            }
        }

        if self.item_is_selectable(current) {
            self.list.select(current);
        }
    }

    pub fn move_next(&mut self) {
        let current = self.list.selected;
        let item_count = self.items().len();
        let mut idx = current.saturating_add(1);
        while idx < item_count {
            if self.item_is_selectable(idx) {
                self.list.select(idx);
                return;
            }
            idx += 1;
        }

        if self.item_is_selectable(current) {
            self.list.select(current);
        }
    }

    pub fn hover(&mut self, idx: Option<usize>) {
        let hovered = idx.filter(|idx| self.item_is_selectable(*idx));
        self.list.hover(hovered);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigIssueImpact {
    UsingDefaults,
    KeepingCurrent,
    Warnings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigDiagnosticEntry {
    pub number: String,
    pub title: String,
    pub details: Vec<String>,
}

impl ConfigDiagnosticEntry {
    fn new(index: usize, diagnostic: &str) -> Self {
        let (title, details) = if let Some((title, details)) = diagnostic.split_once(": ") {
            (title, Some(details))
        } else if let Some((title, details)) = diagnostic.split_once("; ") {
            (title, Some(details))
        } else {
            (diagnostic, None)
        };
        let details = details
            .into_iter()
            .flat_map(|details| details.split("; "))
            .flat_map(str::lines)
            .map(str::trim)
            .filter(|detail| !detail.is_empty())
            .map(str::to_owned)
            .collect();
        Self {
            number: (index + 1).to_string(),
            title: title.trim().to_string(),
            details,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigIssue {
    pub details: String,
    pub(crate) entries: Vec<ConfigDiagnosticEntry>,
    pub impact: ConfigIssueImpact,
}

impl ConfigIssue {
    pub fn from_details(details: String) -> Self {
        let diagnostics = details.lines().map(str::to_owned).collect();
        Self::from_diagnostics(diagnostics)
    }

    pub fn from_diagnostics(diagnostics: Vec<String>) -> Self {
        let details = diagnostics.join("\n");
        let impact = if details.contains("using defaults") {
            ConfigIssueImpact::UsingDefaults
        } else if details.contains("keeping current") {
            ConfigIssueImpact::KeepingCurrent
        } else {
            ConfigIssueImpact::Warnings
        };
        let entries = diagnostics
            .iter()
            .enumerate()
            .map(|(index, diagnostic)| ConfigDiagnosticEntry::new(index, diagnostic))
            .collect();
        Self {
            details,
            entries,
            impact,
        }
    }

    pub fn summary(&self) -> &'static str {
        match self.impact {
            ConfigIssueImpact::UsingDefaults => "Gardn is using default settings.",
            ConfigIssueImpact::KeepingCurrent => "Some changes were not applied.",
            ConfigIssueImpact::Warnings => "Some configuration was ignored.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    NeedsAttention,
    Finished,
    UpdateInstalled,
}

#[derive(Debug, Clone, PartialEq, Eq)]

pub struct ToastTarget {
    pub workspace_id: String,
    pub pane_id: PaneId,
}

#[cfg(test)]
mod context_menu_tests {
    use super::*;

    #[test]
    fn workspace_context_menu_orders_all_project_roles_before_management() {
        let menu = ContextMenuState {
            kind: ContextMenuKind::Workspace {
                ws_idx: 0,
                project_commands: ProjectCommandAvailability::ALL,
            },
            x: 0,
            y: 0,
            list: ModalListState::new(1),
        };

        assert_eq!(
            menu.items(),
            &[
                "new", "tab", "agent", "editor", "browser", "review", "github", "---", "manage",
                "rename", "settings", "---", "danger", "close",
            ]
        );
    }

    #[test]
    fn context_menu_window_follows_keyboard_selection_on_short_screens() {
        let mut menu = ContextMenuState {
            kind: ContextMenuKind::Workspace {
                ws_idx: 0,
                project_commands: ProjectCommandAvailability::ALL,
            },
            x: 0,
            y: 0,
            list: ModalListState::new(1),
        };

        assert_eq!(menu.visible_item_range(11), 0..11);
        menu.list.select(13);
        assert_eq!(menu.visible_item_range(11), 3..14);
        assert_eq!(menu.item_at_visible_row(10, 11), Some(13));
    }

    #[test]
    fn project_command_menu_hides_each_unavailable_role() {
        let menu = |project_commands| ContextMenuState {
            kind: ContextMenuKind::NewTabButton {
                ws_idx: 0,
                project_commands,
            },
            x: 0,
            y: 0,
            list: ModalListState::new(1),
        };

        assert_eq!(
            menu(ProjectCommandAvailability::NONE).items(),
            &["new", "tab", "agent"]
        );
        assert_eq!(
            menu(ProjectCommandAvailability::EDITOR).items(),
            &["new", "tab", "agent", "editor"]
        );
        assert_eq!(
            menu(ProjectCommandAvailability::BROWSER).items(),
            &["new", "tab", "agent", "browser"]
        );
        assert_eq!(
            menu(ProjectCommandAvailability::REVIEW).items(),
            &["new", "tab", "agent", "review"]
        );
        assert_eq!(
            menu(ProjectCommandAvailability::GITHUB).items(),
            &["new", "tab", "agent", "github"]
        );
    }

    #[test]
    fn project_command_availability_requires_repos_only_for_review_role() {
        assert_eq!(
            ProjectCommandAvailability::from_repo_and_configured(true, true, false, true, true,),
            ProjectCommandAvailability::BROWSER
                .union(ProjectCommandAvailability::EDITOR)
                .union(ProjectCommandAvailability::GITHUB)
        );
        assert_eq!(
            ProjectCommandAvailability::from_repo_and_configured(false, true, true, true, true,),
            ProjectCommandAvailability::BROWSER
                .union(ProjectCommandAvailability::EDITOR)
                .union(ProjectCommandAvailability::GITHUB)
        );
    }

    #[test]
    fn tab_context_menu_exposes_only_tab_operations() {
        let menu = ContextMenuState {
            kind: ContextMenuKind::Tab {
                ws_idx: 0,
                tab_idx: 1,
            },
            x: 0,
            y: 0,
            list: ModalListState::new(0),
        };

        assert_eq!(menu.items(), &["rename", "close", "close other tabs"]);
    }
}

/// Typed connection lifecycle request queued for the connection runtime.
///
/// Recorded by Settings → Connections; the runtime worker drains and executes
/// these against system OpenSSH. Queuing a request never claims success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConnectionRequest {
    /// Stable id of the saved SSH connection profile the request targets.
    pub profile_id: String,
    /// Canonical connection verb shared with the host runtime.
    pub action: crate::execution_host::HostConnectionAction,
    /// Client-view owner for any interactive OpenSSH prompt this request may raise.
    ///
    /// `AuthenticationOwner::SYSTEM` is reserved for non-interactive / system paths.
    pub authentication_owner: crate::execution_host::auth::AuthenticationOwner,
}

/// Active screen in the connection settings workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionEditorMode {
    /// Creating a new profile (no stable id yet).
    New,
    /// Viewing status and controls for an existing profile.
    Detail { profile_id: String },
    /// Editing the persistent fields of an existing profile.
    Edit { profile_id: String },
}

/// Non-optional draft fields while the connection editor is open.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConnectionDraft {
    pub name: String,
    pub target: String,
    pub directory: String,
}

/// Cross-session and managed-binding impact shown before connection retirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionRetirementPreview {
    pub plan: crate::execution_host::connection_retirement::ConnectionRetirementPlan,
    pub bindings: crate::execution_host::runtime_paths::BindingInventoryReport,
}

/// Destructive connection retirement state owned by one connection editor.
#[derive(Debug, Clone)]
pub enum ConnectionRetirementState {
    InventoryPending,
    Review(ConnectionRetirementPreview),
    Running(ConnectionRetirementPreview),
    Failed,
    LocalForgetRunning,
}

/// Connection workflow state: screen mode + draft + typed retirement state.
///
/// Invalid combinations (draft without a screen or retirement for a different
/// profile) are unrepresentable because retirement lives inside this state.
#[derive(Debug, Clone)]
pub struct ConnectionEditorState {
    pub mode: ConnectionEditorMode,
    pub draft: ConnectionDraft,
    pub pending_forget_remote_terminal: Option<crate::terminal::TerminalId>,
    pub connection_retirement: Option<ConnectionRetirementState>,
}

impl ConnectionEditorState {
    pub fn new_draft() -> Self {
        Self {
            mode: ConnectionEditorMode::New,
            draft: ConnectionDraft::default(),
            pending_forget_remote_terminal: None,
            connection_retirement: None,
        }
    }

    pub fn detail_profile(
        profile_id: impl Into<String>,
        name: impl Into<String>,
        target: impl Into<String>,
        directory: impl Into<String>,
    ) -> Self {
        Self::existing_profile(
            ConnectionEditorMode::Detail {
                profile_id: profile_id.into(),
            },
            name,
            target,
            directory,
        )
    }
    fn existing_profile(
        mode: ConnectionEditorMode,
        name: impl Into<String>,
        target: impl Into<String>,
        directory: impl Into<String>,
    ) -> Self {
        Self {
            mode,
            draft: ConnectionDraft {
                name: name.into(),
                target: target.into(),
                directory: directory.into(),
            },
            pending_forget_remote_terminal: None,
            connection_retirement: None,
        }
    }

    pub fn profile_id(&self) -> Option<&str> {
        match &self.mode {
            ConnectionEditorMode::New => None,
            ConnectionEditorMode::Detail { profile_id }
            | ConnectionEditorMode::Edit { profile_id } => Some(profile_id.as_str()),
        }
    }

    pub fn is_detail(&self) -> bool {
        matches!(self.mode, ConnectionEditorMode::Detail { .. })
    }

    pub fn is_editing(&self) -> bool {
        matches!(self.mode, ConnectionEditorMode::Edit { .. })
    }

    pub(crate) fn retirement_in_progress(&self) -> bool {
        matches!(
            self.connection_retirement,
            Some(
                ConnectionRetirementState::InventoryPending
                    | ConnectionRetirementState::Running(_)
                    | ConnectionRetirementState::LocalForgetRunning
            )
        )
    }

    pub fn start_editing(&mut self) -> bool {
        let ConnectionEditorMode::Detail { profile_id } = &self.mode else {
            return false;
        };
        self.mode = ConnectionEditorMode::Edit {
            profile_id: profile_id.clone(),
        };
        true
    }

    pub fn show_detail(&mut self) -> bool {
        let ConnectionEditorMode::Edit { profile_id } = &self.mode else {
            return false;
        };
        self.mode = ConnectionEditorMode::Detail {
            profile_id: profile_id.clone(),
        };
        true
    }

    pub fn apply_retirement_preview(
        &mut self,
        profile_id: &str,
        result: &Result<ConnectionRetirementPreview, String>,
    ) -> bool {
        if self.profile_id() != Some(profile_id) {
            return false;
        }
        self.connection_retirement = Some(match result {
            Ok(preview) => ConnectionRetirementState::Review(preview.clone()),
            Err(_) => ConnectionRetirementState::Failed,
        });
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastNotification {
    pub kind: ToastKind,
    pub title: String,
    pub context: String,
    pub position: Option<crate::config::ToastGardnPosition>,
    pub target: Option<ToastTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAgentNotification {
    pub pane_id: PaneId,
    pub workspace_id: String,
    pub agent_label: String,
    pub known_agent: Option<crate::detect::Agent>,
    pub kind: ToastKind,
    pub state: AgentState,
    pub deadline: std::time::Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentNotificationDelivery {
    pub pane_id: PaneId,
    pub workspace_id: String,
    pub agent_label: String,
    pub known_agent: Option<crate::detect::Agent>,
    pub kind: ToastKind,
    pub toast: Option<ToastNotification>,
    pub client_notification: Option<ToastNotification>,
    pub sound: Option<crate::sound::Sound>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyFeedback {
    pub message: String,
}

#[derive(Clone)]
pub struct ReleaseNotesState {
    pub version: String,
    pub body: String,
    pub scroll: u16,
    pub preview: bool,
}

#[derive(Clone)]
pub struct ProductAnnouncementState {
    pub version: String,
    pub id: String,
    pub title: String,
    pub body: String,
    pub scroll: u16,
    pub preview: bool,
}

#[derive(Clone, Default)]
pub struct KeybindHelpState {
    pub scroll: u16,
    pub query: String,
    pub search_focused: bool,
}

#[derive(Clone)]
pub struct CommandPaletteState {
    pub query: String,
    pub list: ModalListState,
    pub scroll: usize,
}

#[derive(Clone)]
pub struct AgentProfilePickerState {
    pub ws_idx: usize,
    pub query: String,
    pub list: ModalListState,
    pub kind_filter: Option<crate::agent_profiles::AgentKind>,
    pub scroll: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectCommandKind {
    Browser,
    Review,
    Editor,
    Github,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProjectCommandAvailability(u8);

impl ProjectCommandAvailability {
    #[cfg(test)]
    pub const NONE: Self = Self(0);
    pub const BROWSER: Self = Self(1 << 1);
    pub const REVIEW: Self = Self(1 << 0);
    pub const EDITOR: Self = Self(1 << 2);
    pub const GITHUB: Self = Self(1 << 3);
    #[cfg(test)]
    pub const ALL: Self = Self(Self::BROWSER.0 | Self::REVIEW.0 | Self::EDITOR.0 | Self::GITHUB.0);

    pub(crate) const fn from_repo_and_configured(
        has_repo: bool,
        browser_configured: bool,
        review_configured: bool,
        editor_configured: bool,
        github_configured: bool,
    ) -> Self {
        let mut bits = 0;
        if browser_configured {
            bits |= Self::BROWSER.0;
        }
        if has_repo && review_configured {
            bits |= Self::REVIEW.0;
        }
        if editor_configured {
            bits |= Self::EDITOR.0;
        }
        if github_configured {
            bits |= Self::GITHUB.0;
        }
        Self(bits)
    }

    #[cfg(test)]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    const fn menu_index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepoPickerState {
    pub ws_idx: usize,
    pub command_kind: ProjectCommandKind,
    pub roots: Vec<std::path::PathBuf>,
    pub list: ModalListState,
    pub scroll: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarWidthSource {
    ConfigDefault,
    Persisted,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PaneFocusTarget {
    pub workspace_id: String,
    pub pane_id: PaneId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PopupPaneState {
    pub pane_id: PaneId,
    pub terminal_id: crate::terminal::TerminalId,
    pub width: Option<crate::popup_size::PopupSize>,
    pub height: Option<crate::popup_size::PopupSize>,
    pub owner: Option<u64>,
}

/// Durable record of a remote runtime that has left the live layout but still
/// needs a worker-side termination acknowledgement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemoteTerminationTombstone {
    pub terminal_id: crate::terminal::TerminalId,
    pub location: crate::execution_host::ResourceLocation,
    pub remote_runtime_identity: crate::execution_host::protocol::RuntimeIdentity,
}

/// All application state — pure data, no channels or async runtime.
/// Testable without PTYs or a tokio runtime.
#[derive(Clone)]
pub struct AppState {
    pub groups: Vec<Group>,
    pub active_group: usize,
    pub group_filter_enabled: bool,
    /// Detached plugin popup panes. Runtime ownership remains in `App`.
    pub(crate) popup_panes: std::collections::HashMap<PaneId, PopupPaneState>,
    pub terminals:
        std::collections::HashMap<crate::terminal::TerminalId, crate::terminal::TerminalState>,
    pub git_repo_summaries:
        std::collections::HashMap<std::path::PathBuf, crate::workspace::GitWorkSummary>,
    pub(crate) next_agent_activity_seq: u64,
    /// Terminal ids whose size is currently owned by a direct attach client.
    pub direct_attach_resize_locks: std::collections::HashSet<crate::terminal::TerminalId>,
    /// Pure render metadata mapping client-local overlay panes to their owning view.
    pub(crate) client_overlay_owners: std::collections::HashMap<PaneId, u64>,
    pub(crate) pane_id_aliases: std::collections::HashMap<u32, PaneId>,
    pub(crate) public_pane_id_aliases: std::collections::HashMap<String, PaneId>,
    pub workspaces: Vec<Workspace>,
    pub active: Option<usize>,
    pub(crate) previous_pane_focus: Option<PaneFocusTarget>,
    pub selected: usize,
    pub mode: Mode,
    pub should_quit: bool,
    /// In monolithic --no-session mode, detach exits the app because there is no server to detach from.
    pub detach_exits: bool,
    /// Set when the current client should detach from the persistent session.
    /// The server's event loop checks this and handles client detach.
    pub detach_requested: bool,
    pub request_new_workspace: bool,
    pub request_new_tab: bool,
    pub request_new_tab_for_client: Option<(usize, Option<String>)>,
    pub request_agent_profile_tab: Option<(usize, String)>,
    pub request_reload_config: bool,
    pub request_open_project_command: Option<ProjectCommandKind>,
    pub request_open_project_command_workspace: Option<usize>,
    /// Set when the headless server should ask attached clients to reload
    /// their client-local sound config from disk.
    pub request_client_config_reload: bool,
    /// Set when UI interaction requested a clipboard write that must be
    pub group_default_execution_host_id: crate::execution_host::ExecutionHostId,
    /// handled by the outer App/event loop instead of directly from AppState.
    pub request_clipboard_write: Option<Vec<u8>>,
    pub creating_new_tab: bool,
    pub creating_new_group: bool,
    pub group_icon_input: String,
    pub group_default_directory_input: String,
    pub group_modal_selected_field: usize,
    pub group_icon_picker_open: bool,
    pub rename_group_target: Option<usize>,
    pub requested_new_tab_name: Option<String>,
    /// Host-qualified location captured for a pending interactive workspace creation prompt.
    pub pending_workspace_create_location: Option<crate::execution_host::ResourceLocation>,
    /// Custom name captured when a pending workspace prompt is saved by mouse input.
    pub requested_new_workspace_name: Option<String>,
    pub rename_pane_target: Option<PaneId>,
    pub confirm_delete_group: Option<usize>,
    pub request_complete_onboarding: bool,
    pub name_input: String,
    pub name_input_replace_on_type: bool,
    pub release_notes: Option<ReleaseNotesState>,
    pub product_announcement: Option<ProductAnnouncementState>,
    pub keybind_help: KeybindHelpState,
    pub config_diagnostics_scroll: u16,
    pub navigator: NavigatorState,
    pub command_palette: CommandPaletteState,
    pub agent_profile_picker: AgentProfilePickerState,
    pub git_repo_picker: GitRepoPickerState,
    pub command_catalog: Vec<crate::commands::ProjectCommand>,
    pub command_runs: std::collections::HashMap<String, crate::commands::CommandRun>,
    pub port_registry: crate::ports::PortRegistry,
    pub copy_mode: Option<CopyModeState>,
    pub agent_profiles: crate::agent_profiles::AgentProfileCatalog,
    pub workspace_scroll: usize,
    pub agent_panel_scroll: usize,
    pub tab_scroll: usize,
    pub tab_scroll_follow_active: bool,
    pub hovered_tab: Option<usize>,
    pub(crate) collapsed_sidebar_hover: Option<CollapsedSidebarHover>,
    pub mobile_switcher_scroll: usize,
    pub(crate) mobile_switcher_level: MobileSwitcherLevel,
    pub(crate) mobile_switcher_selected: usize,
    pub(crate) mobile_agents_expanded: bool,
    // View geometry (computed before render, consumed by render + mouse)
    pub view: ViewState,
    pub(crate) drag: Option<DragState>,

    pub(crate) workspace_press: Option<WorkspacePressState>,
    pub(crate) group_press: Option<GroupPressState>,
    pub(crate) tab_press: Option<TabPressState>,
    pub(crate) agent_press: Option<AgentPressState>,
    pub(crate) agent_follow_up: Vec<AgentFollowUpEntry>,
    pub selection: Option<Selection>,
    pub selection_autoscroll: Option<SelectionAutoscroll>,
    pub context_menu: Option<ContextMenuState>,
    // Notifications
    pub update_available: Option<String>,
    pub update_install: crate::install::UpdateInstallAction,
    pub latest_release_notes_available: bool,
    pub update_dismissed: bool,
    pub config_diagnostic: Option<String>,
    pub config_issue: Option<ConfigIssue>,
    pub toast: Option<ToastNotification>,
    pub pending_agent_notifications: std::collections::HashMap<PaneId, PendingAgentNotification>,
    pub copy_feedback: Option<CopyFeedback>,
    /// Last reported focus state for the outer terminal hosting Gardn.
    /// None means unsupported or not yet reported, which preserves active-pane suppression.
    pub outer_terminal_focus: Option<bool>,
    // Config
    pub prefix_code: KeyCode,
    pub prefix_mods: KeyModifiers,
    pub headless_size: (u16, u16),
    /// Configured outer terminal window-title template.
    pub window_title_template: String,
    pub host_display: crate::app::host_label::HostDisplayNameOverlay,

    pub default_sidebar_width: u16,
    pub sidebar_width: u16,
    pub sidebar_min_width: u16,
    pub sidebar_max_width: u16,
    pub mobile_width_threshold: u16,
    pub sidebar_width_source: SidebarWidthSource,
    pub sidebar_width_auto: bool,
    pub sidebar_collapsed: bool,
    pub right_sidebar_width: u16,
    pub right_sidebar_collapsed: bool,
    pub sidebar_arrangement: crate::config::SidebarArrangementConfig,
    pub context_bar_visibility: crate::config::ContextBarVisibilityConfig,
    /// Per-process override used by the monolithic client. Attached clients own this separately.
    pub context_bar_visibility_override: Option<bool>,
    /// Per-process Zen mode used by the monolithic client.
    pub zen_mode: bool,
    /// Sidebar row/token layout loaded from `[ui.sidebar]`.
    pub sidebar_config: crate::config::SidebarConfig,
    /// Ratio of sidebar height allocated to the workspaces section when activity
    /// is stacked into the same sidebar.
    pub sidebar_section_split: f32,
    pub activity_agents_expanded: bool,
    pub activity_commands_expanded: bool,
    pub activity_ports_expanded: bool,
    pub collapsed_agent_sections: Vec<String>,
    pub collapsed_command_groups: Vec<String>,
    pub collapsed_command_status_groups: Vec<String>,
    pub collapsed_workspace_groups: Vec<String>,
    pub agent_panel_scope: AgentPanelScope,
    /// Keep a just-focused Done agent in Triage until focus leaves that pane.
    pub(crate) triage_hold: Option<(String, crate::layout::PaneId)>,
    /// Capture mouse input for Gardn's own mouse UI. When false, Gardn only
    /// captures mouse while the focused pane app requests mouse reporting.
    pub mouse_capture: bool,
    /// Latest pane mouse-move waiting for the next motion flush.
    pub(crate) pending_pane_mouse_motion: Option<PendingPaneMouseMotion>,
    /// When pane pointer events were last written to a PTY.
    pub(crate) last_pane_mouse_motion_flush: Option<Instant>,
    /// Host is currently in DEC 1016 SGR-pixels mouse reporting.
    pub(crate) host_sgr_pixels: bool,
    /// Last known host cell size for converting 1016 pixels to cells.
    pub(crate) host_cell_size: crate::kitty_graphics::HostCellSize,
    /// Host-pixel position for the current/queued pointer event.
    pub(crate) pointer_host_pixels: Option<(u32, u32)>,
    /// Accumulated pane wheel ticks waiting for the next motion flush.
    pub(crate) pending_pane_wheel: Option<PendingPaneWheel>,
    /// Automatically copy mouse drag selections on completion. When false, retain drag or double-click word selection until Ctrl+C or a host-forwarded Cmd+C. Default: true.
    pub copy_on_select: bool,
    pub right_click_passthrough_modifiers: Option<KeyModifiers>,
    pub right_click_passthrough: Option<RightClickPassthroughGesture>,
    pub redraw_on_focus_gained: bool,
    pub mouse_scroll_lines: usize,
    pub confirm_close: bool,
    pub prompt_new_tab_name: bool,
    pub prompt_new_workspace_name: bool,
    pub pane_borders: bool,
    pub pane_scrollbars: bool,
    pub pane_gaps: bool,
    pub hide_tab_bar_when_single_tab: bool,
    pub show_counters: bool,
    pub sidebar_collapsed_mode: crate::config::SidebarCollapsedModeConfig,
    pub browser_command: String,
    pub review_command: String,
    pub editor_command: String,
    pub pane_border_agent_info: PaneBorderAgentInfoConfig,
    pub status_indicators: StatusIndicatorStyle,
    pub pane_history_persistence: bool,
    pub resume_agents_on_restore: bool,
    /// Expose the focused pane's cursor anchor to the outer terminal even when
    /// the pane requested `?25l`. See `[experimental] reveal_hidden_cursor_for_cjk_ime`.
    pub reveal_hidden_cursor_for_cjk_ime: bool,
    /// Restrict cursor reveal to focused panes whose detected agent matches
    /// one of these. When false, apply to any focused pane.
    pub cjk_ime_agent_filter_configured: bool,
    pub cjk_ime_agents: Vec<crate::detect::Agent>,
    /// DECSCUSR shape parameter (1–6) for the IME anchor cursor.
    pub cjk_ime_cursor_shape: u8,
    /// While prefix mode is active, switch the host input source to an
    /// ASCII-capable mode so prefix commands register as ASCII even when an
    /// IME is active. macOS and Windows (Korean IME); a no-op elsewhere. See
    /// `[experimental] switch_ascii_input_source_in_prefix`.
    pub switch_ascii_input_source_in_prefix: bool,
    pub kitty_graphics_enabled: bool,
    pub default_shell: String,
    pub shell_mode: crate::config::ShellModeConfig,
    pub new_terminal_cwd: NewTerminalCwdConfig,
    pub pane_scrollback_limit_bytes: usize,
    pub sound: SoundConfig,
    pub local_sound_playback: bool,
    pub toast_config: ToastConfig,
    pub update_version_check: bool,
    pub update_manifest_check: bool,
    pub keybinds: Keybinds,
    /// Frame counter for spinner animations (wraps around).
    pub spinner_tick: u32,
    /// UI color palette — all sidebar/UI colors centralized for theming.
    pub palette: Palette,
    /// Default app palette from config, used when the active group has no override.
    pub global_palette: Palette,
    pub theme_name: String,
    /// Default app theme name from config.
    pub global_theme_name: String,
    /// Default app light/dark mode from config.
    pub global_theme_mode: ThemeMode,
    pub(crate) effective_theme_appearance: ThemeAppearance,
    /// Default app light theme from config.
    pub global_light_theme_name: String,
    /// Default app dark theme from config.
    pub global_dark_theme_name: String,
    /// ANSI color used for the app accent when terminal colors resolve light.
    pub global_terminal_light_accent: TerminalAccent,
    /// ANSI color used for the app accent when terminal colors resolve dark.
    pub global_terminal_dark_accent: TerminalAccent,
    /// Settings panel state.
    pub settings: SettingsState,
    /// Cached integration recommendations for onboarding/settings UI.
    pub integration_recommendations: Vec<crate::integration::IntegrationRecommendation>,
    /// Host-qualified integration state reported by managed execution workers.
    pub(crate) host_integration_observations: std::collections::HashMap<
        crate::execution_host::ExecutionHostId,
        crate::integration::host::HostIntegrationObservation,
    >,
    /// Latest coordinator integration request for each execution host.
    pub(crate) host_integration_request_ids: std::collections::HashMap<
        crate::execution_host::ExecutionHostId,
        crate::execution_host::protocol::RequestId,
    >,
    /// Integration action feedback scoped to its execution host.
    pub(crate) host_integration_install_messages:
        std::collections::HashMap<crate::execution_host::ExecutionHostId, Vec<String>>,
    /// Coordinator-owned catalog of saved SSH connection profiles.
    ///
    /// Persisted through `persist::ssh_profiles`; replaced only after a
    /// successful atomic write. Connection drafts live in per-client
    /// `SettingsState`, never here.
    pub ssh_connection_profiles: Vec<crate::persist::ssh_profiles::SshConnectionProfile>,
    /// User-visible health per execution host binding, reported by the
    /// connection runtime. Pure state: missing entries mean disconnected.
    pub host_connection_states: std::collections::HashMap<
        crate::execution_host::ExecutionHostId,
        crate::execution_host::ConnectionStatus,
    >,
    /// Connection lifecycle requests queued for the connection runtime to drain.
    pub pending_ssh_connection_requests: Vec<SshConnectionRequest>,
    /// Cached detection manifest source/version summaries for runtime/API status.
    pub agent_manifest_summaries: Vec<crate::detect::manifest::AgentManifestSummary>,
    /// Cached remote detection manifest update diagnostics for runtime/API status.
    pub agent_manifest_update_status: crate::detect::manifest_update::ManifestUpdateStatus,
    /// Result messages from the latest integration install action.
    pub integration_install_messages: Vec<String>,
    /// Installed or linked plugins known to this running Gardn instance.
    pub(crate) installed_plugins: InstalledPluginRegistry,
    /// Pane ids opened through the plugin pane API.
    pub(crate) plugin_panes: std::collections::HashMap<PaneId, PluginPaneRecord>,
    /// Recent plugin action/event command executions.
    pub(crate) plugin_command_logs: Vec<crate::api::schema::PluginCommandLogInfo>,
    pub(crate) next_plugin_command_log_id: u64,
    pub(crate) plugin_commands_in_flight: usize,
    /// Highlight state for the bottom-right global launcher menu.
    pub global_menu: ModalListState,
    /// Highlight state for the sidebar group switcher menu.
    pub group_menu: ModalListState,
    /// Highlight state for the right-sidebar agent scope menu.
    pub agent_menu: ModalListState,
    /// Resolved host terminal default colors for theming embedded panes.
    pub host_terminal_theme: TerminalTheme,
    /// Durable namespace for remote execution-worker runtime adoption.
    pub session_namespace_id: SessionNamespaceId,
    /// Remote runtimes awaiting acknowledged worker-side termination.
    pub(crate) remote_termination_tombstones: Vec<RemoteTerminationTombstone>,
    /// Set when a persisted session snapshot would change.
    pub session_dirty: bool,
    /// Terminal runtimes that should be shut down by the app/runtime layer
    /// after state has detached their terminal metadata.
    pub(crate) terminal_runtime_shutdowns: Vec<crate::terminal::TerminalId>,
}

impl AppState {
    pub(crate) fn host_label<'a>(
        &'a self,
        target: crate::app::host_label::HostLabelTarget<'a>,
    ) -> crate::app::host_label::HostLabel<'a> {
        match target {
            crate::app::host_label::HostLabelTarget::Coordinator => self.host_display.coordinator(),
            crate::app::host_label::HostLabelTarget::ExecutionHost(host_id)
                if host_id.is_local() =>
            {
                self.host_display.coordinator()
            }
            crate::app::host_label::HostLabelTarget::ExecutionHost(host_id) => self
                .ssh_connection_profiles
                .iter()
                .find(|profile| profile.execution_host_id() == *host_id)
                .map(|profile| crate::app::host_label::HostLabel::new(profile.name()))
                .unwrap_or_else(|| crate::app::host_label::HostLabel::new(host_id.as_str())),
        }
    }

    /// User-visible connection status for a profile's current host binding.
    ///
    /// Pure state: defaults to disconnected until the connection runtime
    /// reports health for this binding.
    pub(crate) fn ssh_connection_status(
        &self,
        profile: &crate::persist::ssh_profiles::SshConnectionProfile,
    ) -> crate::execution_host::ConnectionStatus {
        self.host_connection_states
            .get(&profile.execution_host_id())
            .cloned()
            .unwrap_or_default()
    }

    /// Queue a typed connection lifecycle request for the connection runtime.
    pub(crate) fn queue_ssh_connection_request(
        &mut self,
        profile_id: impl Into<String>,
        action: crate::execution_host::HostConnectionAction,
        authentication_owner: crate::execution_host::auth::AuthenticationOwner,
    ) {
        self.pending_ssh_connection_requests
            .push(SshConnectionRequest {
                profile_id: profile_id.into(),
                action,
                authentication_owner,
            });
    }

    pub(crate) fn next_agent_activity_seq(&mut self) -> u64 {
        self.next_agent_activity_seq = self.next_agent_activity_seq.saturating_add(1);
        self.next_agent_activity_seq
    }

    pub fn theme_appearance_for_mode(&self, mode: ThemeMode) -> ThemeAppearance {
        mode.resolve(self.host_terminal_theme)
    }
    pub fn global_theme_name_for_appearance(&self, appearance: ThemeAppearance) -> &str {
        match appearance {
            ThemeAppearance::Light => &self.global_light_theme_name,
            ThemeAppearance::Dark => &self.global_dark_theme_name,
        }
    }

    pub fn global_theme_name_for_mode(&self, mode: ThemeMode) -> &str {
        self.global_theme_name_for_appearance(self.theme_appearance_for_mode(mode))
    }

    pub fn palette_for_theme_mode(&self, theme_name: &str, mode: ThemeMode) -> Option<Palette> {
        self.palette_for_theme_mode_with_terminal_accents(
            theme_name,
            mode,
            self.global_terminal_light_accent,
            self.global_terminal_dark_accent,
        )
    }

    pub fn terminal_accent_for_mode(&self, mode: ThemeMode) -> TerminalAccent {
        match self.theme_appearance_for_mode(mode) {
            crate::terminal_theme::ThemeAppearance::Light => self.global_terminal_light_accent,
            crate::terminal_theme::ThemeAppearance::Dark => self.global_terminal_dark_accent,
        }
    }

    pub fn palette_for_theme_mode_with_terminal_accents(
        &self,
        theme_name: &str,
        mode: ThemeMode,
        terminal_light_accent: TerminalAccent,
        terminal_dark_accent: TerminalAccent,
    ) -> Option<Palette> {
        let appearance = self.theme_appearance_for_mode(mode);
        let accent = match appearance {
            crate::terminal_theme::ThemeAppearance::Light => terminal_light_accent,
            crate::terminal_theme::ThemeAppearance::Dark => terminal_dark_accent,
        };
        Palette::from_theme_with_terminal_accent(
            theme_name,
            appearance,
            self.host_terminal_theme,
            accent,
        )
    }

    pub fn configured_global_palette(&self, theme_name: &str, mode: ThemeMode) -> Option<Palette> {
        self.palette_for_theme_mode(theme_name, mode)
    }

    pub fn refresh_global_palette(&mut self) {
        let theme_name = self
            .global_theme_name_for_mode(self.global_theme_mode)
            .to_string();
        if let Some(palette) = self.configured_global_palette(&theme_name, self.global_theme_mode) {
            self.global_palette = palette;
            self.global_theme_name = theme_name;
        }
    }

    pub fn active_group_id(&self) -> &str {
        self.groups
            .get(self.active_group)
            .map(|group| group.id.as_str())
            .unwrap_or(crate::workspace::DEFAULT_GROUP_ID)
    }

    pub fn active_group_name(&self) -> &str {
        self.groups
            .get(self.active_group)
            .map(|group| group.name.as_str())
            .unwrap_or("group 1")
    }

    pub fn active_group_icon(&self) -> &str {
        self.groups
            .get(self.active_group)
            .map(|group| group.icon.as_str())
            .unwrap_or(DEFAULT_GROUP_ICON)
    }

    pub fn palette_for_group(&self, group_idx: usize) -> Palette {
        let mut palette = self.palette.clone();
        palette.accent = self.group_accent_color(group_idx);
        palette
    }

    pub fn palette_for_workspace(&self, ws_idx: usize) -> Palette {
        let mut palette = self.palette.clone();
        palette.accent = self
            .workspaces
            .get(ws_idx)
            .and_then(|workspace| self.group_index_by_id(&workspace.group_id))
            .map(|group_idx| self.group_accent_color(group_idx))
            .unwrap_or_else(|| self.active_workspace_accent_color());
        palette
    }

    pub fn group_accent_color(&self, group_idx: usize) -> Color {
        self.groups
            .get(group_idx)
            .and_then(|group| group.accent)
            .map(|accent| self.global_palette.theme_accent_color(accent))
            .unwrap_or(self.global_palette.accent)
    }

    pub fn active_workspace_accent_color(&self) -> Color {
        if !self.group_filter_enabled {
            if let Some(group_idx) = self
                .active
                .and_then(|ws_idx| self.workspaces.get(ws_idx))
                .and_then(|workspace| self.group_index_by_id(&workspace.group_id))
            {
                return self.group_accent_color(group_idx);
            }
        }

        self.group_accent_color(self.active_group)
    }

    pub fn group_index_by_id(&self, group_id: &str) -> Option<usize> {
        self.groups.iter().position(|group| group.id == group_id)
    }

    pub fn workspace_in_active_group(&self, ws_idx: usize) -> bool {
        if !self.group_filter_enabled {
            return self.workspaces.get(ws_idx).is_some();
        }

        self.workspaces
            .get(ws_idx)
            .is_some_and(|workspace| workspace.group_id == self.active_group_id())
    }

    pub fn visible_workspace_indices(&self) -> Vec<usize> {
        if !self.group_filter_enabled {
            return (0..self.workspaces.len()).collect();
        }

        let active_group_id = self.active_group_id();
        self.workspaces
            .iter()
            .enumerate()
            .filter_map(|(idx, workspace)| (workspace.group_id == active_group_id).then_some(idx))
            .collect()
    }

    pub fn workspace_group_collapsed(&self, group_id: &str) -> bool {
        self.collapsed_workspace_groups
            .iter()
            .any(|id| id == group_id)
    }

    pub fn agent_section_collapsed(&self, section_key: &str) -> bool {
        self.collapsed_agent_sections
            .iter()
            .any(|key| key == section_key)
    }

    pub fn toggle_agent_section(&mut self, section_key: String) {
        toggle_string_key(&mut self.collapsed_agent_sections, section_key);
    }

    pub(crate) fn context_bar_is_visible(&self, visibility_override: Option<bool>) -> bool {
        visibility_override.unwrap_or(matches!(
            self.context_bar_visibility,
            crate::config::ContextBarVisibilityConfig::Always
        ))
    }

    pub fn sidebar_visible_workspace_indices(&self) -> Vec<usize> {
        if self.sidebar_collapsed || self.group_filter_enabled {
            return self.visible_workspace_indices();
        }

        self.workspaces
            .iter()
            .enumerate()
            .filter_map(|(idx, workspace)| {
                (!self.workspace_group_collapsed(&workspace.group_id)).then_some(idx)
            })
            .collect()
    }

    pub fn toggle_workspace_group(&mut self, group_idx: usize) {
        let Some(group_id) = self.groups.get(group_idx).map(|group| group.id.clone()) else {
            return;
        };
        let previous_selected = self.selected;
        if let Some(idx) = self
            .collapsed_workspace_groups
            .iter()
            .position(|id| id == &group_id)
        {
            self.collapsed_workspace_groups.remove(idx);
        } else {
            self.collapsed_workspace_groups.push(group_id);
        }
        self.workspace_scroll = self
            .workspace_scroll
            .min(crate::ui::workspace_list_entry_count(self).saturating_sub(1));
        if !self
            .sidebar_visible_workspace_indices()
            .contains(&self.selected)
        {
            let visible = self.sidebar_visible_workspace_indices();
            if let Some(next) = visible
                .iter()
                .copied()
                .find(|idx| *idx > previous_selected)
                .or_else(|| {
                    visible
                        .iter()
                        .rev()
                        .copied()
                        .find(|idx| *idx < previous_selected)
                })
                .or_else(|| visible.first().copied())
            {
                self.selected = next;
                self.ensure_workspace_visible(next);
            }
        }
        self.mark_session_dirty();
    }

    pub fn first_visible_workspace(&self) -> Option<usize> {
        if !self.group_filter_enabled {
            return (!self.workspaces.is_empty()).then_some(0);
        }

        let active_group_id = self.active_group_id();
        self.workspaces
            .iter()
            .position(|workspace| workspace.group_id == active_group_id)
    }

    pub(crate) fn mark_session_dirty(&mut self) {
        self.session_dirty = true;
    }

    pub(crate) fn current_unix_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default()
    }

    pub(crate) fn follow_up_identity(
        &self,
        ws_idx: usize,
        pane_id: crate::layout::PaneId,
    ) -> Option<(String, usize)> {
        let workspace = self.workspaces.get(ws_idx)?;
        let pane_number = workspace.public_pane_number(pane_id)?;
        Some((workspace.id.clone(), pane_number))
    }

    pub(crate) fn resolve_live_agent_target(
        &self,
        workspace_id: &str,
        pane_number: usize,
    ) -> Option<(usize, usize, crate::layout::PaneId)> {
        let ws_idx = self
            .workspaces
            .iter()
            .position(|workspace| workspace.id == workspace_id)?;
        let workspace = self.workspaces.get(ws_idx)?;
        let pane_id = workspace
            .public_pane_numbers
            .iter()
            .find_map(|(pane_id, number)| (*number == pane_number).then_some(*pane_id))?;
        let tab_idx = workspace.find_tab_index_for_pane(pane_id)?;
        Some((ws_idx, tab_idx, pane_id))
    }

    pub(crate) fn prune_agent_follow_up(&mut self) {
        let before = self.agent_follow_up.len();
        let entries = std::mem::take(&mut self.agent_follow_up);
        self.agent_follow_up = Self::restored_agent_follow_up(&self.workspaces, entries);
        if self.agent_follow_up.len() != before {
            self.mark_session_dirty();
        }
    }

    pub(crate) fn restored_agent_follow_up(
        workspaces: &[Workspace],
        entries: Vec<AgentFollowUpEntry>,
    ) -> Vec<AgentFollowUpEntry> {
        let mut restored = Vec::new();
        for entry in entries {
            if restored.iter().any(|existing: &AgentFollowUpEntry| {
                existing.matches(&entry.workspace_id, entry.pane_number)
            }) {
                continue;
            }
            if workspaces.iter().any(|workspace| {
                workspace.id == entry.workspace_id
                    && workspace
                        .public_pane_numbers
                        .values()
                        .any(|number| *number == entry.pane_number)
            }) {
                restored.push(entry);
            }
        }
        restored
    }

    pub(crate) fn is_agent_follow_up(&self, ws_idx: usize, pane_id: crate::layout::PaneId) -> bool {
        let Some((workspace_id, pane_number)) = self.follow_up_identity(ws_idx, pane_id) else {
            return false;
        };
        self.agent_follow_up
            .iter()
            .any(|entry| entry.matches(&workspace_id, pane_number))
    }

    pub(crate) fn pane_is_in_triage(&self, ws_idx: usize, pane_id: crate::layout::PaneId) -> bool {
        let Some(workspace) = self.workspaces.get(ws_idx) else {
            return false;
        };
        let Some(pane) = workspace.pane_state(pane_id) else {
            return false;
        };
        let state = self
            .terminals
            .get(&pane.attached_terminal_id)
            .map(|terminal| terminal.state)
            .unwrap_or(AgentState::Unknown);
        if state == AgentState::Blocked {
            return true;
        }
        if state != AgentState::Idle {
            return false;
        }
        if !pane.seen {
            return true;
        }
        self.triage_hold
            .as_ref()
            .is_some_and(|(workspace_id, hold_pane)| {
                workspace_id == &workspace.id && *hold_pane == pane_id
            })
    }

    pub(crate) fn follow_up_added_at(
        &self,
        ws_idx: usize,
        pane_id: crate::layout::PaneId,
    ) -> Option<u64> {
        let (workspace_id, pane_number) = self.follow_up_identity(ws_idx, pane_id)?;
        self.agent_follow_up
            .iter()
            .find(|entry| entry.matches(&workspace_id, pane_number))
            .map(|entry| entry.added_at_unix_secs)
    }

    pub(crate) fn migrate_agent_follow_up(
        &mut self,
        old_workspace_id: &str,
        old_pane_number: usize,
        new_workspace_id: String,
        new_pane_number: usize,
    ) -> bool {
        if old_workspace_id == new_workspace_id && old_pane_number == new_pane_number {
            return false;
        }
        let Some(idx) = self
            .agent_follow_up
            .iter()
            .position(|entry| entry.matches(old_workspace_id, old_pane_number))
        else {
            return false;
        };
        if self
            .agent_follow_up
            .iter()
            .any(|entry| entry.matches(&new_workspace_id, new_pane_number))
        {
            self.agent_follow_up.remove(idx);
            self.mark_session_dirty();
            return true;
        }
        self.agent_follow_up[idx].workspace_id = new_workspace_id;
        self.agent_follow_up[idx].pane_number = new_pane_number;
        self.mark_session_dirty();
        true
    }

    pub(crate) fn insert_agent_follow_up(
        &mut self,
        ws_idx: usize,
        pane_id: crate::layout::PaneId,
    ) -> bool {
        let Some((workspace_id, pane_number)) = self.follow_up_identity(ws_idx, pane_id) else {
            return false;
        };
        if self
            .agent_follow_up
            .iter()
            .any(|entry| entry.matches(&workspace_id, pane_number))
        {
            return false;
        }
        self.agent_follow_up.push(AgentFollowUpEntry {
            workspace_id,
            pane_number,
            added_at_unix_secs: Self::current_unix_secs(),
        });
        self.mark_session_dirty();
        true
    }

    pub(crate) fn clear_agent_follow_up_for_pane(
        &mut self,
        workspace_id: &str,
        pane_id: crate::layout::PaneId,
    ) -> bool {
        let Some(workspace) = self
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
        else {
            return false;
        };
        let Some(pane_number) = workspace.public_pane_number(pane_id) else {
            return false;
        };
        let before = self.agent_follow_up.len();
        self.agent_follow_up
            .retain(|entry| !entry.matches(workspace_id, pane_number));
        if self.agent_follow_up.len() != before {
            self.mark_session_dirty();
            true
        } else {
            false
        }
    }

    pub(crate) fn project_command_availability_for_workspace(
        &self,
        terminal_runtimes: &crate::terminal::TerminalRuntimeRegistry,
        ws_idx: usize,
    ) -> ProjectCommandAvailability {
        let has_repo = !self
            .observed_git_repos_for_workspace(terminal_runtimes, ws_idx)
            .is_empty();
        ProjectCommandAvailability::from_repo_and_configured(
            has_repo,
            self.project_command_configured(ProjectCommandKind::Browser),
            self.project_command_configured(ProjectCommandKind::Review),
            self.project_command_configured(ProjectCommandKind::Editor),
            self.project_command_configured(ProjectCommandKind::Github),
        )
    }

    pub(crate) fn remove_alias_shadowed_by_new_pane(&mut self, pane_id: PaneId) {
        self.pane_id_aliases.remove(&pane_id.raw());
    }

    pub fn sound_enabled(&self) -> bool {
        self.sound.enabled
    }

    pub fn toast_delivery(&self) -> ToastDelivery {
        self.toast_config.delivery
    }

    pub fn confirm_close_enabled(&self) -> bool {
        self.confirm_close
    }

    pub fn prompt_new_tab_name_enabled(&self) -> bool {
        self.prompt_new_tab_name
    }

    pub(crate) fn project_command_role(&self, kind: ProjectCommandKind) -> &'static str {
        match kind {
            ProjectCommandKind::Browser => "Browser",
            ProjectCommandKind::Review => "Review",
            ProjectCommandKind::Editor => "Editor",
            ProjectCommandKind::Github => "GitHub",
        }
    }

    pub(crate) fn project_command_configured(&self, kind: ProjectCommandKind) -> bool {
        let command = match kind {
            ProjectCommandKind::Browser => &self.browser_command,
            ProjectCommandKind::Review => &self.review_command,
            ProjectCommandKind::Editor => &self.editor_command,
            ProjectCommandKind::Github => return true,
        };
        !command.trim().is_empty()
    }

    pub fn pane_border_agent_info(&self) -> PaneBorderAgentInfoConfig {
        self.pane_border_agent_info
    }

    pub fn status_indicators(&self) -> StatusIndicatorStyle {
        self.status_indicators
    }

    pub(crate) fn status_indicator_animation_active(&self) -> bool {
        self.status_indicators == StatusIndicatorStyle::Symbols
            && self
                .workspaces
                .iter()
                .flat_map(|workspace| workspace.tabs.iter())
                .flat_map(|tab| tab.panes.values())
                .any(|pane| {
                    self.terminals
                        .get(&pane.attached_terminal_id)
                        .is_some_and(|terminal| terminal.state == AgentState::Working)
                })
    }

    pub fn switch_ascii_input_source_in_prefix_enabled(&self) -> bool {
        self.switch_ascii_input_source_in_prefix
    }

    pub(crate) fn integration_updates_available(&self) -> bool {
        self.integration_recommendations
            .iter()
            .any(|item| item.state == crate::integration::IntegrationStatusKind::Outdated)
    }

    pub(crate) fn agent_kind_integration_installed(
        &self,
        kind: crate::agent_profiles::AgentKind,
    ) -> bool {
        let Some(target) = kind.integration_target() else {
            return false;
        };
        self.integration_recommendations.iter().any(|item| {
            item.target == target
                && matches!(
                    item.state,
                    crate::integration::IntegrationStatusKind::Current
                        | crate::integration::IntegrationStatusKind::Outdated
                )
        })
    }

    pub(crate) fn agent_profile_launchable(
        &self,
        profile: &crate::agent_profiles::AgentProfile,
    ) -> bool {
        profile.available() && self.agent_kind_integration_installed(profile.kind)
    }

    pub(crate) fn agent_profile_kind_available(
        &self,
        kind: crate::agent_profiles::AgentKind,
    ) -> bool {
        kind == crate::agent_profiles::AgentKind::Custom
            || self.agent_kind_integration_installed(kind)
    }

    pub(crate) fn agent_profile_kind_choices(
        &self,
    ) -> impl Iterator<Item = crate::agent_profiles::AgentKind> + '_ {
        crate::agent_profiles::AgentKind::ALL
            .iter()
            .copied()
            .filter(|kind| self.agent_profile_kind_available(*kind))
    }

    pub(crate) fn default_agent_profile_kind_choice(&self) -> crate::agent_profiles::AgentKind {
        self.agent_profile_kind_choices()
            .find(|kind| *kind != crate::agent_profiles::AgentKind::Custom)
            .unwrap_or(crate::agent_profiles::AgentKind::Custom)
    }

    pub(crate) fn refresh_agent_manifest_summaries(&mut self) {
        self.agent_manifest_summaries = crate::detect::manifest::manifest_summaries();
    }

    pub(crate) fn global_menu_attention_badge_visible(&self) -> bool {
        self.config_issue.is_some()
            || self.update_available.is_some()
            || self.latest_release_notes_available
            || self.integration_updates_available()
    }

    pub(crate) fn global_menu_item_has_badge(&self, item: &str) -> bool {
        (item == "Configuration Issue" && self.config_issue.is_some())
            || (item == "Update Ready" && self.update_available.is_some())
            || (item == "Changelog"
                && (self.update_available.is_some() || self.latest_release_notes_available))
            || (item == "Integrations" && self.integration_updates_available())
    }

    pub(crate) fn focused_pane_requests_mouse_capture_from_view(
        &self,
        terminal_runtimes: &crate::terminal::TerminalRuntimeRegistry,
        view: &crate::app::ClientViewState,
    ) -> bool {
        if view.mode != Mode::Terminal {
            return false;
        }
        let Some(ws_idx) = view.active_workspace else {
            return false;
        };
        let Some(workspace) = self.workspaces.get(ws_idx) else {
            return false;
        };
        let Some(tab_idx) = view.active_tab_for_workspace(&workspace.id) else {
            return false;
        };
        let Some(tab) = workspace.tabs.get(tab_idx) else {
            return false;
        };
        let pane_id = view
            .focused_pane_for_tab(&workspace.id, tab_idx + 1)
            .unwrap_or_else(|| tab.layout.focused());
        self.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, pane_id)
            .and_then(crate::terminal::TerminalRuntime::input_state)
            .is_some_and(crate::pane::InputState::mouse_reporting_enabled)
    }

    pub(crate) fn focused_pane_requests_sgr_pixels_from_view(
        &self,
        terminal_runtimes: &crate::terminal::TerminalRuntimeRegistry,
        view: &crate::app::ClientViewState,
    ) -> bool {
        if view.mode != Mode::Terminal {
            return false;
        }
        let Some(ws_idx) = view.active_workspace else {
            return false;
        };
        let Some(workspace) = self.workspaces.get(ws_idx) else {
            return false;
        };
        let Some(tab_idx) = view.active_tab_for_workspace(&workspace.id) else {
            return false;
        };
        let Some(tab) = workspace.tabs.get(tab_idx) else {
            return false;
        };
        let pane_id = view
            .focused_pane_for_tab(&workspace.id, tab_idx + 1)
            .unwrap_or_else(|| tab.layout.focused());
        self.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, pane_id)
            .and_then(crate::terminal::TerminalRuntime::input_state)
            .is_some_and(crate::pane::InputState::sgr_pixels_enabled)
    }

    pub(crate) fn should_capture_host_mouse_from_view(
        &self,
        terminal_runtimes: &crate::terminal::TerminalRuntimeRegistry,
        view: &crate::app::ClientViewState,
    ) -> bool {
        self.mouse_capture
            || self.focused_pane_requests_mouse_capture_from_view(terminal_runtimes, view)
    }

    pub fn is_prefix_key(&self, key: &crate::input::TerminalKey) -> bool {
        crate::config::terminal_key_matches_combo(key, (self.prefix_code, self.prefix_mods))
    }

    pub fn estimate_pane_size(&self) -> (u16, u16) {
        if let Some(info) = self.view.pane_infos.first() {
            (info.rect.height, info.rect.width)
        } else {
            (self.headless_size.1, self.headless_size.0)
        }
    }

    /// Returns true when the given (workspace, tab, pane) refers to the
    /// currently focused pane in the active workspace's active tab.
    pub(crate) fn runtime_for_pane_in_workspace<'a>(
        &'a self,
        terminal_runtimes: &'a crate::terminal::TerminalRuntimeRegistry,
        ws_idx: usize,
        pane_id: crate::layout::PaneId,
    ) -> Option<&'a crate::terminal::TerminalRuntime> {
        #[cfg(test)]
        if let Some(runtime) = self.workspaces.get(ws_idx)?.test_runtimes.get(&pane_id) {
            return Some(runtime);
        }
        #[cfg(test)]
        if let Some(runtime) = self
            .workspaces
            .get(ws_idx)?
            .tabs
            .iter()
            .find_map(|tab| tab.runtimes.get(&pane_id))
        {
            return Some(runtime);
        }
        let terminal_id = self.workspaces.get(ws_idx)?.terminal_id(pane_id)?;
        terminal_runtimes.get(terminal_id)
    }

    #[cfg(test)]
    pub(crate) fn runtime_for_pane<'a>(
        &'a self,
        terminal_runtimes: &'a crate::terminal::TerminalRuntimeRegistry,
        pane_id: crate::layout::PaneId,
    ) -> Option<&'a crate::terminal::TerminalRuntime> {
        self.workspaces.iter().find_map(|ws| {
            #[cfg(test)]
            if let Some(runtime) = ws.test_runtimes.get(&pane_id) {
                return Some(runtime);
            }
            #[cfg(test)]
            if let Some(runtime) = ws.tabs.iter().find_map(|tab| tab.runtimes.get(&pane_id)) {
                return Some(runtime);
            }
            let terminal_id = ws.terminal_id(pane_id)?;
            terminal_runtimes.get(terminal_id)
        })
    }

    pub(crate) fn focused_runtime_in_workspace<'a>(
        &'a self,
        terminal_runtimes: &'a crate::terminal::TerminalRuntimeRegistry,
        ws_idx: usize,
    ) -> Option<&'a crate::terminal::TerminalRuntime> {
        let ws = self.workspaces.get(ws_idx)?;
        let pane_id = ws.focused_pane_id()?;
        self.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, pane_id)
    }

    pub fn is_active_pane(
        &self,
        ws_idx: usize,
        tab_idx: usize,
        pane_id: crate::layout::PaneId,
    ) -> bool {
        let Some(active_ws_idx) = self.active else {
            return false;
        };
        if ws_idx != active_ws_idx {
            return false;
        }
        let Some(ws) = self.workspaces.get(ws_idx) else {
            return false;
        };
        if tab_idx != ws.active_tab_index() {
            return false;
        }
        ws.active_tab().map(|tab| tab.layout.focused()) == Some(pane_id)
    }
}

fn toggle_string_key(keys: &mut Vec<String>, key: String) {
    if let Some(idx) = keys.iter().position(|existing| existing == &key) {
        keys.remove(idx);
    } else {
        keys.push(key);
    }
}

#[allow(dead_code)]
pub fn key_matches(
    key: &crossterm::event::KeyEvent,
    expected_code: KeyCode,
    expected_mods: KeyModifiers,
) -> bool {
    crate::config::terminal_key_matches_combo(
        &crate::input::TerminalKey::from(*key),
        (expected_code, expected_mods),
    )
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
impl AppState {
    /// Create an AppState for testing — no channels, no PTYs.
    pub fn test_new() -> Self {
        Self {
            groups: vec![Group::default_group()],
            active_group: 0,
            group_filter_enabled: true,
            terminals: std::collections::HashMap::new(),
            popup_panes: std::collections::HashMap::new(),
            git_repo_summaries: std::collections::HashMap::new(),
            next_agent_activity_seq: 0,
            direct_attach_resize_locks: std::collections::HashSet::new(),
            client_overlay_owners: std::collections::HashMap::new(),
            pane_id_aliases: std::collections::HashMap::new(),
            public_pane_id_aliases: std::collections::HashMap::new(),
            workspaces: Vec::new(),
            active: None,
            selected: 0,
            mode: Mode::Navigate,
            should_quit: false,
            detach_exits: false,
            detach_requested: false,
            request_new_workspace: false,
            request_new_tab: false,
            request_new_tab_for_client: None,
            request_agent_profile_tab: None,
            request_reload_config: false,
            request_open_project_command: None,
            request_open_project_command_workspace: None,
            group_default_execution_host_id: crate::execution_host::ExecutionHostId::local(),
            request_client_config_reload: false,
            request_clipboard_write: None,
            creating_new_tab: false,
            creating_new_group: false,
            group_icon_input: DEFAULT_GROUP_ICON.to_string(),
            group_default_directory_input: String::new(),
            group_modal_selected_field: 0,
            group_icon_picker_open: false,
            rename_group_target: None,
            requested_new_tab_name: None,
            pending_workspace_create_location: None,
            requested_new_workspace_name: None,
            rename_pane_target: None,
            confirm_delete_group: None,
            request_complete_onboarding: false,
            name_input: String::new(),
            name_input_replace_on_type: false,
            release_notes: None,
            product_announcement: None,
            keybind_help: KeybindHelpState::default(),
            config_diagnostics_scroll: 0,
            command_palette: CommandPaletteState {
                query: String::new(),
                list: ModalListState::hidden(0),
                scroll: 0,
            },
            agent_profile_picker: AgentProfilePickerState {
                ws_idx: 0,
                query: String::new(),
                kind_filter: None,
                list: ModalListState::hidden(0),
                scroll: 0,
            },
            git_repo_picker: GitRepoPickerState {
                ws_idx: 0,
                command_kind: ProjectCommandKind::Review,
                roots: Vec::new(),
                list: ModalListState::hidden(0),
                scroll: 0,
            },
            navigator: NavigatorState::default(),
            previous_pane_focus: None,
            command_catalog: Vec::new(),
            command_runs: std::collections::HashMap::new(),
            port_registry: crate::ports::PortRegistry::default(),
            copy_mode: None,
            workspace_scroll: 0,
            agent_panel_scroll: 0,
            tab_scroll: 0,
            tab_scroll_follow_active: true,
            hovered_tab: None,
            collapsed_sidebar_hover: None,
            mobile_switcher_scroll: 0,
            mobile_switcher_level: MobileSwitcherLevel::default(),
            mobile_switcher_selected: 0,
            mobile_agents_expanded: false,
            view: ViewState {
                layout: ViewLayout::Desktop,
                sidebar_rect: Rect::default(),
                right_sidebar_rect: Rect::default(),
                workspace_card_areas: Vec::new(),
                workspace_group_header_areas: Vec::new(),
                workspace_group_empty_areas: Vec::new(),
                tab_bar_rect: Rect::default(),
                tab_hit_areas: Vec::new(),
                tab_close_hit_areas: Vec::new(),
                tab_scroll_left_hit_area: Rect::default(),
                tab_scroll_right_hit_area: Rect::default(),
                new_tab_hit_area: Rect::default(),
                context_bar: ContextBarView::default(),
                terminal_area: Rect::default(),
                mobile_header_rect: Rect::default(),
                toast_hit_area: Rect::default(),
                pane_infos: Vec::new(),
                split_borders: Vec::new(),
            },
            drag: None,
            workspace_press: None,
            group_press: None,
            tab_press: None,
            agent_press: None,
            agent_follow_up: Vec::new(),
            selection: None,
            selection_autoscroll: None,
            context_menu: None,
            update_available: None,
            update_install: crate::install::UpdateInstallAction::Direct,
            latest_release_notes_available: false,
            update_dismissed: false,
            config_diagnostic: None,
            config_issue: None,
            toast: None,
            pending_agent_notifications: std::collections::HashMap::new(),
            outer_terminal_focus: None,
            prefix_code: KeyCode::Char('b'),
            prefix_mods: KeyModifiers::CONTROL,
            headless_size: (
                crate::config::DEFAULT_HEADLESS_COLS,
                crate::config::DEFAULT_HEADLESS_ROWS,
            ),
            window_title_template: crate::config::Config::default().ui.window_title,
            host_display: crate::app::host_label::HostDisplayNameOverlay::from_config_or_hostname(
                "test-host",
                None,
            ),

            default_sidebar_width: 26,
            sidebar_width: 26,
            sidebar_min_width: 18,
            sidebar_max_width: 36,
            sidebar_width_source: SidebarWidthSource::ConfigDefault,
            sidebar_width_auto: false,
            sidebar_collapsed: false,
            right_sidebar_width: 28,
            right_sidebar_collapsed: false,
            sidebar_arrangement: crate::config::SidebarArrangementConfig::Auto,
            context_bar_visibility: crate::config::ContextBarVisibilityConfig::Always,
            context_bar_visibility_override: None,
            zen_mode: false,
            sidebar_config: crate::config::SidebarConfig::default(),
            sidebar_section_split: 0.5,
            activity_agents_expanded: true,
            activity_commands_expanded: false,
            activity_ports_expanded: false,
            collapsed_agent_sections: Vec::new(),
            collapsed_command_groups: Vec::new(),
            collapsed_command_status_groups: Vec::new(),
            collapsed_workspace_groups: Vec::new(),
            agent_panel_scope: AgentPanelScope::CurrentWorkspace,
            triage_hold: None,
            mouse_capture: true,
            pending_pane_mouse_motion: None,
            last_pane_mouse_motion_flush: None,
            host_sgr_pixels: false,
            host_cell_size: crate::kitty_graphics::HostCellSize::default(),
            pointer_host_pixels: None,
            pending_pane_wheel: None,
            copy_on_select: true,
            right_click_passthrough_modifiers: None,
            right_click_passthrough: None,
            redraw_on_focus_gained: true,
            mouse_scroll_lines: crate::config::DEFAULT_MOUSE_SCROLL_LINES,
            confirm_close: true,
            prompt_new_tab_name: true,
            prompt_new_workspace_name: false,
            pane_borders: true,
            pane_scrollbars: true,
            pane_gaps: true,
            hide_tab_bar_when_single_tab: false,
            show_counters: false,
            sidebar_collapsed_mode: crate::config::SidebarCollapsedModeConfig::default(),
            copy_feedback: None,
            browser_command: "terminal-browser".to_string(),
            review_command: "hunk diff --watch".to_string(),
            editor_command: "fresh .".to_string(),
            pane_border_agent_info: PaneBorderAgentInfoConfig::default(),
            status_indicators: StatusIndicatorStyle::default(),
            mobile_width_threshold: crate::config::DEFAULT_MOBILE_WIDTH_THRESHOLD,
            pane_history_persistence: true,
            resume_agents_on_restore: true,
            reveal_hidden_cursor_for_cjk_ime: false,
            cjk_ime_agent_filter_configured: false,
            cjk_ime_agents: Vec::new(),
            cjk_ime_cursor_shape: 2, // steady_block
            switch_ascii_input_source_in_prefix: false,
            kitty_graphics_enabled: false,
            default_shell: String::new(),
            shell_mode: crate::config::ShellModeConfig::Auto,
            new_terminal_cwd: NewTerminalCwdConfig::Follow,
            pane_scrollback_limit_bytes: crate::config::DEFAULT_SCROLLBACK_LIMIT_BYTES,
            sound: SoundConfig {
                enabled: false,
                ..SoundConfig::default()
            },
            local_sound_playback: false,
            toast_config: ToastConfig {
                delay_seconds: 0,
                ..ToastConfig::default()
            },
            update_version_check: true,
            update_manifest_check: true,
            agent_profiles: crate::agent_profiles::AgentProfileCatalog::from_config(
                &crate::agent_profiles::AgentProfilesConfig::default(),
            ),
            keybinds: Keybinds::default(),
            spinner_tick: 0,
            palette: Palette::catppuccin(),
            global_palette: Palette::catppuccin(),
            theme_name: "catppuccin".to_string(),
            global_theme_name: "catppuccin".to_string(),
            global_theme_mode: ThemeMode::System,
            effective_theme_appearance: ThemeAppearance::Dark,
            global_light_theme_name: DEFAULT_LIGHT_THEME_NAME.to_string(),
            global_dark_theme_name: DEFAULT_DARK_THEME_NAME.to_string(),
            global_terminal_light_accent: TerminalAccent::Blue,
            global_terminal_dark_accent: TerminalAccent::Blue,
            settings: SettingsState {
                section: SettingsSection::Theme,
                sidebar_expanded: Some(SettingsSection::Theme),
                sidebar_selection: SettingsSidebarSelection::section(SettingsSection::Theme),
                sidebar_focused: false,
                list: ModalListState::hidden(0),
                focused_input: None,
                scroll: 0,
                original_palette: None,
                original_theme: None,
                pending_theme_name: None,
                pending_theme_mode: None,
                pending_light_theme_name: None,
                pending_dark_theme_name: None,
                pending_terminal_light_accent: None,
                pending_terminal_dark_accent: None,
                pending_sound_enabled: None,
                pending_toast_delivery: None,
                pending_default_shell: None,
                pending_shell_mode: None,
                pending_version_check: None,
                pending_manifest_check: None,
                pending_toast_delay: None,
                pending_toast_gardn_position: None,
                pending_clipboard_toast_enabled: None,
                pending_clipboard_toast_position: None,
                pending_confirm_close: None,
                pending_prompt_new_tab_name: None,
                pending_show_counters: None,
                pending_pane_borders: None,
                pending_pane_scrollbars: None,
                pending_pane_gaps: None,
                pending_hide_tab_bar_when_single_tab: None,
                pending_copy_on_select: None,
                pending_prompt_new_workspace_name: None,
                pending_right_click_passthrough_modifier: None,
                pending_new_terminal_cwd: None,
                pending_context_bar_visibility: None,
                pending_mouse_scroll_lines: None,
                pending_browser_command: None,
                pending_review_command: None,
                pending_editor_command: None,
                pending_sidebar_width: None,
                pending_sidebar_arrangement: None,
                pending_sidebar_initial_state: None,
                pending_sidebar_initial_agent_scope: None,
                pending_sidebar_min_width: None,
                pending_sidebar_max_width: None,
                pending_pane_border_agent_info: None,
                pending_status_indicators: None,
                pending_switch_ascii_input_source_in_prefix: None,
                pending_resume_agents_on_restore: None,
                pending_window_title: None,
                pending_headless_cols: None,
                pending_headless_rows: None,
                pending_group_accent_choice: None,
                pending_group_name: None,
                pending_group_icon: None,
                pending_group_github_organization: None,

                pending_group_default_directory: None,
                pending_group_default_execution_host_id: None,

                pending_workspace_name: None,
                pending_workspace_default_cwd: None,
                pending_workspace_default_execution_host_id: None,
                pending_workspace_github_scope: None,
                pending_workspace_github_repositories: None,
                pending_agent_profile_id: None,
                pending_agent_profile_name: None,
                pending_agent_profile_kind: None,
                pending_agent_profile_command: None,
                pending_agent_profile_enabled: None,
                agent_profile_kind_filter: None,
                integration_host_profile_id: None,
                connection_editor: None,
                group_settings_target: None,
                group_icon_picker_open: false,
                workspace_settings_target: None,
            },
            integration_recommendations: Vec::new(),
            host_integration_observations: std::collections::HashMap::new(),
            host_integration_request_ids: std::collections::HashMap::new(),
            host_integration_install_messages: std::collections::HashMap::new(),
            ssh_connection_profiles: Vec::new(),
            host_connection_states: std::collections::HashMap::new(),
            pending_ssh_connection_requests: Vec::new(),
            agent_manifest_summaries: Vec::new(),
            agent_manifest_update_status:
                crate::detect::manifest_update::ManifestUpdateStatus::default(),
            integration_install_messages: Vec::new(),
            installed_plugins: std::collections::HashMap::new(),
            plugin_panes: std::collections::HashMap::new(),
            plugin_command_logs: Vec::new(),
            next_plugin_command_log_id: 1,
            plugin_commands_in_flight: 0,
            global_menu: ModalListState::hidden(0),
            group_menu: ModalListState::hidden(0),
            agent_menu: ModalListState::hidden(0),
            host_terminal_theme: TerminalTheme::default(),
            session_namespace_id: crate::persist::installation::new_session_namespace_id(),
            remote_termination_tombstones: Vec::new(),
            session_dirty: false,
            terminal_runtime_shutdowns: Vec::new(),
        }
    }

    /// Populate missing `TerminalState` entries for every pane so tests that
    /// read or write terminal metadata don't need to manually create them.
    pub fn ensure_test_terminals(&mut self) {
        use crate::terminal::TerminalState;
        for ws in &self.workspaces {
            for tab in &ws.tabs {
                for pane in tab.panes.values() {
                    if !self.terminals.contains_key(&pane.attached_terminal_id) {
                        self.terminals.insert(
                            pane.attached_terminal_id.clone(),
                            TerminalState::new_at(
                                pane.attached_terminal_id.clone(),
                                ws.default_location.clone(),
                            ),
                        );
                    }
                }
            }
        }
    }

    pub fn insert_test_runtime(
        &mut self,
        pane_id: crate::layout::PaneId,
        runtime: crate::terminal::TerminalRuntime,
    ) {
        if let Some(ws) = self
            .workspaces
            .iter_mut()
            .find(|ws| ws.terminal_id(pane_id).is_some())
        {
            ws.insert_test_runtime(pane_id, runtime);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn settings_section_order_keeps_about_last() {
        assert_eq!(
            SettingsSection::ALL,
            &[
                SettingsSection::Theme,
                SettingsSection::Sound,
                SettingsSection::PaneLabels,
                SettingsSection::Commands,
                SettingsSection::Agents,
                SettingsSection::Integrations,
                SettingsSection::Connections,
                SettingsSection::Experiments,
                SettingsSection::About,
            ]
        );
    }

    #[test]
    fn host_labels_use_overlay_and_profile_names() {
        let mut app = AppState::test_new();
        let local = crate::execution_host::ExecutionHostId::local();
        assert_eq!(
            app.host_label(crate::app::host_label::HostLabelTarget::Coordinator)
                .as_str(),
            "test-host"
        );
        assert_eq!(
            app.host_label(crate::app::host_label::HostLabelTarget::ExecutionHost(
                &local
            ))
            .as_str(),
            "test-host"
        );

        let profile = crate::persist::ssh_profiles::SshConnectionProfile::new(
            "workbox",
            "Work box",
            "build.example",
            None,
        )
        .expect("valid profile");
        let host_id = profile.execution_host_id();
        app.ssh_connection_profiles.push(profile);
        assert_eq!(
            app.host_label(crate::app::host_label::HostLabelTarget::ExecutionHost(
                &host_id
            ))
            .as_str(),
            "Work box"
        );

        let unresolved =
            crate::execution_host::ExecutionHostId::new("ssh:missing:1").expect("valid host id");
        assert_eq!(
            app.host_label(crate::app::host_label::HostLabelTarget::ExecutionHost(
                &unresolved
            ))
            .as_str(),
            "ssh:missing:1"
        );
    }

    #[test]
    fn group_icons_are_single_cell_fun_distinct_set() {
        assert_eq!(
            GROUP_ICONS,
            &[
                "☀", "☁", "☂", "♥", "♪", "⚑", "⚙", "☎", "☄", "☘", "✉", "✿", "✂", "✎", "✚", "⊕",
                "▥", "⌁",
            ]
        );
        assert_eq!(DEFAULT_GROUP_ICON, GROUP_ICONS[0]);

        let wide_icons = GROUP_ICONS
            .iter()
            .copied()
            .filter(|icon| icon.width() != 1)
            .collect::<Vec<_>>();
        assert!(wide_icons.is_empty(), "wide group icons: {wide_icons:?}");
    }

    #[test]
    fn built_in_theme_names_resolve() {
        for name in THEME_NAMES {
            assert!(
                Palette::from_theme(name, ThemeAppearance::Dark).is_some(),
                "theme should resolve: {name}"
            );
        }
    }

    #[test]
    fn rose_pine_surface_dim_uses_the_overlay_swatch() {
        // Rosé Pine's "overlay" swatch keeps dimmed surfaces readable against
        // the base background; the base color here made them invisible.
        assert_eq!(Palette::rose_pine().surface_dim, Color::Rgb(38, 35, 58));
    }

    #[test]
    fn light_theme_names_resolve_to_light_appearance() {
        for name in LIGHT_THEME_NAMES {
            assert!(
                Palette::from_theme(name, ThemeAppearance::Light).is_some(),
                "light theme should resolve: {name}"
            );
        }
    }
    #[test]
    fn monokai_pro_variants_resolve() {
        for name in [
            "monokai-pro",
            "monokai-pro-light",
            "monokai-pro-light-sun",
            "monokai-pro-spectrum",
            "monokai-pro-ristretto",
            "monokai-pro-octagon",
            "monokai-pro-machine",
            "monokai-classic",
        ] {
            assert!(
                Palette::from_name(name).is_some(),
                "monokai variant should resolve: {name}"
            );
        }
    }
    #[test]
    fn catppuccin_flavors_resolve() {
        for name in [
            "catppuccin-latte",
            "catppuccin-frappe",
            "catppuccin-macchiato",
            "catppuccin",
            "catppuccin-mocha",
        ] {
            assert!(
                Palette::from_name(name).is_some(),
                "catppuccin flavor should resolve: {name}"
            );
        }
    }

    #[test]
    fn gardn_day_and_night_resolve_and_pair_across_appearance() {
        let day = Palette::from_name("gardn-day").expect("gardn day");
        let night = Palette::from_name("gardn-night").expect("gardn night");
        assert_eq!(day.accent, Color::Rgb(11, 90, 60));
        assert_eq!(day.panel_bg, Color::Rgb(255, 255, 252));
        assert_eq!(day.text, Color::Rgb(31, 31, 31));
        assert_eq!(night.accent, Color::Rgb(125, 186, 114));
        assert_eq!(night.panel_bg, Color::Rgb(7, 26, 19));
        assert_eq!(night.text, Color::Rgb(247, 243, 234));
        assert_eq!(
            theme_name_for_appearance("gardn-night", ThemeAppearance::Light),
            Some("gardn-day")
        );
        assert_eq!(
            theme_name_for_appearance("gardn-day", ThemeAppearance::Dark),
            Some("gardn-night")
        );
    }

    #[test]
    fn catppuccin_flavors_use_official_palette_values() {
        let latte = Palette::from_name("catppuccin-latte").expect("latte");
        assert_eq!(latte.panel_bg, Color::Rgb(239, 241, 245));
        assert_eq!(latte.surface0, Color::Rgb(204, 208, 218));
        assert_eq!(latte.text, Color::Rgb(76, 79, 105));

        let frappe = Palette::from_name("catppuccin-frappe").expect("frappe");
        assert_eq!(frappe.panel_bg, Color::Rgb(48, 52, 70));
        assert_eq!(frappe.surface0, Color::Rgb(65, 69, 89));
        assert_eq!(frappe.text, Color::Rgb(198, 208, 245));

        let macchiato = Palette::from_name("catppuccin-macchiato").expect("macchiato");
        assert_eq!(macchiato.panel_bg, Color::Rgb(36, 39, 58));
        assert_eq!(macchiato.surface0, Color::Rgb(54, 58, 79));
        assert_eq!(macchiato.text, Color::Rgb(202, 211, 245));

        let mocha = Palette::from_name("catppuccin").expect("mocha");
        assert_eq!(mocha.panel_bg, Color::Rgb(30, 30, 46));
        assert_eq!(mocha.surface0, Color::Rgb(49, 50, 68));
        assert_eq!(mocha.text, Color::Rgb(205, 214, 244));
    }
    #[test]
    fn flexoki_variants_use_official_website_values() {
        let light = Palette::from_name("flexoki-light").expect("flexoki light");
        assert_eq!(light.accent, Color::Rgb(36, 131, 123));
        assert_eq!(light.panel_bg, Color::Rgb(255, 252, 240));
        assert_eq!(light.surface_dim, Color::Rgb(242, 240, 229));
        assert_eq!(light.surface0, Color::Rgb(230, 228, 217));
        assert_eq!(light.surface1, Color::Rgb(206, 205, 195));
        assert_eq!(light.text, Color::Rgb(16, 15, 15));

        let dark = Palette::from_name("flexoki").expect("flexoki");
        assert_eq!(dark.accent, Color::Rgb(58, 169, 159));
        assert_eq!(dark.panel_bg, Color::Rgb(16, 15, 15));
        assert_eq!(dark.surface_dim, Color::Rgb(28, 27, 26));
        assert_eq!(dark.surface0, Color::Rgb(40, 39, 38));
        assert_eq!(dark.surface1, Color::Rgb(64, 62, 60));
        assert_eq!(dark.text, Color::Rgb(206, 205, 195));
    }

    #[test]
    fn light_theme_aliases_resolve() {
        for name in ["light", "latte", "tokyo-day", "onelight", "lotus", "dawn"] {
            assert!(
                Palette::from_name(name).is_some(),
                "theme should resolve: {name}"
            );
        }
    }

    #[test]
    fn appearance_theme_lists_do_not_include_terminal_color_sources() {
        for names in [
            theme_names_for_appearance(ThemeAppearance::Light),
            theme_names_for_appearance(ThemeAppearance::Dark),
        ] {
            assert!(!names.contains(&"system"));
            assert!(!names.contains(&"terminal"));
        }
    }

    #[test]
    fn key_matches_requires_exact_modifiers() {
        assert!(key_matches(
            &KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
            KeyCode::Char('b'),
            KeyModifiers::CONTROL,
        ));

        assert!(!key_matches(
            &KeyEvent::new(
                KeyCode::Char('b'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            KeyCode::Char('b'),
            KeyModifiers::CONTROL,
        ));
    }

    #[test]
    fn key_matches_letters_case_insensitively() {
        assert!(key_matches(
            &KeyEvent::new(KeyCode::Char('B'), KeyModifiers::SHIFT),
            KeyCode::Char('b'),
            KeyModifiers::SHIFT,
        ));
    }

    #[test]
    fn dark_only_theme_is_not_valid_in_light_mode() {
        assert!(Palette::from_theme("nord", ThemeAppearance::Light).is_none());
    }

    #[test]
    fn theme_config_names_derive_appearance_pair_from_legacy_name() {
        let config = ThemeConfig {
            name: Some("gruvbox".to_string()),
            ..ThemeConfig::default()
        };

        assert_eq!(
            theme_config_names(&config),
            ("gruvbox-light".to_string(), "gruvbox".to_string())
        );
    }

    #[test]
    fn system_theme_uses_terminal_defaults_and_ansi_palette() {
        let mut host_theme = TerminalTheme {
            foreground: Some(crate::terminal_theme::RgbColor {
                r: 220,
                g: 221,
                b: 222,
            }),
            background: Some(crate::terminal_theme::RgbColor {
                r: 10,
                g: 11,
                b: 12,
            }),
            ..Default::default()
        };
        host_theme.palette[1] = Some(crate::terminal_theme::RgbColor {
            r: 180,
            g: 40,
            b: 50,
        });
        host_theme.palette[2] = Some(crate::terminal_theme::RgbColor {
            r: 30,
            g: 160,
            b: 80,
        });
        host_theme.palette[3] = Some(crate::terminal_theme::RgbColor {
            r: 210,
            g: 170,
            b: 30,
        });
        host_theme.palette[4] = Some(crate::terminal_theme::RgbColor {
            r: 80,
            g: 130,
            b: 230,
        });
        host_theme.palette[5] = Some(crate::terminal_theme::RgbColor {
            r: 160,
            g: 90,
            b: 200,
        });
        host_theme.palette[6] = Some(crate::terminal_theme::RgbColor {
            r: 30,
            g: 180,
            b: 170,
        });
        host_theme.palette[7] = Some(crate::terminal_theme::RgbColor {
            r: 210,
            g: 211,
            b: 212,
        });
        host_theme.palette[8] = Some(crate::terminal_theme::RgbColor {
            r: 120,
            g: 121,
            b: 122,
        });

        let palette =
            Palette::from_theme_with_terminal("system", ThemeAppearance::Dark, host_theme)
                .expect("system theme resolves");

        assert_eq!(palette.panel_bg, Color::Reset);
        assert_eq!(palette.text, Color::Rgb(220, 221, 222));
        assert_eq!(palette.overlay0, Color::Rgb(126, 127, 128));
        assert_eq!(palette.overlay1, Color::Rgb(178, 179, 180));
        assert_eq!(palette.subtext0, Color::Rgb(147, 148, 149));
        assert_eq!(palette.accent, Color::Rgb(80, 130, 230));
        assert_eq!(palette.green, Color::Rgb(30, 160, 80));
        assert_eq!(palette.yellow, Color::Rgb(210, 170, 30));
        assert_eq!(palette.red, Color::Rgb(180, 40, 50));
        assert_eq!(palette.blue, Color::Rgb(80, 130, 230));
        assert_eq!(palette.teal, Color::Rgb(30, 180, 170));
        assert_eq!(palette.mauve, Color::Rgb(160, 90, 200));
        assert_ne!(palette.accent, palette.text);
        assert_ne!(palette.surface0, Palette::catppuccin().surface0);
    }

    #[test]
    fn system_theme_uses_selected_terminal_accent() {
        let mut host_theme = TerminalTheme::default();
        host_theme.palette[5] = Some(crate::terminal_theme::RgbColor {
            r: 160,
            g: 90,
            b: 200,
        });

        let palette = Palette::from_theme_with_terminal_accent(
            "system",
            ThemeAppearance::Dark,
            host_theme,
            crate::config::TerminalAccent::Magenta,
        )
        .expect("system theme resolves");

        assert_eq!(palette.accent, Color::Rgb(160, 90, 200));
        assert_eq!(palette.blue, Color::Blue);
    }

    #[test]
    fn system_theme_derives_neutral_text_from_terminal_foreground() {
        let mut host_theme = TerminalTheme {
            foreground: Some(crate::terminal_theme::RgbColor { r: 0, g: 0, b: 0 }),
            background: Some(crate::terminal_theme::RgbColor {
                r: 255,
                g: 255,
                b: 255,
            }),
            ..Default::default()
        };
        host_theme.palette[7] = Some(crate::terminal_theme::RgbColor {
            r: 250,
            g: 250,
            b: 250,
        });
        host_theme.palette[8] = Some(crate::terminal_theme::RgbColor {
            r: 230,
            g: 230,
            b: 230,
        });

        let palette =
            Palette::from_theme_with_terminal("system", ThemeAppearance::Light, host_theme)
                .expect("system theme resolves");

        assert_eq!(palette.text, Color::Rgb(0, 0, 0));
        assert_eq!(palette.overlay0, Color::Rgb(115, 115, 115));
        assert_eq!(palette.overlay1, Color::Rgb(51, 51, 51));
        assert_eq!(palette.subtext0, Color::Rgb(89, 89, 89));
        assert_ne!(palette.overlay0, Color::Rgb(230, 230, 230));
        assert_ne!(palette.overlay1, Color::Rgb(250, 250, 250));
    }

    #[test]
    fn system_theme_falls_back_to_ansi_colors_not_catppuccin() {
        let palette = Palette::from_theme_with_terminal(
            "system",
            ThemeAppearance::Dark,
            TerminalTheme::default(),
        )
        .expect("system theme resolves");

        assert_eq!(palette.panel_bg, Color::Reset);
        assert_eq!(palette.surface0, Color::Reset);
        assert_eq!(palette.accent, Color::Blue);
        assert_eq!(palette.green, Color::Green);
        assert_eq!(palette.yellow, Color::Yellow);
        assert_eq!(palette.red, Color::LightRed);
        assert_eq!(palette.teal, Color::Cyan);
        assert_eq!(palette.mauve, Color::Magenta);
    }

    #[test]
    fn github_organization_parser_enforces_github_name_rules() {
        assert_eq!(
            GithubOrganization::parse(" masakiro-corp "),
            Ok(Some(GithubOrganization("masakiro-corp".to_string())))
        );
        assert_eq!(GithubOrganization::parse(""), Ok(None));
        for value in [
            "-leading",
            "trailing-",
            "double--hyphen",
            "under_score",
            "é",
        ] {
            assert!(GithubOrganization::parse(value).is_err(), "{value}");
        }
        assert!(GithubOrganization::parse(&"a".repeat(40)).is_err());
    }
}
