//! HADAL visual themes and color depth mapping.
//!
//! Four themes: Abyss (default), HighContrast, AmberCrt, Mono.
//! Each theme defines semantic colors: bg, panel, text, dim, accent, amber, alert.
//! `ColorDepth` maps from CLI flags and config to ratatui color types.

use ratatui::style::Color;

// ---------------------------------------------------------------------
// Theme palette
// ---------------------------------------------------------------------

/// Visual theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Abyss,
    HighContrast,
    AmberCrt,
    Mono,
}

/// Semantic colors used throughout the UI.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Main background.
    pub bg: Color,
    /// Panel background.
    pub panel: Color,
    /// Primary text.
    pub text: Color,
    /// Dim / secondary text.
    pub dim: Color,
    /// Accent (action highlights, generators).
    pub accent: Color,
    /// Amber (carrier, warnings).
    pub amber: Color,
    /// Alert (errors, unavailable).
    pub alert: Color,
}

impl Theme {
    /// Return the palette for this theme.
    pub fn palette(self) -> Palette {
        match self {
            Theme::Abyss => Palette {
                bg: Color::Rgb(0x03, 0x08, 0x0B),
                panel: Color::Rgb(0x07, 0x14, 0x1A),
                text: Color::Rgb(0xD8, 0xF3, 0xEC),
                dim: Color::Rgb(0x2A, 0x4A, 0x52),
                accent: Color::Rgb(0x4D, 0xE1, 0xC1),
                amber: Color::Rgb(0xFF, 0xB4, 0x54),
                alert: Color::Rgb(0xFF, 0x6B, 0x6B),
            },
            Theme::HighContrast => Palette {
                bg: Color::Rgb(0x00, 0x00, 0x00),
                panel: Color::Rgb(0x11, 0x11, 0x11),
                text: Color::Rgb(0xFF, 0xFF, 0xFF),
                dim: Color::Rgb(0x88, 0x88, 0x88),
                accent: Color::Rgb(0x00, 0xFF, 0xFF),
                amber: Color::Rgb(0xFF, 0xFF, 0x00),
                alert: Color::Rgb(0xFF, 0x00, 0x00),
            },
            Theme::AmberCrt => Palette {
                bg: Color::Rgb(0x0A, 0x06, 0x00),
                panel: Color::Rgb(0x1A, 0x10, 0x00),
                text: Color::Rgb(0xFF, 0xCC, 0x66),
                dim: Color::Rgb(0x80, 0x66, 0x33),
                accent: Color::Rgb(0xFF, 0xCC, 0x00),
                amber: Color::Rgb(0xFF, 0xAA, 0x00),
                alert: Color::Rgb(0xFF, 0x44, 0x44),
            },
            Theme::Mono => Palette {
                bg: Color::Rgb(0x00, 0x00, 0x00),
                panel: Color::Rgb(0x11, 0x11, 0x11),
                text: Color::Rgb(0xFF, 0xFF, 0xFF),
                dim: Color::Rgb(0x88, 0x88, 0x88),
                accent: Color::Rgb(0xFF, 0xFF, 0xFF),
                amber: Color::Rgb(0xCC, 0xCC, 0xCC),
                alert: Color::Rgb(0xFF, 0xFF, 0xFF),
            },
        }
    }

    /// Short label used by SYSTEM.
    pub fn label(self) -> &'static str {
        match self {
            Theme::Abyss => "abyss",
            Theme::HighContrast => "high-contrast",
            Theme::AmberCrt => "amber-crt",
            Theme::Mono => "mono",
        }
    }

    /// Cycle to the next theme.
    pub fn next(self) -> Self {
        match self {
            Theme::Abyss => Theme::HighContrast,
            Theme::HighContrast => Theme::AmberCrt,
            Theme::AmberCrt => Theme::Mono,
            Theme::Mono => Theme::Abyss,
        }
    }
}

// ---------------------------------------------------------------------
// Cross-crate conversions
// ---------------------------------------------------------------------

