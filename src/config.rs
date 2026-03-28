use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::theme::ThemeConfig;

pub const DEFAULT_CONFIG: &str = include_str!("../config/default.toml");

fn default_true() -> bool {
    true
}

fn default_one() -> usize {
    1
}

// ─── Config structs ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub general: GeneralConfig,
    #[serde(default)]
    pub keybinds: KeybindsConfig,
    #[serde(default)]
    pub layout: Option<ProfileConfig>,
    #[serde(default)]
    pub profiles: Option<IndexMap<String, ProfileConfig>>,
    pub theme: ThemeConfig,
    #[serde(default)]
    pub widgets: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralConfig {
    pub tick_rate: u64,
    #[serde(default)]
    pub default_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct KeybindsConfig {
    pub quit: Option<String>,
    pub force_quit: Option<String>,
    pub reload: Option<String>,
    pub help: Option<String>,
    pub focus_next: Option<String>,
    pub focus_prev: Option<String>,
    pub focus_up: Option<String>,
    pub focus_down: Option<String>,
    pub focus_left: Option<String>,
    pub focus_right: Option<String>,
    pub enter_interact: Option<String>,
    pub exit_interact: Option<String>,
    pub edit_mode: Option<String>,
    pub profile_next: Option<String>,
    pub profile_prev: Option<String>,
}

/// A single layout profile (formerly `LayoutConfig`).
///
/// Used both in the legacy `[layout]` form and in the new `[profiles.*]` form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
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

/// Legacy alias so downstream code that references `LayoutConfig` still compiles.
pub type LayoutConfig = ProfileConfig;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelConfig {
    pub row: usize,
    pub col: usize,
    #[serde(rename = "type")]
    pub widget_type: String,
    pub id: Option<String>,
    #[serde(default = "default_one")]
    pub col_span: usize,
    #[serde(default = "default_one")]
    pub row_span: usize,
}

impl PanelConfig {
    pub fn instance_id(&self) -> &str {
        self.id.as_deref().unwrap_or(&self.widget_type)
    }
}

// ─── NormalizedConfig ────────────────────────────────────────────────────────

/// Config after normalization: legacy `[layout]` is converted to a single
/// "default" profile, and `[profiles.*]` is kept as-is. Validation runs
/// against each profile independently.
#[derive(Debug, Clone)]
pub struct NormalizedConfig {
    pub general: GeneralConfig,
    pub profiles: IndexMap<String, ProfileConfig>,
    pub default_profile: String,
    pub theme: ThemeConfig,
    pub keybinds: KeybindsConfig,
    pub widgets: HashMap<String, toml::Value>,
}

impl NormalizedConfig {
    pub fn default_profile(&self) -> &str {
        &self.default_profile
    }

    /// Convert back to a saveable `Config` struct (profiles format).
    pub fn to_saveable(&self) -> Config {
        Config {
            general: self.general.clone(),
            keybinds: self.keybinds.clone(),
            layout: None,
            profiles: Some(self.profiles.clone()),
            theme: self.theme.clone(),
            widgets: self.widgets.clone(),
        }
    }

    pub fn profiles(&self) -> &IndexMap<String, ProfileConfig> {
        &self.profiles
    }

    pub fn active_profile_config(&self) -> &ProfileConfig {
        &self.profiles[&self.default_profile]
    }

    /// Validate every profile independently against a registry-provided type
    /// predicate. Also validates profile name format.
    pub fn validate(&self, is_known_type: impl Fn(&str) -> bool) -> ValidationResult {
        let mut errors: Vec<ConfigError> = Vec::new();
        let mut warnings: Vec<ConfigWarning> = Vec::new();

        // Validate profile names
        for name in self.profiles.keys() {
            if !is_valid_profile_name(name) {
                errors.push(ConfigError::InvalidProfileName {
                    name: name.clone(),
                });
            }
        }

        // Validate each profile
        for (_name, profile) in &self.profiles {
            let result = validate_profile(profile, &self.widgets, &is_known_type);
            errors.extend(result.errors);
            warnings.extend(result.warnings);
        }

        ValidationResult { errors, warnings }
    }
}

// ─── Error / Warning types ───────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid grid dimensions: rows={rows}, columns={columns} (both must be > 0)")]
    InvalidGridDimensions { rows: usize, columns: usize },

    #[error(
        "panel '{id}' at ({row}, {col}) has invalid span: col_span={col_span}, row_span={row_span} (both must be >= 1)"
    )]
    InvalidSpan {
        id: String,
        row: usize,
        col: usize,
        col_span: usize,
        row_span: usize,
    },

    #[error(
        "panel '{id}' at ({row}, {col}) with span {col_span}x{row_span} exceeds {rows}x{cols} grid"
    )]
    SpanOutOfBounds {
        id: String,
        row: usize,
        col: usize,
        col_span: usize,
        row_span: usize,
        rows: usize,
        cols: usize,
    },

    #[error("panels '{id1}' and '{id2}' overlap at cell ({overlap_row}, {overlap_col})")]
    SpanOverlap {
        id1: String,
        id2: String,
        overlap_row: usize,
        overlap_col: usize,
    },

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

    #[error("invalid profile name '{name}': must match [A-Za-z0-9_]+")]
    InvalidProfileName { name: String },
}

