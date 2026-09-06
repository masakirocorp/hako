#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}
pub(crate) type AnsiPalette = [RgbColor; 16];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedTerminalTheme {
    pub foreground: RgbColor,
    pub background: RgbColor,
    pub cursor: RgbColor,
    pub palette: AnsiPalette,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PaneTerminalTheme {
    pub host: TerminalTheme,
    pub resolved_override: Option<ResolvedTerminalTheme>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TerminalThemeSource {
    WorkspacePalette,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TerminalThemeBinding {
    pub source: TerminalThemeSource,
}

impl TerminalThemeBinding {
    pub const fn workspace_palette() -> Self {
        Self {
            source: TerminalThemeSource::WorkspacePalette,
        }
    }
}

impl From<ResolvedTerminalTheme> for TerminalTheme {
    fn from(theme: ResolvedTerminalTheme) -> Self {
        Self {
            foreground: Some(theme.foreground),
            background: Some(theme.background),
            cursor: Some(theme.cursor),
            palette: theme.palette.map(Some),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerminalTheme {
    pub foreground: Option<RgbColor>,
    pub background: Option<RgbColor>,
    pub cursor: Option<RgbColor>,
    pub palette: [Option<RgbColor>; 16],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeAppearance {
    Light,
    Dark,
}

impl ThemeAppearance {
    pub const fn color_scheme_report(self) -> &'static [u8] {
        match self {
            Self::Dark => b"\x1b[?997;1n",
            Self::Light => b"\x1b[?997;2n",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultColorKind {
    Foreground,
    Background,
}

pub const HOST_COLOR_QUERY_SEQUENCE: &str = "\x1b]4;0;?\x07\x1b]4;1;?\x07\x1b]4;2;?\x07\x1b]4;3;?\x07\x1b]4;4;?\x07\x1b]4;5;?\x07\x1b]4;6;?\x07\x1b]4;7;?\x07\x1b]4;8;?\x07\x1b]4;9;?\x07\x1b]4;10;?\x07\x1b]4;11;?\x07\x1b]4;12;?\x07\x1b]4;13;?\x07\x1b]4;14;?\x07\x1b]4;15;?\x07\x1b]10;?\x07\x1b]11;?\x07\x1b]12;?\x07";

impl TerminalTheme {
    pub fn with_color(mut self, kind: DefaultColorKind, color: RgbColor) -> Self {
        match kind {
            DefaultColorKind::Foreground => self.foreground = Some(color),
            DefaultColorKind::Background => self.background = Some(color),
        }
        self
    }

    pub fn with_cursor_color(mut self, color: RgbColor) -> Self {
        self.cursor = Some(color);
        self
    }

    pub fn with_palette_color(mut self, index: u8, color: RgbColor) -> Self {
        if let Some(slot) = self.palette.get_mut(index as usize) {
            *slot = Some(color);
        }
        self
    }

    pub fn is_empty(self) -> bool {
        self.foreground.is_none()
            && self.background.is_none()
            && self.cursor.is_none()
            && self.palette.iter().all(Option::is_none)
    }

    pub fn appearance(self) -> Option<ThemeAppearance> {
        let bg = self.background?;
        let luminance =
            (u32::from(bg.r) * 299 + u32::from(bg.g) * 587 + u32::from(bg.b) * 114) / 1000;
        if luminance >= 128 {
            Some(ThemeAppearance::Light)
        } else {
            Some(ThemeAppearance::Dark)
        }
    }
}

pub fn parse_default_color_response(sequence: &str) -> Option<(DefaultColorKind, RgbColor)> {
    let body = osc_body(sequence)?;
    let (command, value) = body.split_once(';')?;
    let kind = match command {
        "10" => DefaultColorKind::Foreground,
        "11" => DefaultColorKind::Background,
        _ => return None,
    };
    Some((kind, parse_rgb_color(value)?))
}

pub fn parse_palette_color_response(sequence: &str) -> Option<(u8, RgbColor)> {
    let body = osc_body(sequence)?;
    let mut parts = body.split(';');
    if parts.next()? != "4" {
        return None;
    }
    let index = parts.next()?.parse::<u8>().ok()?;
    if index >= 16 {
        return None;
    }
    let color = parse_rgb_color(parts.next()?)?;
    parts.next().is_none().then_some((index, color))
}

pub fn parse_cursor_color_response(sequence: &str) -> Option<RgbColor> {
    let body = osc_body(sequence)?;
    let (command, value) = body.split_once(';')?;
    (command == "12").then(|| parse_rgb_color(value)).flatten()
}

pub fn osc_set_default_color_sequence(kind: DefaultColorKind, color: RgbColor) -> String {
    let command = match kind {
        DefaultColorKind::Foreground => 10,
        DefaultColorKind::Background => 11,
    };
    format!(
        "\x1b]{command};rgb:{:02x}/{:02x}/{:02x}\x1b\\",
        color.r, color.g, color.b
    )
}

pub fn osc_reset_default_color_sequence(kind: DefaultColorKind) -> &'static str {
    match kind {
        DefaultColorKind::Foreground => "\x1b]110\x1b\\",
        DefaultColorKind::Background => "\x1b]111\x1b\\",
    }
}

fn osc_body(sequence: &str) -> Option<&str> {
    let body = sequence.strip_prefix("\x1b]")?;
    body.strip_suffix("\x1b\\")
        .or_else(|| body.strip_suffix('\u{7}'))
}

fn parse_rgb_color(value: &str) -> Option<RgbColor> {
    if let Some(rgb) = value.strip_prefix("rgb:") {
        let mut parts = rgb.split('/');
        let color = RgbColor {
            r: parse_hex_component(parts.next()?)?,
            g: parse_hex_component(parts.next()?)?,
            b: parse_hex_component(parts.next()?)?,
        };
        return parts.next().is_none().then_some(color);
    }

    if let Some(hex) = value.strip_prefix('#') {
        let digits = hex.len() / 3;
        if !matches!(digits, 1..=4) || hex.len() != digits * 3 {
            return None;
        }
        return Some(RgbColor {
            r: parse_hex_component(&hex[..digits])?,
            g: parse_hex_component(&hex[digits..digits * 2])?,
            b: parse_hex_component(&hex[digits * 2..])?,
        });
    }

    None
}

fn parse_hex_component(component: &str) -> Option<u8> {
    if component.is_empty()
        || component.len() > 4
        || !component.chars().all(|ch| ch.is_ascii_hexdigit())
    {
        return None;
    }
    let value = u32::from_str_radix(component, 16).ok()?;
    let max = (1u32 << (component.len() * 4)) - 1;
    Some(((value * 255 + (max / 2)) / max) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_st_terminated_rgb_response() {
        let parsed = parse_default_color_response("\x1b]10;rgb:cccc/dddd/eeee\x1b\\");
        assert_eq!(
            parsed,
            Some((
                DefaultColorKind::Foreground,
                RgbColor {
                    r: 0xcc,
                    g: 0xdd,
                    b: 0xee,
                },
            ))
        );
    }

    #[test]
    fn parses_bel_terminated_hash_response() {
        let parsed = parse_default_color_response("\x1b]11;#123456\u{7}");
        assert_eq!(
            parsed,
            Some((
                DefaultColorKind::Background,
                RgbColor {
                    r: 0x12,
                    g: 0x34,
                    b: 0x56,
                },
            ))
        );
    }

    #[test]
    fn parses_palette_color_response() {
        let parsed = parse_palette_color_response("\x1b]4;3;rgb:aaaa/bbbb/cccc\x1b\\");
        assert_eq!(
            parsed,
            Some((
                3,
                RgbColor {
                    r: 0xaa,
                    g: 0xbb,
                    b: 0xcc,
                },
            ))
        );
    }

    #[test]
    fn parses_cursor_color_response() {
        let parsed = parse_cursor_color_response("\x1b]12;#123456\u{7}");
        assert_eq!(
            parsed,
            Some(RgbColor {
                r: 0x12,
                g: 0x34,
                b: 0x56,
            })
        );
    }

    #[test]
    fn scales_short_hex_components() {
        assert_eq!(parse_hex_component("f"), Some(255));
        assert_eq!(parse_hex_component("80"), Some(128));
        assert_eq!(parse_hex_component("800"), Some(128));
        assert_eq!(parse_hex_component("8000"), Some(128));
    }

    #[test]
    fn terminal_theme_appearance_uses_background_luminance() {
        assert_eq!(
            TerminalTheme::default()
                .with_color(
                    DefaultColorKind::Background,
                    RgbColor {
                        r: 245,
                        g: 245,
                        b: 245,
                    },
                )
                .appearance(),
            Some(ThemeAppearance::Light)
        );
        assert_eq!(
            TerminalTheme::default()
                .with_color(
                    DefaultColorKind::Background,
                    RgbColor {
                        r: 20,
                        g: 20,
                        b: 20,
                    },
                )
                .appearance(),
            Some(ThemeAppearance::Dark)
        );
    }
}