use hadal_core::save::config::{ColorDepth as CoreColorDepth, Theme as CoreTheme};

impl From<CoreTheme> for Theme {
    fn from(value: CoreTheme) -> Self {
        match value {
            CoreTheme::Abyss => Theme::Abyss,
            CoreTheme::HighContrast => Theme::HighContrast,
            CoreTheme::AmberCrt => Theme::AmberCrt,
            CoreTheme::Mono => Theme::Mono,
        }
    }
}

impl From<Theme> for CoreTheme {
    fn from(value: Theme) -> Self {
        match value {
            Theme::Abyss => CoreTheme::Abyss,
            Theme::HighContrast => CoreTheme::HighContrast,
            Theme::AmberCrt => CoreTheme::AmberCrt,
            Theme::Mono => CoreTheme::Mono,
        }
    }
}

impl From<CoreColorDepth> for ColorDepth {
    fn from(value: CoreColorDepth) -> Self {
        match value {
            CoreColorDepth::Auto => ColorDepth::Auto,
            CoreColorDepth::Ansi16 => ColorDepth::Ansi16,
            CoreColorDepth::Ansi256 => ColorDepth::Ansi256,
            CoreColorDepth::TrueColor => ColorDepth::TrueColor,
            CoreColorDepth::None => ColorDepth::None,
        }
    }
}

impl From<ColorDepth> for CoreColorDepth {
    fn from(value: ColorDepth) -> Self {
        match value {
            ColorDepth::Auto => CoreColorDepth::Auto,
            ColorDepth::TrueColor => CoreColorDepth::TrueColor,
            ColorDepth::Ansi256 => CoreColorDepth::Ansi256,
            ColorDepth::Ansi16 => CoreColorDepth::Ansi16,
            ColorDepth::None => CoreColorDepth::None,
        }
    }
}

impl ColorDepth {
    /// Short label used by SYSTEM.
    pub fn label(self) -> &'static str {
        match self {
            ColorDepth::Auto => "auto",
            ColorDepth::TrueColor => "truecolor",
            ColorDepth::Ansi256 => "256",
            ColorDepth::Ansi16 => "16",
            ColorDepth::None => "none",
        }
    }

    /// Cycle to the next color depth.
    pub fn next(self) -> Self {
        match self {
            ColorDepth::Auto => ColorDepth::TrueColor,
            ColorDepth::TrueColor => ColorDepth::Ansi256,
            ColorDepth::Ansi256 => ColorDepth::Ansi16,
            ColorDepth::Ansi16 => ColorDepth::None,
            ColorDepth::None => ColorDepth::Auto,
        }
    }
}

// ---------------------------------------------------------------------
// Color depth
// ---------------------------------------------------------------------

/// Terminal color-depth override, matching [`crate::save::ColorDepth`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorDepth {
    #[default]
    Auto,
    TrueColor,
    Ansi256,
    Ansi16,
    None,
}

/// Convert a semantic color to a ratatui `Color` given the current color depth.
pub fn to_color(depth: ColorDepth, color: Color) -> Color {
    match depth {
        ColorDepth::TrueColor => color,
        ColorDepth::Ansi256 => to_ansi256(color),
        ColorDepth::Ansi16 => to_ansi16(color),
        ColorDepth::None => Color::Reset,
        ColorDepth::Auto => color,
    }
}

fn to_ansi256(c: Color) -> Color {
    match c {
        Color::Rgb(r, g, b) => {
            if r == g && g == b {
                // 24-step grayscale ramp (232-255).
                let index = 232 + ((u16::from(r) + 8) / 10).min(23);
                return Color::Indexed(index as u8);
            }
            let rr = (u16::from(r) + 38) / 51;
            let gg = (u16::from(g) + 38) / 51;
            let bb = (u16::from(b) + 38) / 51;
            let index = 16 + rr.clamp(0, 5) * 36 + gg.clamp(0, 5) * 6 + bb.clamp(0, 5);
            Color::Indexed(index as u8)
        }
        _ => c,
    }
}

