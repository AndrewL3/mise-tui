use color_eyre::{Result, eyre::eyre};
use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

/// Raw TOML-deserialized theme configuration. All fields are optional;
/// absent fields fall back to defaults in `Theme::from_config`.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ThemeConfig {
    pub border: Option<String>,
    pub border_focused: Option<String>,
    pub border_interact: Option<String>,
    pub title: Option<String>,
    pub label: Option<String>,
    pub value: Option<String>,
    pub gauge_fill: Option<String>,
    pub gauge_bg: Option<String>,
    pub sparkline: Option<String>,
    pub ok: Option<String>,
    pub warning: Option<String>,
    pub critical: Option<String>,
    pub header_bg: Option<String>,
    pub header_fg: Option<String>,
    pub error_fg: Option<String>,
}

/// Resolved theme holding ratatui `Style` and `Color` values ready for use
/// by widgets and the app shell.
#[derive(Debug, Clone)]
pub struct Theme {
    pub border: Style,
    pub border_focused: Style,
    pub border_interact: Style,
    pub title: Style,
    pub label: Style,
    pub value: Style,
    pub gauge_fill: Style,
    pub gauge_bg: Style,
    pub sparkline: Style,
    pub ok: Style,
    pub warning: Style,
    pub critical: Style,
    pub header_bg: Color,
    pub header_fg: Color,
    pub error_fg: Color,
}

/// Parse a color string into a ratatui `Color`.
///
/// Supported formats:
/// - Named: `"cyan"`, `"dark_gray"`, `"light_red"`, …
/// - 256-palette: `"color(123)"` → `Color::Indexed(123)`
/// - Hex RGB: `"#ff5500"` → `Color::Rgb(255, 85, 0)`
pub fn parse_color(s: &str) -> Result<Color> {
    let s = s.trim();

    // Hex: #rrggbb
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() != 6 {
            return Err(eyre!("invalid hex color: '#{hex}' (expected #rrggbb)"));
        }
        let r =
            u8::from_str_radix(&hex[0..2], 16).map_err(|_| eyre!("invalid hex color: '#{hex}'"))?;
        let g =
            u8::from_str_radix(&hex[2..4], 16).map_err(|_| eyre!("invalid hex color: '#{hex}'"))?;
        let b =
            u8::from_str_radix(&hex[4..6], 16).map_err(|_| eyre!("invalid hex color: '#{hex}'"))?;
        return Ok(Color::Rgb(r, g, b));
    }

    // 256-palette: color(N)
    if let Some(inner) = s.strip_prefix("color(").and_then(|s| s.strip_suffix(')')) {
        let index: u8 = inner
            .parse()
            .map_err(|_| eyre!("invalid 256-palette color index: '{inner}'"))?;
        return Ok(Color::Indexed(index));
    }

    // Named colors
    let color = match s {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" => Color::Gray,
        "dark_gray" => Color::DarkGray,
        "light_red" => Color::LightRed,
        "light_green" => Color::LightGreen,
        "light_yellow" => Color::LightYellow,
        "light_blue" => Color::LightBlue,
        "light_magenta" => Color::LightMagenta,
        "light_cyan" => Color::LightCyan,
        other => return Err(eyre!("unknown color: '{other}'")),
    };

    Ok(color)
}

