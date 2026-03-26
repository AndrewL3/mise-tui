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
use crate::data::types::{ActiveState, ServiceStatus};
use crate::event::Event;
use crate::theme::Theme;

fn default_scope() -> String {
    "system".to_string()
}
fn default_timeout() -> u64 {
    10
}
fn default_interval() -> u64 {
    10
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServicesConfig {
    #[serde(default = "default_scope")]
    pub scope: String,
    pub services: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default = "default_interval")]
    pub interval: u64,
}

#[derive(Debug)]
pub struct ServicesWidget {
    id: String,
    pub(crate) config: ServicesConfig,
    pub(crate) statuses: Vec<ServiceStatus>,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
}

impl ServicesWidget {
    pub fn new(id: String, config: Option<toml::Value>) -> Result<Self> {
        let config: ServicesConfig = match config {
            Some(value) => value.try_into()?,
            None => {
                return Err(eyre!(
                    "services widget requires a [widgets.X] section with services list"
                ));
            }
        };
        match config.scope.as_str() {
            "system" | "user" => {}
            other => {
                return Err(eyre!(
                    "invalid services scope: '{other}' (expected 'system' or 'user')"
                ));
            }
        }
        if config.services.is_empty() {
            return Err(eyre!("services list must not be empty"));
        }
        Ok(Self {
            id,
            config,
            statuses: Vec::new(),
            loading: true,
            error: None,
        })
    }

    fn state_style(&self, state: &ActiveState, theme: &Theme) -> Style {
        match state {
            ActiveState::Active => theme.gauge_fill,
            ActiveState::Failed => theme.critical,
            _ => theme.border,
        }
    }

