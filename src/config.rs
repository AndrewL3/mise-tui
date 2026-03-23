use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use serde::Deserialize;
use thiserror::Error;

use crate::theme::ThemeConfig;

pub const DEFAULT_CONFIG: &str = include_str!("../config/default.toml");

fn default_true() -> bool {
    true
}

// ─── Config structs ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub general: GeneralConfig,
    pub layout: LayoutConfig,
    pub theme: ThemeConfig,
    #[serde(default)]
    pub widgets: HashMap<String, toml::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralConfig {
    pub tick_rate: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutConfig {
    pub columns: usize,
    pub rows: usize,
    #[serde(default)]
    pub column_widths: Option<Vec<String>>,
    #[serde(default)]
    pub row_heights: Option<Vec<String>>,
    #[serde(default = "default_true")]
    pub header: bool,
    #[serde(default = "default_true")]
    pub footer: bool,
    pub panels: Vec<PanelConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelConfig {
    pub row: usize,
    pub col: usize,
    #[serde(rename = "type")]
    pub widget_type: String,
    pub id: Option<String>,
}

impl PanelConfig {
    pub fn instance_id(&self) -> &str {
        self.id.as_deref().unwrap_or(&self.widget_type)
    }
}

// ─── Error / Warning types ───────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid grid dimensions: rows={rows}, columns={columns} (both must be > 0)")]
    InvalidGridDimensions { rows: usize, columns: usize },

    #[error("panel at ({row}, {col}) is out of bounds for {rows}x{cols} grid")]
    PanelOutOfBounds {
        row: usize,
        col: usize,
        rows: usize,
        cols: usize,
    },

    #[error("duplicate panel at ({row}, {col})")]
    DuplicatePosition { row: usize, col: usize },

    #[error("unknown widget type '{widget_type}'")]
    UnknownWidgetType { widget_type: String },

    #[error("duplicate instance ID '{id}'")]
    DuplicateInstanceId { id: String },

    #[error("column_widths length ({got}) does not match columns ({expected})")]
    ColumnWidthsMismatch { expected: usize, got: usize },

    #[error("row_heights length ({got}) does not match rows ({expected})")]
    RowHeightsMismatch { expected: usize, got: usize },

    #[error("percentages sum to {sum}%, expected 100%")]
    PercentageSumError { sum: u16 },

    #[error("invalid percentage value '{value}'")]
    InvalidPercentage { value: String },

    #[error("orphan widget section '[widgets.{id}]' does not match any panel")]
    OrphanWidgetSection { id: String },

    #[error("invalid instance ID '{id}': must match [A-Za-z0-9_-]+")]
    InvalidInstanceId { id: String },
}

#[derive(Debug)]
pub enum ConfigWarning {
    EmptyPanels,
}

#[derive(Debug)]
pub struct ValidationResult {
    pub errors: Vec<ConfigError>,
    pub warnings: Vec<ConfigWarning>,
}

impl ValidationResult {
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

// ─── Config impl ─────────────────────────────────────────────────────────────

impl Config {
    /// Deserialize a TOML string into a `Config`.
    pub fn parse(toml_str: &str) -> Result<Self> {
        let config: Config =
            toml::from_str(toml_str).map_err(|e| eyre!("failed to parse config: {e}"))?;
        Ok(config)
    }

    /// Load config from a given path, or from `~/.config/mise-tui/config.toml`
    /// if `None` is passed, writing the embedded default if the file does not
    /// yet exist.
    pub fn load(path: Option<&std::path::Path>) -> Result<Self> {
        match path {
            Some(p) => {
                let contents = std::fs::read_to_string(p)
                    .map_err(|e| eyre!("failed to read config file {}: {e}", p.display()))?;
                Self::parse(&contents)
            }
            None => {
                let p = config_path()?;
                if !p.exists() {
                    if let Some(parent) = p.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&p, DEFAULT_CONFIG)?;
                }
                let contents = std::fs::read_to_string(&p)
                    .map_err(|e| eyre!("failed to read config file {}: {e}", p.display()))?;
                Self::parse(&contents)
            }
        }
    }

    /// Validate the parsed config against a registry-provided type predicate.
    ///
    /// All errors are collected; the caller should check `ValidationResult::has_errors()`.
    pub fn validate(&self, is_known_type: impl Fn(&str) -> bool) -> ValidationResult {
        let mut errors: Vec<ConfigError> = Vec::new();
        let mut warnings: Vec<ConfigWarning> = Vec::new();

        let layout = &self.layout;

        // ── 1. Grid dimensions must be > 0 ──────────────────────────────────
        if layout.rows == 0 || layout.columns == 0 {
            errors.push(ConfigError::InvalidGridDimensions {
                rows: layout.rows,
                columns: layout.columns,
            });
        }

        // ── 2. Panel-level checks ────────────────────────────────────────────
        let mut seen_positions: HashSet<(usize, usize)> = HashSet::new();
        let mut seen_ids: HashSet<String> = HashSet::new();
        let mut panel_ids: HashSet<String> = HashSet::new();

        for panel in &layout.panels {
            // Out-of-bounds (only meaningful when grid is nonzero)
            if layout.rows > 0
                && layout.columns > 0
                && (panel.row >= layout.rows || panel.col >= layout.columns)
            {
                errors.push(ConfigError::PanelOutOfBounds {
                    row: panel.row,
                    col: panel.col,
                    rows: layout.rows,
                    cols: layout.columns,
                });
            }

            // Duplicate position
            let pos = (panel.row, panel.col);
            if !seen_positions.insert(pos) {
                errors.push(ConfigError::DuplicatePosition {
                    row: panel.row,
                    col: panel.col,
                });
            }

            // Unknown widget type
            if !is_known_type(&panel.widget_type) {
                errors.push(ConfigError::UnknownWidgetType {
                    widget_type: panel.widget_type.clone(),
                });
            }

            // Instance ID validation (format)
            let id = panel.instance_id();
            if !is_valid_instance_id(id) {
                errors.push(ConfigError::InvalidInstanceId { id: id.to_string() });
            }

            // Duplicate instance ID
            if !seen_ids.insert(id.to_string()) {
                errors.push(ConfigError::DuplicateInstanceId { id: id.to_string() });
            }

            panel_ids.insert(id.to_string());
        }

        // ── 3. Empty panels warning ──────────────────────────────────────────
        if layout.panels.is_empty() {
            warnings.push(ConfigWarning::EmptyPanels);
        }

        // ── 4. column_widths length and percentage sums ──────────────────────
        if let Some(widths) = &layout.column_widths {
            if widths.len() != layout.columns {
                errors.push(ConfigError::ColumnWidthsMismatch {
                    expected: layout.columns,
                    got: widths.len(),
                });
            } else {
                // Only check sum when length is correct
                match parse_percentage_sum(widths) {
                    Ok(sum) if sum != 100 => {
                        errors.push(ConfigError::PercentageSumError { sum });
                    }
                    Err(bad) => {
                        errors.push(ConfigError::InvalidPercentage { value: bad });
                    }
                    _ => {}
                }
            }
        }

        // ── 5. row_heights length and percentage sums ────────────────────────
        if let Some(heights) = &layout.row_heights {
            if heights.len() != layout.rows {
                errors.push(ConfigError::RowHeightsMismatch {
                    expected: layout.rows,
                    got: heights.len(),
                });
            } else {
                match parse_percentage_sum(heights) {
                    Ok(sum) if sum != 100 => {
                        errors.push(ConfigError::PercentageSumError { sum });
                    }
                    Err(bad) => {
                        errors.push(ConfigError::InvalidPercentage { value: bad });
                    }
                    _ => {}
                }
            }
        }

        // ── 6. Orphan widget sections ────────────────────────────────────────
        for widget_id in self.widgets.keys() {
            if !panel_ids.contains(widget_id.as_str()) {
                errors.push(ConfigError::OrphanWidgetSection {
                    id: widget_id.clone(),
                });
            }
        }

        ValidationResult { errors, warnings }
    }
}

// ─── Helper functions ─────────────────────────────────────────────────────────

/// Returns the path to the user config file.
pub fn config_path() -> Result<PathBuf> {
    let dir = dirs::config_dir().ok_or_else(|| eyre!("could not determine config directory"))?;
    Ok(dir.join("mise-tui").join("config.toml"))
}

/// Returns `true` if the string matches `[A-Za-z0-9_-]+` (non-empty).
fn is_valid_instance_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Parse a slice of percentage strings (e.g. `["60%", "40%"]`) and return
/// their sum as `u16`. Returns `Err(bad_value)` for the first unparseable entry.
fn parse_percentage_sum(values: &[String]) -> std::result::Result<u16, String> {
    let mut sum: u16 = 0;
    for v in values {
        let trimmed = v.trim();
        let num_str = trimmed.strip_suffix('%').ok_or_else(|| v.clone())?;
        let n: u16 = num_str.parse().map_err(|_| v.clone())?;
        sum = sum.saturating_add(n);
    }
    Ok(sum)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Phase 1: Parsing tests ────────────────────────────────────────────────

    #[test]
    fn parse_default_config() {
        let config = Config::parse(DEFAULT_CONFIG).unwrap();
        assert_eq!(config.general.tick_rate, 250);
        assert_eq!(config.layout.columns, 3);
        assert_eq!(config.layout.rows, 2);
        assert_eq!(config.layout.panels.len(), 6);
        assert!(config.layout.header);
        assert!(config.layout.footer);
    }

    #[test]
    fn parse_minimal_config() {
        let toml = r#"
            [general]
            tick_rate = 100
            [layout]
            columns = 1
            rows = 1
            panels = []
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.general.tick_rate, 100);
        assert_eq!(config.layout.panels.len(), 0);
        assert!(config.layout.header);
    }

    #[test]
    fn parse_panel_instance_id_defaults_to_type() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 1
            rows = 1
            [[layout.panels]]
            row = 0
            col = 0
            type = "cpu"
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.layout.panels[0].instance_id(), "cpu");
    }

    #[test]
    fn parse_panel_explicit_id() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 1
            rows = 1
            [[layout.panels]]
            row = 0
            col = 0
            type = "network"
            id = "net-wifi"
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.layout.panels[0].instance_id(), "net-wifi");
    }

    #[test]
    fn parse_unknown_top_level_field_errors() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 1
            rows = 1
            panels = []
            [theme]
            [bogus]
            foo = "bar"
        "#;
        assert!(Config::parse(toml).is_err());
    }

    #[test]
    fn parse_unknown_panel_field_errors() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 1
            rows = 1
            [[layout.panels]]
            row = 0
            col = 0
            type = "cpu"
            col_span = 2
            [theme]
        "#;
        assert!(Config::parse(toml).is_err());
    }

    #[test]
    fn parse_malformed_toml_errors() {
        assert!(Config::parse("this is not valid toml [[[").is_err());
    }

    #[test]
    fn parse_widget_sections_as_raw_values() {
        let config = Config::parse(DEFAULT_CONFIG).unwrap();
        assert!(config.widgets.contains_key("cpu"));
        assert!(config.widgets.contains_key("memory"));
        assert!(config.widgets.contains_key("network"));
        assert!(config.widgets.contains_key("temps"));
    }

    #[test]
    fn parse_explicit_sizing() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 2
            rows = 2
            column_widths = ["60%", "40%"]
            row_heights = ["70%", "30%"]
            panels = []
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let widths = config.layout.column_widths.unwrap();
        assert_eq!(widths, vec!["60%", "40%"]);
        let heights = config.layout.row_heights.unwrap();
        assert_eq!(heights, vec!["70%", "30%"]);
    }

    // ── Phase 2: Validation tests ─────────────────────────────────────────────

    fn known_type(t: &str) -> bool {
        matches!(
            t,
            "cpu" | "memory" | "network" | "temps" | "disk" | "processes"
        )
    }

    #[test]
    fn validate_default_config_passes() {
        let config = Config::parse(DEFAULT_CONFIG).unwrap();
        let result = config.validate(known_type);
        assert!(!result.has_errors(), "errors: {:?}", result.errors);
    }

    #[test]
    fn validate_zero_rows_errors() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 2
            rows = 0
            panels = []
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ConfigError::InvalidGridDimensions { .. }))
        );
    }

    #[test]
    fn validate_panel_out_of_bounds_errors() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 2
            rows = 2
            [[layout.panels]]
            row = 5
            col = 0
            type = "cpu"
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ConfigError::PanelOutOfBounds { .. }))
        );
    }

    #[test]
    fn validate_duplicate_position_errors() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 2
            rows = 2
            [[layout.panels]]
            row = 0
            col = 0
            type = "cpu"
            [[layout.panels]]
            row = 0
            col = 0
            type = "memory"
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ConfigError::DuplicatePosition { .. }))
        );
    }

    #[test]
    fn validate_unknown_widget_type_errors() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 1
            rows = 1
            [[layout.panels]]
            row = 0
            col = 0
            type = "doesnt_exist"
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ConfigError::UnknownWidgetType { .. }))
        );
    }

    #[test]
    fn validate_duplicate_implicit_ids_errors() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 2
            rows = 1
            [[layout.panels]]
            row = 0
            col = 0
            type = "cpu"
            [[layout.panels]]
            row = 0
            col = 1
            type = "cpu"
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ConfigError::DuplicateInstanceId { .. }))
        );
    }

    #[test]
    fn validate_duplicate_explicit_ids_errors() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 2
            rows = 1
            [[layout.panels]]
            row = 0
            col = 0
            type = "cpu"
            id = "my-widget"
            [[layout.panels]]
            row = 0
            col = 1
            type = "memory"
            id = "my-widget"
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ConfigError::DuplicateInstanceId { .. }))
        );
    }

    #[test]
    fn validate_orphan_widget_section_errors() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 1
            rows = 1
            [[layout.panels]]
            row = 0
            col = 0
            type = "cpu"
            [theme]
            [widgets.typo_widget]
            mode = "bars"
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ConfigError::OrphanWidgetSection { .. }))
        );
    }

    #[test]
    fn validate_column_widths_mismatch_errors() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 2
            rows = 1
            column_widths = ["50%"]
            panels = []
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ConfigError::ColumnWidthsMismatch { .. }))
        );
    }

    #[test]
    fn validate_percentages_not_100_errors() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 2
            rows = 1
            column_widths = ["30%", "30%"]
            panels = []
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ConfigError::PercentageSumError { .. }))
        );
    }

    #[test]
    fn validate_invalid_instance_id_errors() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 1
            rows = 1
            [[layout.panels]]
            row = 0
            col = 0
            type = "cpu"
            id = "has spaces"
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ConfigError::InvalidInstanceId { .. }))
        );
    }

    // ── Phase 3: Config::load tests ────────────────────────────────────────────

    #[test]
    fn load_explicit_path_valid() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config/default.toml");
        let config = Config::load(Some(&path)).unwrap();
        assert_eq!(config.general.tick_rate, 250);
    }

    #[test]
    fn load_explicit_path_missing_errors() {
        let path = std::path::Path::new("/tmp/nonexistent-mise-tui-config.toml");
        assert!(Config::load(Some(path)).is_err());
    }

    #[test]
    fn load_none_uses_default_path() {
        let _ = Config::load(None);
    }

    // ── Validation tests (continued) ────────────────────────────────────────

    #[test]
    fn validate_empty_panels_is_warning_not_error() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 1
            rows = 1
            panels = []
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(!result.has_errors());
        assert!(
            result
                .warnings
                .iter()
                .any(|w| matches!(w, ConfigWarning::EmptyPanels))
        );
    }
}