fn to_ansi16(c: Color) -> Color {
    match c {
        Color::Rgb(r, g, b) if r < 0x80 && g < 0x80 && b < 0x80 => Color::Black,
        Color::Rgb(r, g, b) if r < 0x80 && g < 0x80 && b >= 0x80 => Color::Blue,
        Color::Rgb(r, g, b) if r < 0x80 && g >= 0x80 && b < 0x80 => Color::Green,
        Color::Rgb(r, g, b) if r < 0x80 && g >= 0x80 && b >= 0x80 => Color::Cyan,
        Color::Rgb(r, g, b) if r >= 0x80 && g < 0x80 && b < 0x80 => Color::Red,
        Color::Rgb(r, g, b) if r >= 0x80 && g < 0x80 && b >= 0x80 => Color::Magenta,
        Color::Rgb(r, g, b) if r >= 0x80 && g >= 0x80 && b < 0x80 => Color::Yellow,
        Color::Rgb(r, g, b) if r >= 0x80 && g >= 0x80 && b >= 0x80 => Color::White,
        _ => Color::Gray,
    }
}

impl Palette {
    /// Build a palette with `depth` applied to every semantic color.
    pub fn resolve(theme: Theme, depth: ColorDepth) -> Self {
        let p = theme.palette();
        Self {
            bg: to_color(depth, p.bg),
            panel: to_color(depth, p.panel),
            text: to_color(depth, p.text),
            dim: to_color(depth, p.dim),
            accent: to_color(depth, p.accent),
            amber: to_color(depth, p.amber),
            alert: to_color(depth, p.alert),
        }
    }
}

// ---------------------------------------------------------------------
// WCAG contrast tests
// ---------------------------------------------------------------------