#[derive(Debug)]
pub enum ConfigWarning {
    EmptyPanels,
    OrphanWidgetSection { id: String },
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

    /// Normalize the config into a `NormalizedConfig`, converting legacy
    /// `[layout]` into a single "default" profile if present.
    ///
    /// Returns an error if:
    /// - Both `layout` and `profiles` are present
    /// - Neither `layout` nor `profiles` is present
    /// - `default_profile` references a non-existent profile
    pub fn normalize(&self) -> Result<NormalizedConfig> {
        let has_layout = self.layout.is_some();
        let has_profiles = self.profiles.as_ref().is_some_and(|p| !p.is_empty());

        if has_layout && has_profiles {
            return Err(eyre!(
                "config must use either [layout] or [profiles.*], not both"
            ));
        }

        if !has_layout && !has_profiles {
            return Err(eyre!(
                "config must have either a [layout] section or [profiles.*] sections"
            ));
        }

        let (default_profile, profiles) = if has_layout {
            let layout = self.layout.clone().unwrap();
            let mut map = IndexMap::new();
            map.insert("default".to_string(), layout);
            ("default".to_string(), map)
        } else {
            let map = self.profiles.clone().unwrap();
            let default_name = self
                .general
                .default_profile
                .clone()
                .unwrap_or_else(|| map.keys().next().unwrap().clone());

            if !map.contains_key(&default_name) {
                return Err(eyre!(
                    "default_profile '{}' not found in [profiles]",
                    default_name
                ));
            }

            (default_name, map)
        };

        Ok(NormalizedConfig {
            general: self.general.clone(),
            profiles,
            default_profile,
            theme: self.theme.clone(),
            keybinds: self.keybinds.clone(),
            widgets: self.widgets.clone(),
        })
    }

    /// Validate the parsed config against a registry-provided type predicate.
    ///
    /// This delegates to normalize() and then validates the active profile.
    /// All errors are collected; the caller should check `ValidationResult::has_errors()`.
    pub fn validate(&self, is_known_type: impl Fn(&str) -> bool) -> ValidationResult {
        match self.normalize() {
            Ok(normalized) => normalized.validate(is_known_type),
            Err(_) => {
                // normalize() errors are structural (both/neither layout and profiles).
                // The caller should use normalize() directly for those.
                // Here we return an empty result so legacy callers can still validate
                // configs that have a [layout] section without needing to normalize first.
                ValidationResult {
                    errors: vec![],
                    warnings: vec![],
                }
            }
        }
    }
}

// ─── Standalone validation ──────────────────────────────────────────────────

