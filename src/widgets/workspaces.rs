use std::any::Any;

use color_eyre::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use serde::Deserialize;

use crate::action::Action;
use crate::component::Component;
use crate::data::DataUpdate;
use crate::data::types::{HyprlandData, WorkspaceInfo};
use crate::event::Event;
use crate::theme::Theme;

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspacesConfig {
    #[serde(default = "default_true")]
    show_window_count: bool,
    #[serde(default = "default_true")]
    show_active_window: bool,
}

impl Default for WorkspacesConfig {
    fn default() -> Self {
        Self {
            show_window_count: true,
            show_active_window: true,
        }
    }
}

pub struct WorkspacesWidget {
    id: String,
    config: WorkspacesConfig,
    data: Option<HyprlandData>,
}

impl WorkspacesWidget {
    pub fn new(id: String, config: Option<toml::Value>) -> Result<Self> {
        let config: WorkspacesConfig = match config {
            Some(value) => value.try_into()?,
            None => WorkspacesConfig::default(),
        };
        Ok(Self {
            id,
            config,
            data: None,
        })
    }
}

impl Component for WorkspacesWidget {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        "Workspaces"
    }

    fn widget_type(&self) -> &str {
        "workspaces"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn update(&mut self) -> Result<Option<Action>> {
        Ok(None)
    }

    fn handle_data(&mut self, update: &DataUpdate) -> Result<Option<Action>> {
        if let DataUpdate::Hyprland(data) = update {
            self.data = Some(data.clone());
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let Some(data) = &self.data else {
            let text = Paragraph::new("Waiting for data...");
            frame.render_widget(text, area);
            return;
        };

        if !data.detected {
            let text = Paragraph::new("Hyprland not detected");
            frame.render_widget(text, area);
            return;
        }

        if !data.connected {
            let text = Paragraph::new(Line::from(Span::styled("Reconnecting...", theme.warning)));
            frame.render_widget(text, area);
            return;
        }

        // Minimal tier: < 20 cols or < 3 rows
        if area.width < 20 || area.height < 3 {
            self.draw_minimal(frame, area, data, theme);
            return;
        }

        // Full tier: >= 30 cols and >= 5 rows (medium + active window)
        if area.width >= 30 && area.height >= 5 && self.config.show_active_window {
            self.draw_full(frame, area, data, theme);
            return;
        }

        // Medium tier
        self.draw_medium(frame, area, data, theme);
    }

    fn handle_event(&mut self, _event: &Event) -> Result<Option<Action>> {
        Ok(None)
    }

    fn min_size(&self) -> (u16, u16) {
        (15, 1)
    }

    fn transfer_state(&mut self, old: &dyn Component) {
        if let Some(old_ws) = old.as_any().downcast_ref::<WorkspacesWidget>() {
            self.data = old_ws.data.clone();
        }
    }
}

impl WorkspacesWidget {
    fn draw_minimal(&self, frame: &mut Frame, area: Rect, data: &HyprlandData, theme: &Theme) {
        let mut spans = vec![Span::styled("WS: ", theme.label)];
        let mut sorted_workspaces: Vec<&WorkspaceInfo> = data.workspaces.iter().collect();
        sorted_workspaces.sort_by_key(|w| w.id);

        for ws in &sorted_workspaces {
            let is_active = data.active_workspace == Some(ws.id);
            let label = if is_active {
                format!("[{}]", ws.id)
            } else {
                format!("{}", ws.id)
            };
            let style = if is_active {
                theme.border_focused
            } else {
                theme.value
            };
            spans.push(Span::styled(label, style));
            spans.push(Span::raw(" "));
        }

        let line = Line::from(spans);
        frame.render_widget(Paragraph::new(line), area);
    }

    fn draw_medium(&self, frame: &mut Frame, area: Rect, data: &HyprlandData, theme: &Theme) {
        let mut lines = Vec::new();
        let mut sorted_workspaces: Vec<&WorkspaceInfo> = data.workspaces.iter().collect();
        sorted_workspaces.sort_by_key(|w| w.id);

        for monitor in &data.monitors {
            let mon_workspaces: Vec<&&WorkspaceInfo> = sorted_workspaces
                .iter()
                .filter(|w| w.monitor == monitor.name)
                .collect();

            if data.monitors.len() > 1 {
                lines.push(Line::from(Span::styled(
                    format!(" {}", monitor.name),
                    theme.label,
                )));
            }

            let mut spans = vec![Span::raw(" ")];
            for ws in &mon_workspaces {
                let is_active = monitor.active_workspace_id == ws.id;
                let mut label = if is_active {
                    format!("[{}]", ws.name)
                } else {
                    ws.name.clone()
                };
                if self.config.show_window_count {
                    label.push_str(&format!("({})", ws.window_count));
                }
                let style = if is_active {
                    theme.border_focused
                } else {
                    theme.value
                };
                spans.push(Span::styled(label, style));
                spans.push(Span::raw("  "));
            }
            lines.push(Line::from(spans));
        }

        let text = Paragraph::new(lines);
        frame.render_widget(text, area);
    }

    fn draw_full(&self, frame: &mut Frame, area: Rect, data: &HyprlandData, theme: &Theme) {
        let mut lines = Vec::new();
        let mut sorted_workspaces: Vec<&WorkspaceInfo> = data.workspaces.iter().collect();
        sorted_workspaces.sort_by_key(|w| w.id);

        for monitor in &data.monitors {
            let mon_workspaces: Vec<&&WorkspaceInfo> = sorted_workspaces
                .iter()
                .filter(|w| w.monitor == monitor.name)
                .collect();

            if data.monitors.len() > 1 {
                lines.push(Line::from(Span::styled(
                    format!(" {}", monitor.name),
                    theme.label,
                )));
            }

            let mut spans = vec![Span::raw(" ")];
            for ws in &mon_workspaces {
                let is_active = monitor.active_workspace_id == ws.id;
                let mut label = if is_active {
                    format!("[{}]", ws.name)
                } else {
                    ws.name.clone()
                };
                if self.config.show_window_count {
                    label.push_str(&format!("({})", ws.window_count));
                }
                let style = if is_active {
                    theme.border_focused
                } else {
                    theme.value
                };
                spans.push(Span::styled(label, style));
                spans.push(Span::raw("  "));
            }
            lines.push(Line::from(spans));
        }

        if let Some(title) = &data.active_window {
            lines.push(Line::from(""));
            let truncated = if title.len() > area.width as usize - 2 {
                format!(" {}\u{2026}", &title[..area.width as usize - 3])
            } else {
                format!(" {}", title)
            };
            lines.push(Line::from(Span::styled(truncated, theme.label)));
        }

        let text = Paragraph::new(lines);
        frame.render_widget(text, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{CpuData, MonitorInfo, WorkspaceInfo};

    #[test]
    fn new_with_default_config() {
        let widget = WorkspacesWidget::new("workspaces".to_string(), None).unwrap();
        assert_eq!(widget.id(), "workspaces");
        assert_eq!(widget.widget_type(), "workspaces");
        assert_eq!(widget.name(), "Workspaces");
        assert!(widget.config.show_window_count);
        assert!(widget.config.show_active_window);
    }

    #[test]
    fn new_with_custom_config() {
        let config: toml::Value = toml::from_str(
            r#"
            show_window_count = false
            show_active_window = false
        "#,
        )
        .unwrap();
        let widget = WorkspacesWidget::new("workspaces".to_string(), Some(config)).unwrap();
        assert!(!widget.config.show_window_count);
        assert!(!widget.config.show_active_window);
    }

    #[test]
    fn handle_data_stores_hyprland_data() {
        let mut widget = WorkspacesWidget::new("workspaces".to_string(), None).unwrap();
        assert!(widget.data.is_none());

        let update = DataUpdate::Hyprland(HyprlandData {
            monitors: vec![MonitorInfo {
                id: 0,
                name: "DP-1".to_string(),
                active_workspace_id: 1,
            }],
            workspaces: vec![WorkspaceInfo {
                id: 1,
                name: "1".to_string(),
                monitor: "DP-1".to_string(),
                window_count: 2,
            }],
            active_workspace: Some(1),
            active_window: Some("Test".to_string()),
            connected: true,
            detected: true,
        });

        widget.handle_data(&update).unwrap();
        assert!(widget.data.is_some());
        assert_eq!(widget.data.as_ref().unwrap().monitors.len(), 1);
    }

    #[test]
    fn handle_data_ignores_non_hyprland() {
        let mut widget = WorkspacesWidget::new("workspaces".to_string(), None).unwrap();
        let update = DataUpdate::Cpu(CpuData {
            per_core: vec![],
            overall: 0.0,
        });
        widget.handle_data(&update).unwrap();
        assert!(widget.data.is_none());
    }

    #[test]
    fn does_not_support_interact() {
        let widget = WorkspacesWidget::new("workspaces".to_string(), None).unwrap();
        assert!(!widget.supports_interact());
    }

    #[test]
    fn min_size_is_standard() {
        let widget = WorkspacesWidget::new("workspaces".to_string(), None).unwrap();
        assert_eq!(widget.min_size(), (15, 1));
    }

    #[test]
    fn transfer_state_copies_data() {
        let mut old = WorkspacesWidget::new("workspaces".to_string(), None).unwrap();
        old.data = Some(HyprlandData {
            monitors: vec![],
            workspaces: vec![],
            active_workspace: Some(1),
            active_window: None,
            connected: true,
            detected: true,
        });

        let mut new_widget = WorkspacesWidget::new("workspaces".to_string(), None).unwrap();
        assert!(new_widget.data.is_none());
        new_widget.transfer_state(&old);
        assert!(new_widget.data.is_some());
        assert_eq!(new_widget.data.as_ref().unwrap().active_workspace, Some(1));
    }

    #[test]
    fn unknown_config_field_errors() {
        let config: toml::Value = toml::from_str(
            r#"
            bogus = true
        "#,
        )
        .unwrap();
        assert!(WorkspacesWidget::new("workspaces".to_string(), Some(config)).is_err());
    }
}