/// Relative luminance of an RGB color (sRGB).
fn luminance(r: u8, g: u8, b: u8) -> f64 {
    let to_linear = |v: u8| {
        let s = v as f64 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * to_linear(r) + 0.7152 * to_linear(g) + 0.0722 * to_linear(b)
}

/// Contrast ratio between two colors.
fn contrast_ratio(a: Color, b: Color) -> f64 {
    let lum = |c: Color| match c {
        Color::Rgb(r, g, b) => luminance(r, g, b),
        Color::Indexed(idx) => {
            // Approximate indexed colors with their RGB values.
            if idx < 16 {
                // Standard 16 colors.
                match idx {
                    0 => luminance(0, 0, 0),
                    1 => luminance(0, 0, 0x80),
                    2 => luminance(0, 0x80, 0),
                    3 => luminance(0, 0x80, 0x80),
                    4 => luminance(0x80, 0, 0),
                    5 => luminance(0x80, 0, 0x80),
                    6 => luminance(0x80, 0x80, 0),
                    7 => luminance(0xC0, 0xC0, 0xC0),
                    8 => luminance(0x80, 0x80, 0x80),
                    9 => luminance(0x00, 0x00, 0xFF),
                    10 => luminance(0x00, 0xFF, 0x00),
                    11 => luminance(0x00, 0xFF, 0xFF),
                    12 => luminance(0xFF, 0x00, 0x00),
                    13 => luminance(0xFF, 0x00, 0xFF),
                    14 => luminance(0xFF, 0xFF, 0x00),
                    15 => luminance(0xFF, 0xFF, 0xFF),
                    _ => 0.5,
                }
            } else {
                // Map to nearest RGB.
                let i = (idx - 16) as usize;
                if i < 216 {
                    let r = (i / 36) % 6;
                    let g = (i / 6) % 6;
                    let b = i % 6;
                    let rr = if r == 0 { 0 } else { 55 + r * 45 };
                    let gg = if g == 0 { 0 } else { 55 + g * 45 };
                    let bb = if b == 0 { 0 } else { 55 + b * 45 };
                    luminance(rr as u8, gg as u8, bb as u8)
                } else {
                    let gray = 8 + (i - 216) * 10;
                    luminance(gray as u8, gray as u8, gray as u8)
                }
            }
        }
        _ => 0.5,
    };
    let l1 = lum(a);
    let l2 = lum(b);
    let lighter = l1.max(l2);
    let darker = l1.min(l2);
    (lighter + 0.05) / (darker + 0.05)
}

/// Check that body text on background meets WCAG AA (4.5:1).
fn check_body_text(palette: &Palette) -> bool {
    contrast_ratio(palette.text, palette.bg) >= 4.5
}

/// Check that large numerals on background meet WCAG AA large text (3:1).
fn check_large_numerals(palette: &Palette) -> bool {
    contrast_ratio(palette.accent, palette.bg) >= 3.0
        && contrast_ratio(palette.amber, palette.bg) >= 3.0
        && contrast_ratio(palette.alert, palette.bg) >= 3.0
}

/// Verify all WCAG contrast requirements for a theme.
///
/// Body text on bg ≥ 4.5:1, large numerals (accent, amber, alert) on bg ≥ 3:1.
pub fn validate_contrast(theme: Theme) -> bool {
    let p = theme.palette();
    check_body_text(&p) && check_large_numerals(&p)
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_themes_pass_wcag() {
        for theme in [
            Theme::Abyss,
            Theme::HighContrast,
            Theme::AmberCrt,
            Theme::Mono,
        ] {
            assert!(
                validate_contrast(theme),
                "theme {:?} fails WCAG contrast checks",
                theme
            );
        }
    }

    #[test]
    fn contrast_ratio_commutative() {
        let a = Color::Rgb(0xD8, 0xF3, 0xEC);
        let b = Color::Rgb(0x03, 0x08, 0x0B);
        assert!((contrast_ratio(a, b) - contrast_ratio(b, a)).abs() < 0.001);
    }

    #[test]
    fn theme_and_color_cycles() {
        assert_eq!(Theme::Abyss.next(), Theme::HighContrast);
        assert_eq!(Theme::HighContrast.next(), Theme::AmberCrt);
        assert_eq!(Theme::AmberCrt.next(), Theme::Mono);
        assert_eq!(Theme::Mono.next(), Theme::Abyss);

        assert_eq!(ColorDepth::Auto.next(), ColorDepth::TrueColor);
        assert_eq!(ColorDepth::TrueColor.next(), ColorDepth::Ansi256);
        assert_eq!(ColorDepth::Ansi256.next(), ColorDepth::Ansi16);
        assert_eq!(ColorDepth::Ansi16.next(), ColorDepth::None);
        assert_eq!(ColorDepth::None.next(), ColorDepth::Auto);

        assert_eq!(Theme::Mono.label(), "mono");
        assert_eq!(ColorDepth::None.label(), "none");
    }

    #[test]
    fn color_depth_mapping() {
        let rgb = Color::Rgb(0x4D, 0xE1, 0xC1);
        assert_eq!(to_color(ColorDepth::TrueColor, rgb), rgb);
        assert_eq!(to_color(ColorDepth::None, rgb), Color::Reset);
        assert!(matches!(
            to_color(ColorDepth::Ansi256, rgb),
            Color::Indexed(_)
        ));
        for depth in [ColorDepth::Ansi16, ColorDepth::Ansi256] {
            assert_ne!(to_color(depth, rgb), rgb);
        }
    }

    #[test]
    fn core_config_roundtrip() {
        for theme in [
            Theme::Abyss,
            Theme::HighContrast,
            Theme::AmberCrt,
            Theme::Mono,
        ] {
            assert_eq!(Theme::from(CoreTheme::from(theme)), theme);
        }
        for depth in [
            ColorDepth::Auto,
            ColorDepth::TrueColor,
            ColorDepth::Ansi256,
            ColorDepth::Ansi16,
            ColorDepth::None,
        ] {
            assert_eq!(ColorDepth::from(CoreColorDepth::from(depth)), depth);
        }
    }
}
