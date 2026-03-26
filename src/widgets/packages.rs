use std::any::Any;

use color_eyre::{Result, eyre::eyre};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};
use serde::Deserialize;

use crate::action::Action;
use crate::component::Component;
use crate::data::DataUpdate;
use crate::data::types::PackageUpdate;
use crate::event::Event;
use crate::theme::Theme;

fn default_mode() -> String {
    "list".to_string()
}
fn default_warn_packages() -> Vec<String> {
    vec![]
}
fn default_timeout() -> u64 {
    30
}
fn default_interval() -> u64 {
    1800
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackagesConfig {
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_warn_packages")]
    pub warn_packages: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default = "default_interval")]
    pub interval: u64,
}

impl Default for PackagesConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            warn_packages: default_warn_packages(),
            timeout: default_timeout(),
            interval: default_interval(),
        }
    }
}

pub struct PackagesWidget {
    id: String,
    pub(crate) config: PackagesConfig,
    pub(crate) updates: Vec<PackageUpdate>,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
}

impl PackagesWidget {
    pub fn new(id: String, config: Option<toml::Value>) -> Result<Self> {
        let config: PackagesConfig = match config {
            Some(value) => value.try_into()?,
            None => PackagesConfig::default(),
        };
        match config.mode.as_str() {
            "badge" | "list" => {}
            other => return Err(eyre!("invalid packages mode: '{other}'")),
        }
        Ok(Self {
            id,
            config,
            updates: Vec::new(),
            loading: true,
            error: None,
        })
    }

    fn has_warnings(&self) -> bool {
        self.updates
            .iter()
            .any(|u| self.config.warn_packages.contains(&u.name))
    }

    fn draw_content(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Minimal: just text label
        if area.height < 3 || area.width < 15 {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled("Packages", theme.value))),
                area,
            );
            return;
        }

        // Badge mode: single-line summary
        if area.height < 5 || self.config.mode == "badge" {
            self.draw_badge(frame, area, theme);
            return;
        }

        // List mode: table with columns
        self.draw_list(frame, area, theme);
    }

    fn draw_badge(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.loading {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled("Loading\u{2026}", theme.label))),
                area,
            );
            return;
        }

        if let Some(ref err) = self.error {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(err.as_str(), theme.critical))),
                area,
            );
            return;
        }

        if self.updates.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled("No updates", theme.title))),
                area,
            );
            return;
        }

        let count = self.updates.len();
        let style = if self.has_warnings() {
            theme.warning
        } else {
            theme.gauge_fill
        };
        let label = if count == 1 {
            "1 update".to_string()
        } else {
            format!("{count} updates")
        };
        frame.render_widget(Paragraph::new(Line::from(Span::styled(label, style))), area);
    }

    fn draw_list(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.loading {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled("Loading\u{2026}", theme.label))),
                area,
            );
            return;
        }

        if let Some(ref err) = self.error {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(err.as_str(), theme.critical))),
                area,
            );
            return;
        }

        if self.updates.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled("No updates", theme.title))),
                area,
            );
            return;
        }

        // Header row takes 1 line, each update takes 1 line
        let available_rows = (area.height as usize).saturating_sub(1);
        let visible = self.updates.len().min(available_rows.max(1));

        let header = Row::new(vec![
            Cell::from(Span::styled("Name", theme.label)),
            Cell::from(Span::styled("Old", theme.label)),
            Cell::from(Span::styled("New", theme.label)),
        ])
        .style(Style::default());

        let rows: Vec<Row> = self
            .updates
            .iter()
            .take(visible)
            .map(|u| {
                let style = if self.config.warn_packages.contains(&u.name) {
                    theme.warning
                } else {
                    theme.value
                };
                Row::new(vec![
                    Cell::from(Span::styled(u.name.clone(), style)),
                    Cell::from(Span::styled(u.old_version.clone(), style)),
                    Cell::from(Span::styled(u.new_version.clone(), style)),
                ])
            })
            .collect();

        let widths = [
            Constraint::Percentage(40),
            Constraint::Percentage(30),
            Constraint::Percentage(30),
        ];

        let table = Table::new(rows, widths).header(header);
        frame.render_widget(table, area);
    }
}