/// Parse a style string into a ratatui `Style`.
///
/// A style string is space-separated tokens. Recognized modifier keywords
/// (`bold`, `italic`, `underline`, `dim`) accumulate modifiers. The first
/// non-modifier token is interpreted as a color (any format accepted by
/// `parse_color`). Multiple modifiers before the color are all applied.
///
/// Examples: `"cyan"`, `"bold cyan"`, `"bold italic white"`, `"dim #aabbcc"`
pub fn parse_style(s: &str) -> Result<Style> {
    let mut style = Style::new();
    let mut color_set = false;

    for token in s.split_whitespace() {
        match token {
            "bold" => {
                style = style.add_modifier(Modifier::BOLD);
            }
            "italic" => {
                style = style.add_modifier(Modifier::ITALIC);
            }
            "underline" => {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            "dim" => {
                style = style.add_modifier(Modifier::DIM);
            }
            other => {
                if color_set {
                    return Err(eyre!(
                        "unexpected token after color in style string: '{other}'"
                    ));
                }
                let color = parse_color(other)?;
                style = style.fg(color);
                color_set = true;
            }
        }
    }

    Ok(style)
}

impl Theme {
    /// Build a `Theme` from optional config overrides, falling back to
    /// `Theme::default_theme()` for any absent field.
    pub fn from_config(config: &ThemeConfig) -> Result<Theme> {
        let defaults = Theme::default_theme();

        macro_rules! resolve_style {
            ($field:ident) => {
                match &config.$field {
                    Some(s) => parse_style(s)?,
                    None => defaults.$field,
                }
            };
        }

        macro_rules! resolve_color {
            ($field:ident) => {
                match &config.$field {
                    Some(s) => parse_color(s)?,
                    None => defaults.$field,
                }
            };
        }

        Ok(Theme {
            border: resolve_style!(border),
            border_focused: resolve_style!(border_focused),
            border_interact: resolve_style!(border_interact),
            title: resolve_style!(title),
            label: resolve_style!(label),
            value: resolve_style!(value),
            gauge_fill: resolve_style!(gauge_fill),
            gauge_bg: resolve_style!(gauge_bg),
            sparkline: resolve_style!(sparkline),
            ok: resolve_style!(ok),
            warning: resolve_style!(warning),
            critical: resolve_style!(critical),
            header_bg: resolve_color!(header_bg),
            header_fg: resolve_color!(header_fg),
            error_fg: resolve_color!(error_fg),
        })
    }

    /// Hardcoded defaults matching `config/default.toml`.
    pub fn default_theme() -> Self {
        Theme {
            border: Style::new().fg(Color::Gray),
            border_focused: Style::new().fg(Color::Cyan),
            border_interact: Style::new().fg(Color::Yellow),
            title: Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            label: Style::new().fg(Color::Gray),
            value: Style::new().fg(Color::White),
            gauge_fill: Style::new().fg(Color::Green),
            gauge_bg: Style::new().fg(Color::DarkGray),
            sparkline: Style::new().fg(Color::Cyan),
            ok: Style::new().fg(Color::Green),
            warning: Style::new().fg(Color::Yellow),
            critical: Style::new().fg(Color::Red),
            header_bg: Color::DarkGray,
            header_fg: Color::White,
            error_fg: Color::Red,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::default_theme()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_named_color_cyan() {
        assert_eq!(parse_color("cyan").unwrap(), Color::Cyan);
    }

    #[test]
    fn parse_named_color_dark_gray() {
        assert_eq!(parse_color("dark_gray").unwrap(), Color::DarkGray);
    }

    #[test]
    fn parse_bright_variant_light_red() {
        assert_eq!(parse_color("light_red").unwrap(), Color::LightRed);
    }

    #[test]
    fn parse_256_palette() {
        assert_eq!(parse_color("color(123)").unwrap(), Color::Indexed(123));
    }

    #[test]
    fn parse_hex_color() {
        assert_eq!(parse_color("#ff5500").unwrap(), Color::Rgb(255, 85, 0));
    }

    #[test]
    fn parse_hex_color_lowercase() {
        assert_eq!(parse_color("#00aaff").unwrap(), Color::Rgb(0, 170, 255));
    }

    #[test]
    fn parse_invalid_color_errors() {
        assert!(parse_color("not_a_color").is_err());
    }

    #[test]
    fn parse_invalid_hex_errors() {
        assert!(parse_color("#xyz").is_err());
    }

    #[test]
    fn parse_style_plain_color() {
        let style = parse_style("cyan").unwrap();
        assert_eq!(style.fg, Some(Color::Cyan));
    }

    #[test]
    fn parse_style_bold_color() {
        let style = parse_style("bold cyan").unwrap();
        assert_eq!(style.fg, Some(Color::Cyan));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn parse_style_multiple_modifiers() {
        let style = parse_style("bold italic white").unwrap();
        assert_eq!(style.fg, Some(Color::White));
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn parse_style_dim() {
        let style = parse_style("dim gray").unwrap();
        assert_eq!(style.fg, Some(Color::Gray));
        assert!(style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn parse_style_invalid_errors() {
        assert!(parse_style("bold not_a_color").is_err());
    }

    #[test]
    fn theme_from_empty_config_uses_defaults() {
        let config = ThemeConfig {
            border: None,
            border_focused: None,
            border_interact: None,
            title: None,
            label: None,
            value: None,
            gauge_fill: None,
            gauge_bg: None,
            sparkline: None,
            ok: None,
            warning: None,
            critical: None,
            header_bg: None,
            header_fg: None,
            error_fg: None,
        };
        let theme = Theme::from_config(&config).unwrap();
        assert_eq!(theme.border.fg, Some(Color::Gray));
        assert_eq!(theme.border_focused.fg, Some(Color::Cyan));
    }

    #[test]
    fn theme_from_config_overrides() {
        let config = ThemeConfig {
            border: Some("red".to_string()),
            border_focused: Some("bold green".to_string()),
            border_interact: None,
            title: None,
            label: None,
            value: None,
            gauge_fill: None,
            gauge_bg: None,
            sparkline: None,
            ok: None,
            warning: None,
            critical: None,
            header_bg: None,
            header_fg: None,
            error_fg: None,
        };
        let theme = Theme::from_config(&config).unwrap();
        assert_eq!(theme.border.fg, Some(Color::Red));
        assert_eq!(theme.border_focused.fg, Some(Color::Green));
        assert!(theme.border_focused.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn theme_from_config_invalid_color_errors() {
        let config = ThemeConfig {
            border: Some("not_a_color".to_string()),
            border_focused: None,
            border_interact: None,
            title: None,
            label: None,
            value: None,
            gauge_fill: None,
            gauge_bg: None,
            sparkline: None,
            ok: None,
            warning: None,
            critical: None,
            header_bg: None,
            header_fg: None,
            error_fg: None,
        };
        assert!(Theme::from_config(&config).is_err());
    }

    #[test]
    fn theme_default_has_error_fg() {
        let theme = Theme::default();
        assert_eq!(theme.error_fg, Color::Red);
    }

    #[test]
    fn theme_default_has_border_interact() {
        let theme = Theme::default();
        assert_eq!(theme.border_interact.fg, Some(Color::Yellow));
    }

    #[test]
    fn theme_from_config_border_interact_override() {
        let config = ThemeConfig {
            border_interact: Some("bold green".to_string()),
            ..ThemeConfig::default()
        };
        let theme = Theme::from_config(&config).unwrap();
        assert_eq!(theme.border_interact.fg, Some(Color::Green));
        assert!(theme.border_interact.add_modifier.contains(Modifier::BOLD));
    }
}