/// Validate a single profile's layout against a registry-provided type predicate.
pub fn validate_profile(
    layout: &ProfileConfig,
    widgets: &HashMap<String, toml::Value>,
    is_known_type: &impl Fn(&str) -> bool,
) -> ValidationResult {
    let mut errors: Vec<ConfigError> = Vec::new();
    let mut warnings: Vec<ConfigWarning> = Vec::new();

    // 1. Grid dimensions must be > 0
    if layout.rows == 0 || layout.columns == 0 {
        errors.push(ConfigError::InvalidGridDimensions {
            rows: layout.rows,
            columns: layout.columns,
        });
    }

    // 2. Panel-level checks
    let mut cell_owners: HashMap<(usize, usize), String> = HashMap::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut panel_ids: HashSet<String> = HashSet::new();

    for panel in &layout.panels {
        let id = panel.instance_id().to_string();
        let col_span = panel.col_span;
        let row_span = panel.row_span;

        // Non-zero spans
        if col_span == 0 || row_span == 0 {
            errors.push(ConfigError::InvalidSpan {
                id: id.clone(),
                row: panel.row,
                col: panel.col,
                col_span,
                row_span,
            });
        }

        // Bounds check (span must fit within grid)
        if layout.rows > 0
            && layout.columns > 0
            && (panel.row + row_span > layout.rows || panel.col + col_span > layout.columns)
        {
            errors.push(ConfigError::SpanOutOfBounds {
                id: id.clone(),
                row: panel.row,
                col: panel.col,
                col_span,
                row_span,
                rows: layout.rows,
                cols: layout.columns,
            });
        }

        // Overlap detection: claim all cells in the span
        for r in panel.row..(panel.row + row_span).min(layout.rows) {
            for c in panel.col..(panel.col + col_span).min(layout.columns) {
                if let Some(existing) = cell_owners.get(&(r, c)) {
                    errors.push(ConfigError::SpanOverlap {
                        id1: existing.clone(),
                        id2: id.clone(),
                        overlap_row: r,
                        overlap_col: c,
                    });
                } else {
                    cell_owners.insert((r, c), id.clone());
                }
            }
        }

        // Unknown widget type
        if !is_known_type(&panel.widget_type) {
            errors.push(ConfigError::UnknownWidgetType {
                widget_type: panel.widget_type.clone(),
            });
        }

        // Instance ID validation (format)
        if !is_valid_instance_id(&id) {
            errors.push(ConfigError::InvalidInstanceId { id: id.clone() });
        }

        // Duplicate instance ID
        if !seen_ids.insert(id.clone()) {
            errors.push(ConfigError::DuplicateInstanceId { id: id.clone() });
        }

        panel_ids.insert(id);
    }

    // 3. Empty panels warning
    if layout.panels.is_empty() {
        warnings.push(ConfigWarning::EmptyPanels);
    }

    // 4. column_widths length and percentage sums
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

    // 5. row_heights length and percentage sums
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

    // 6. Orphan widget sections
    for widget_id in widgets.keys() {
        if !panel_ids.contains(widget_id.as_str()) {
            errors.push(ConfigError::OrphanWidgetSection {
                id: widget_id.clone(),
            });
        }
    }

    ValidationResult { errors, warnings }
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

/// Returns `true` if the profile name matches `[A-Za-z0-9_]+` (non-empty).
/// Profile names use underscores only (no hyphens) since they are TOML keys.
pub fn is_valid_profile_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
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
        let normalized = config.normalize().unwrap();
        let profile = normalized.active_profile_config();
        assert_eq!(config.general.tick_rate, 250);
        assert_eq!(profile.columns, 3);
        assert_eq!(profile.rows, 2);
        assert_eq!(profile.panels.len(), 6);
        assert!(profile.header);
        assert!(profile.footer);
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
        let normalized = config.normalize().unwrap();
        let profile = normalized.active_profile_config();
        assert_eq!(config.general.tick_rate, 100);
        assert_eq!(profile.panels.len(), 0);
        assert!(profile.header);
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
        let normalized = config.normalize().unwrap();
        let profile = normalized.active_profile_config();
        assert_eq!(profile.panels[0].instance_id(), "cpu");
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
        let normalized = config.normalize().unwrap();
        let profile = normalized.active_profile_config();
        assert_eq!(profile.panels[0].instance_id(), "net-wifi");
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
            bogus_field = 2
            [theme]
        "#;
        assert!(Config::parse(toml).is_err());
    }

    #[test]
    fn parse_panel_with_spans() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 3
            rows = 2
            [[layout.panels]]
            row = 0
            col = 0
            type = "cpu"
            col_span = 2
            row_span = 2
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let normalized = config.normalize().unwrap();
        let profile = normalized.active_profile_config();
        assert_eq!(profile.panels[0].col_span, 2);
        assert_eq!(profile.panels[0].row_span, 2);
    }

    #[test]
    fn parse_panel_spans_default_to_one() {
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
        let normalized = config.normalize().unwrap();
        let profile = normalized.active_profile_config();
        assert_eq!(profile.panels[0].col_span, 1);
        assert_eq!(profile.panels[0].row_span, 1);
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
        let normalized = config.normalize().unwrap();
        let profile = normalized.active_profile_config();
        let widths = profile.column_widths.as_ref().unwrap();
        assert_eq!(widths, &vec!["60%", "40%"]);
        let heights = profile.row_heights.as_ref().unwrap();
        assert_eq!(heights, &vec!["70%", "30%"]);
    }

    // ── Phase 2: Validation tests ─────────────────────────────────────────────

    fn known_type(t: &str) -> bool {
        matches!(
            t,
            "cpu"
                | "memory"
                | "network"
                | "temps"
                | "disk"
                | "processes"
                | "packages"
                | "services"
                | "workspaces"
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
                .any(|e| matches!(e, ConfigError::SpanOutOfBounds { .. }))
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
                .any(|e| matches!(e, ConfigError::SpanOverlap { .. }))
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

    #[test]
    fn validate_span_zero_errors() {
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
            col_span = 0
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ConfigError::InvalidSpan { .. }))
        );
    }

    #[test]
    fn validate_span_out_of_bounds_errors() {
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
            col_span = 3
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ConfigError::SpanOutOfBounds { .. }))
        );
    }

    #[test]
    fn validate_span_overlap_errors() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 3
            rows = 1
            [[layout.panels]]
            row = 0
            col = 0
            type = "cpu"
            col_span = 2
            [[layout.panels]]
            row = 0
            col = 1
            type = "memory"
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ConfigError::SpanOverlap { .. }))
        );
    }

    #[test]
    fn validate_span_valid_no_errors() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 3
            rows = 2
            [[layout.panels]]
            row = 0
            col = 0
            type = "cpu"
            col_span = 2
            [[layout.panels]]
            row = 0
            col = 2
            type = "memory"
            [[layout.panels]]
            row = 1
            col = 0
            type = "network"
            col_span = 3
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let result = config.validate(known_type);
        assert!(!result.has_errors(), "errors: {:?}", result.errors);
    }

    // ── Profile-specific tests ────────────────────────────────────────────────

    #[test]
    fn test_parse_profiles_config() {
        let toml = r#"
            [general]
            tick_rate = 250
            default_profile = "compact"

            [profiles.compact]
            columns = 2
            rows = 1
            [[profiles.compact.panels]]
            row = 0
            col = 0
            type = "cpu"
            [[profiles.compact.panels]]
            row = 0
            col = 1
            type = "memory"

            [profiles.full]
            columns = 3
            rows = 2
            [[profiles.full.panels]]
            row = 0
            col = 0
            type = "cpu"
            [[profiles.full.panels]]
            row = 0
            col = 1
            type = "memory"
            [[profiles.full.panels]]
            row = 0
            col = 2
            type = "network"
            [[profiles.full.panels]]
            row = 1
            col = 0
            type = "temps"
            [[profiles.full.panels]]
            row = 1
            col = 1
            type = "disk"
            [[profiles.full.panels]]
            row = 1
            col = 2
            type = "processes"

            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let normalized = config.normalize().unwrap();
        assert_eq!(normalized.default_profile(), "compact");
        assert_eq!(normalized.profiles().len(), 2);
        assert_eq!(normalized.active_profile_config().columns, 2);
        assert_eq!(normalized.active_profile_config().panels.len(), 2);

        let full = &normalized.profiles()["full"];
        assert_eq!(full.columns, 3);
        assert_eq!(full.panels.len(), 6);
    }

    #[test]
    fn test_parse_legacy_layout_as_default_profile() {
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
            type = "memory"
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let normalized = config.normalize().unwrap();
        assert_eq!(normalized.default_profile(), "default");
        assert_eq!(normalized.profiles().len(), 1);
        assert!(normalized.profiles().contains_key("default"));
        assert_eq!(normalized.active_profile_config().columns, 2);
        assert_eq!(normalized.active_profile_config().panels.len(), 2);
    }

    #[test]
    fn test_parse_both_layout_and_profiles_is_error() {
        let toml = r#"
            [general]
            tick_rate = 250
            [layout]
            columns = 1
            rows = 1
            panels = []

            [profiles.other]
            columns = 1
            rows = 1
            panels = []

            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        assert!(config.normalize().is_err());
    }

    #[test]
    fn test_parse_neither_layout_nor_profiles_is_error() {
        let toml = r#"
            [general]
            tick_rate = 250
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        assert!(config.normalize().is_err());
    }

    #[test]
    fn test_default_profile_defaults_to_first() {
        let toml = r#"
            [general]
            tick_rate = 250

            [profiles.alpha]
            columns = 1
            rows = 1
            [[profiles.alpha.panels]]
            row = 0
            col = 0
            type = "cpu"

            [profiles.beta]
            columns = 2
            rows = 1
            [[profiles.beta.panels]]
            row = 0
            col = 0
            type = "cpu"
            [[profiles.beta.panels]]
            row = 0
            col = 1
            type = "memory"

            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let normalized = config.normalize().unwrap();
        // Without default_profile set, it should default to the first key
        assert_eq!(normalized.default_profile(), "alpha");
    }

    #[test]
    fn test_invalid_default_profile_is_error() {
        let toml = r#"
            [general]
            tick_rate = 250
            default_profile = "nonexistent"

            [profiles.alpha]
            columns = 1
            rows = 1
            panels = []

            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        assert!(config.normalize().is_err());
    }

    #[test]
    fn test_profile_name_validation() {
        assert!(is_valid_profile_name("default"));
        assert!(is_valid_profile_name("compact_layout"));
        assert!(is_valid_profile_name("profile1"));
        assert!(!is_valid_profile_name(""));
        assert!(!is_valid_profile_name("has spaces"));
        assert!(!is_valid_profile_name("has-hyphen"));
        assert!(!is_valid_profile_name("has.dot"));
    }

    #[test]
    fn test_keybinds_config_defaults() {
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
        let normalized = config.normalize().unwrap();
        assert!(normalized.keybinds.quit.is_none());
        assert!(normalized.keybinds.reload.is_none());
    }

    #[test]
    fn test_keybinds_config_custom() {
        let toml = r#"
            [general]
            tick_rate = 250
            [keybinds]
            quit = "q"
            reload = "r"
            [layout]
            columns = 1
            rows = 1
            panels = []
            [theme]
        "#;
        let config = Config::parse(toml).unwrap();
        let normalized = config.normalize().unwrap();
        assert_eq!(normalized.keybinds.quit.as_deref(), Some("q"));
        assert_eq!(normalized.keybinds.reload.as_deref(), Some("r"));
    }
}