impl Component for PackagesWidget {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        "Packages"
    }
    fn widget_type(&self) -> &str {
        "packages"
    }
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn update(&mut self) -> Result<Option<Action>> {
        Ok(None)
    }

    fn handle_data(&mut self, update: &DataUpdate) -> Result<Option<Action>> {
        if let DataUpdate::Packages(result) = update
            && result.instance_id == self.id
        {
            match &result.data {
                Ok(data) => {
                    self.updates = data.updates.clone();
                    self.error = None;
                    self.loading = false;
                }
                Err(msg) => {
                    self.error = Some(msg.clone());
                    self.loading = false;
                }
            }
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        self.draw_content(frame, area, theme);
    }

    fn handle_event(&mut self, _event: &Event) -> Result<Option<Action>> {
        Ok(None)
    }

    fn min_size(&self) -> (u16, u16) {
        (15, 1)
    }

    fn transfer_state(&mut self, old: &dyn Component) {
        if let Some(old_pkg) = old.as_any().downcast_ref::<PackagesWidget>() {
            self.updates = old_pkg.updates.clone();
            self.loading = old_pkg.loading;
            self.error = old_pkg.error.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{CpuData, PackagesData, PackagesResult};

    #[test]
    fn default_config_values() {
        let w = PackagesWidget::new("packages".into(), None).unwrap();
        assert_eq!(w.config.mode, "list");
        assert!(w.config.warn_packages.is_empty());
        assert_eq!(w.config.timeout, 30);
        assert_eq!(w.config.interval, 1800);
    }

    #[test]
    fn config_from_toml() {
        let val: toml::Value = toml::from_str(
            r#"
            mode = "badge"
            warn_packages = ["linux", "nvidia"]
            timeout = 60
            interval = 3600
        "#,
        )
        .unwrap();
        let w = PackagesWidget::new("packages".into(), Some(val)).unwrap();
        assert_eq!(w.config.mode, "badge");
        assert_eq!(w.config.warn_packages, vec!["linux", "nvidia"]);
        assert_eq!(w.config.timeout, 60);
        assert_eq!(w.config.interval, 3600);
    }

    #[test]
    fn invalid_mode_errors() {
        let val: toml::Value = toml::from_str(r#"mode = "invalid""#).unwrap();
        assert!(PackagesWidget::new("packages".into(), Some(val)).is_err());
    }

    #[test]
    fn unknown_field_errors() {
        let val: toml::Value = toml::from_str(r#"bogus = true"#).unwrap();
        assert!(PackagesWidget::new("packages".into(), Some(val)).is_err());
    }

    #[test]
    fn handle_data_updates_state() {
        let mut w = PackagesWidget::new("packages".into(), None).unwrap();
        // Set an error first to verify it gets cleared
        w.error = Some("old error".into());
        let update = DataUpdate::Packages(PackagesResult {
            instance_id: "packages".to_string(),
            data: Ok(PackagesData {
                updates: vec![PackageUpdate {
                    name: "linux".to_string(),
                    old_version: "6.8.1".to_string(),
                    new_version: "6.8.2".to_string(),
                }],
            }),
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.updates.len(), 1);
        assert_eq!(w.updates[0].name, "linux");
        assert!(!w.loading);
        assert!(w.error.is_none());
    }

    #[test]
    fn handle_data_stores_error() {
        let mut w = PackagesWidget::new("packages".into(), None).unwrap();
        let update = DataUpdate::Packages(PackagesResult {
            instance_id: "packages".to_string(),
            data: Err("checkupdates not found".to_string()),
        });
        w.handle_data(&update).unwrap();
        assert!(w.error.is_some());
        assert_eq!(w.error.as_deref(), Some("checkupdates not found"));
        assert!(!w.loading);
    }

    #[test]
    fn handle_data_ignores_other_variants() {
        let mut w = PackagesWidget::new("packages".into(), None).unwrap();
        let update = DataUpdate::Cpu(CpuData {
            per_core: vec![50.0],
            overall: 50.0,
        });
        w.handle_data(&update).unwrap();
        assert!(w.updates.is_empty());
        assert!(w.loading); // still loading, unchanged
    }

    #[test]
    fn handle_data_ignores_mismatched_id() {
        let mut w = PackagesWidget::new("packages".into(), None).unwrap();
        let update = DataUpdate::Packages(PackagesResult {
            instance_id: "other_packages".to_string(),
            data: Ok(PackagesData {
                updates: vec![PackageUpdate {
                    name: "linux".to_string(),
                    old_version: "6.8.1".to_string(),
                    new_version: "6.8.2".to_string(),
                }],
            }),
        });
        w.handle_data(&update).unwrap();
        assert!(w.updates.is_empty());
        assert!(w.loading); // still loading, unchanged
    }

    #[test]
    fn warn_packages_matching() {
        let val: toml::Value = toml::from_str(r#"warn_packages = ["linux"]"#).unwrap();
        let mut w = PackagesWidget::new("packages".into(), Some(val)).unwrap();
        w.updates = vec![
            PackageUpdate {
                name: "linux".to_string(),
                old_version: "6.8.1".to_string(),
                new_version: "6.8.2".to_string(),
            },
            PackageUpdate {
                name: "vim".to_string(),
                old_version: "9.0".to_string(),
                new_version: "9.1".to_string(),
            },
        ];
        assert!(w.has_warnings());
    }

    #[test]
    fn transfer_state_preserves_data() {
        let mut old = PackagesWidget::new("packages".into(), None).unwrap();
        old.updates = vec![PackageUpdate {
            name: "linux".to_string(),
            old_version: "6.8.1".to_string(),
            new_version: "6.8.2".to_string(),
        }];
        old.loading = false;
        old.error = Some("test error".to_string());

        let mut new_w = PackagesWidget::new("packages".into(), None).unwrap();
        assert!(new_w.updates.is_empty());
        assert!(new_w.loading);
        assert!(new_w.error.is_none());

        new_w.transfer_state(&old);
        assert_eq!(new_w.updates.len(), 1);
        assert_eq!(new_w.updates[0].name, "linux");
        assert!(!new_w.loading);
        assert_eq!(new_w.error.as_deref(), Some("test error"));
    }

    #[test]
    fn draw_does_not_panic_at_small_sizes() {
        let mut w = PackagesWidget::new("packages".into(), None).unwrap();
        let backend = ratatui::backend::TestBackend::new(15, 3);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let theme = crate::theme::Theme::default();
        terminal
            .draw(|frame| {
                w.draw(frame, frame.area(), &theme);
            })
            .unwrap();
    }
}