    fn state_dot(&self, state: &ActiveState, theme: &Theme) -> Span<'static> {
        let style = self.state_style(state, theme);
        Span::styled("\u{25cf}", style) // ●
    }

    fn draw_content(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Loading state
        if self.loading {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled("Loading\u{2026}", theme.label))),
                area,
            );
            return;
        }

        // Error state
        if let Some(ref err) = self.error {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    err.clone(),
                    Style::default().fg(theme.error_fg),
                ))),
                area,
            );
            return;
        }

        // Minimal: just text label
        if area.height < 3 || area.width < 15 {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled("Services", theme.value))),
                area,
            );
            return;
        }

        // Medium: compact dot + name (height 3-5)
        if area.height < 6 {
            let lines: Vec<Line> = self
                .statuses
                .iter()
                .take(area.height as usize)
                .map(|s| {
                    Line::from(vec![
                        self.state_dot(&s.active_state, theme),
                        Span::raw(" "),
                        Span::styled(s.name.clone(), theme.label),
                    ])
                })
                .collect();
            frame.render_widget(Paragraph::new(lines), area);
            return;
        }

        // Full: table with Name, State, Sub-State columns
        // Sort failed services to top
        let mut sorted: Vec<&ServiceStatus> = self.statuses.iter().collect();
        sorted.sort_by_key(|s| match s.active_state {
            ActiveState::Failed => 0,
            _ => 1,
        });

        let header = Row::new(vec![
            Cell::from(Span::styled("Name", Style::default().fg(theme.header_fg))),
            Cell::from(Span::styled("State", Style::default().fg(theme.header_fg))),
            Cell::from(Span::styled(
                "Sub-State",
                Style::default().fg(theme.header_fg),
            )),
        ])
        .style(Style::default().bg(theme.header_bg));

        let rows: Vec<Row> = sorted
            .iter()
            .map(|s| {
                let state_str = match &s.active_state {
                    ActiveState::Active => "active",
                    ActiveState::Inactive => "inactive",
                    ActiveState::Failed => "failed",
                    ActiveState::Activating => "activating",
                    ActiveState::Deactivating => "deactivating",
                    ActiveState::Other(val) => val.as_str(),
                };
                let style = self.state_style(&s.active_state, theme);
                Row::new(vec![
                    Cell::from(Span::styled(s.name.clone(), theme.label)),
                    Cell::from(Span::styled(state_str.to_string(), style)),
                    Cell::from(Span::styled(s.sub_state.clone(), theme.label)),
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

impl Component for ServicesWidget {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        "Services"
    }
    fn widget_type(&self) -> &str {
        "services"
    }
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn update(&mut self) -> Result<Option<Action>> {
        Ok(None)
    }

    fn handle_data(&mut self, update: &DataUpdate) -> Result<Option<Action>> {
        if let DataUpdate::Services(result) = update
            && result.instance_id == self.id
        {
            match &result.data {
                Ok(data) => {
                    self.statuses = data.services.clone();
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
        if let Some(old_services) = old.as_any().downcast_ref::<ServicesWidget>() {
            self.statuses = old_services.statuses.clone();
            self.loading = old_services.loading;
            self.error = old_services.error.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{CpuData, ServicesData, ServicesResult};

    fn make_config(toml_str: &str) -> Option<toml::Value> {
        Some(toml::from_str(toml_str).unwrap())
    }

    fn sample_statuses() -> Vec<ServiceStatus> {
        vec![
            ServiceStatus {
                name: "sshd".to_string(),
                active_state: ActiveState::Active,
                sub_state: "running".to_string(),
            },
            ServiceStatus {
                name: "nginx".to_string(),
                active_state: ActiveState::Failed,
                sub_state: "failed".to_string(),
            },
        ]
    }

    #[test]
    fn config_from_toml() {
        let val = make_config(
            r#"
            scope = "user"
            services = ["sshd", "nginx"]
            timeout = 5
            interval = 30
        "#,
        );
        let w = ServicesWidget::new("svc".into(), val).unwrap();
        assert_eq!(w.config.scope, "user");
        assert_eq!(w.config.services, vec!["sshd", "nginx"]);
        assert_eq!(w.config.timeout, 5);
        assert_eq!(w.config.interval, 30);
    }

    #[test]
    fn config_defaults() {
        let val = make_config(r#"services = ["sshd"]"#);
        let w = ServicesWidget::new("svc".into(), val).unwrap();
        assert_eq!(w.config.scope, "system");
        assert_eq!(w.config.timeout, 10);
        assert_eq!(w.config.interval, 10);
    }

    #[test]
    fn invalid_scope_errors() {
        let val = make_config(
            r#"
            scope = "invalid"
            services = ["sshd"]
        "#,
        );
        let result = ServicesWidget::new("svc".into(), val);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("invalid services scope"));
    }

    #[test]
    fn missing_services_errors() {
        let result = ServicesWidget::new("svc".into(), None);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("services widget requires"));
    }

    #[test]
    fn empty_services_errors() {
        let val = make_config(r#"services = []"#);
        let result = ServicesWidget::new("svc".into(), val);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("must not be empty"));
    }

    #[test]
    fn unknown_field_errors() {
        let val = make_config(
            r#"
            services = ["sshd"]
            bogus = true
        "#,
        );
        let result = ServicesWidget::new("svc".into(), val);
        assert!(result.is_err());
    }

    #[test]
    fn handle_data_updates_state() {
        let val = make_config(r#"services = ["sshd", "nginx"]"#);
        let mut w = ServicesWidget::new("svc".into(), val).unwrap();
        assert!(w.loading);
        assert!(w.statuses.is_empty());

        let update = DataUpdate::Services(ServicesResult {
            instance_id: "svc".to_string(),
            data: Ok(ServicesData {
                services: sample_statuses(),
            }),
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.statuses.len(), 2);
        assert!(!w.loading);
        assert!(w.error.is_none());
    }

    #[test]
    fn handle_data_stores_error() {
        let val = make_config(r#"services = ["sshd"]"#);
        let mut w = ServicesWidget::new("svc".into(), val).unwrap();

        let update = DataUpdate::Services(ServicesResult {
            instance_id: "svc".to_string(),
            data: Err("systemctl not found".to_string()),
        });
        w.handle_data(&update).unwrap();
        assert!(!w.loading);
        assert_eq!(w.error.as_deref(), Some("systemctl not found"));
    }

    #[test]
    fn handle_data_ignores_other_variants() {
        let val = make_config(r#"services = ["sshd"]"#);
        let mut w = ServicesWidget::new("svc".into(), val).unwrap();

        let update = DataUpdate::Cpu(CpuData {
            per_core: vec![50.0],
            overall: 50.0,
        });
        w.handle_data(&update).unwrap();
        assert!(w.loading); // still loading, not changed
        assert!(w.statuses.is_empty());
    }

    #[test]
    fn handle_data_ignores_mismatched_id() {
        let val = make_config(r#"services = ["sshd"]"#);
        let mut w = ServicesWidget::new("svc".into(), val).unwrap();

        let update = DataUpdate::Services(ServicesResult {
            instance_id: "other_svc".to_string(),
            data: Ok(ServicesData {
                services: sample_statuses(),
            }),
        });
        w.handle_data(&update).unwrap();
        assert!(w.loading); // still loading, not changed
        assert!(w.statuses.is_empty());
    }

    #[test]
    fn transfer_state_preserves_data() {
        let val = make_config(r#"services = ["sshd"]"#);
        let mut old = ServicesWidget::new("svc".into(), val).unwrap();
        old.statuses = sample_statuses();
        old.loading = false;
        old.error = Some("previous error".to_string());

        let val2 = make_config(r#"services = ["sshd"]"#);
        let mut new_w = ServicesWidget::new("svc".into(), val2).unwrap();
        assert!(new_w.statuses.is_empty());
        assert!(new_w.loading);

        new_w.transfer_state(&old);
        assert_eq!(new_w.statuses.len(), 2);
        assert!(!new_w.loading);
        assert_eq!(new_w.error.as_deref(), Some("previous error"));
    }

    #[test]
    fn draw_does_not_panic_at_small_sizes() {
        let val = make_config(r#"services = ["sshd"]"#);
        let mut w = ServicesWidget::new("svc".into(), val).unwrap();
        w.loading = false;
        w.statuses = vec![ServiceStatus {
            name: "sshd".to_string(),
            active_state: ActiveState::Active,
            sub_state: "running".to_string(),
        }];
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
