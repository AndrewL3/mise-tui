use color_eyre::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Gauge, Paragraph};
use serde::Deserialize;

use crate::action::Action;
use crate::component::Component;
use crate::data::DataUpdate;
use crate::data::types::ProcessInfo;
use crate::event::Event;
use crate::theme::Theme;
use crate::widgets::util::format_bytes;

fn default_true() -> bool {
    true
}
fn default_process_count() -> usize {
    5
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryConfig {
    #[serde(default = "default_true")]
    show_swap: bool,
    #[serde(default = "default_true")]
    show_processes: bool,
    #[serde(default = "default_process_count")]
    process_count: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            show_swap: default_true(),
            show_processes: default_true(),
            process_count: default_process_count(),
        }
    }
}

pub struct MemoryWidget {
    id: String,
    config: MemoryConfig,
    total_mem: u64,
    used_mem: u64,
    total_swap: u64,
    used_swap: u64,
    top_processes: Vec<ProcessInfo>,
}

impl MemoryWidget {
    pub fn new(id: String, config: Option<toml::Value>) -> Result<Self> {
        let config: MemoryConfig = match config {
            Some(value) => value.try_into()?,
            None => MemoryConfig::default(),
        };
        Ok(Self {
            id,
            config,
            total_mem: 0,
            used_mem: 0,
            total_swap: 0,
            used_swap: 0,
            top_processes: Vec::new(),
        })
    }

    fn gauge_style(theme: &Theme) -> Style {
        Style::new()
            .fg(theme.gauge_fill.fg.unwrap_or_default())
            .bg(theme.gauge_bg.fg.unwrap_or_default())
    }

    fn safe_ratio(used: u64, total: u64) -> f64 {
        if total == 0 {
            0.0
        } else {
            (used as f64 / total as f64).clamp(0.0, 1.0)
        }
    }

    fn draw_content(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let mut constraints: Vec<Constraint> = vec![Constraint::Length(2)]; // RAM gauge always
        let show_swap = self.config.show_swap && self.total_swap > 0;
        if show_swap {
            constraints.push(Constraint::Length(2));
        }
        if self.config.show_processes {
            constraints.push(Constraint::Min(0));
        }

        let areas = Layout::vertical(constraints).split(area);
        let mut area_idx = 0;

        // RAM gauge
        let ram_ratio = Self::safe_ratio(self.used_mem, self.total_mem);
        let ram_label = format!(
            "RAM: {} / {} ({:.0}%)",
            format_bytes(self.used_mem),
            format_bytes(self.total_mem),
            ram_ratio * 100.0,
        );
        let ram_gauge = Gauge::default()
            .gauge_style(Self::gauge_style(theme))
            .ratio(ram_ratio)
            .label(ram_label)
            .use_unicode(true);
        frame.render_widget(ram_gauge, areas[area_idx]);
        area_idx += 1;

        // Swap gauge
        if show_swap {
            let swap_ratio = Self::safe_ratio(self.used_swap, self.total_swap);
            let swap_label = format!(
                "Swap: {} / {} ({:.0}%)",
                format_bytes(self.used_swap),
                format_bytes(self.total_swap),
                swap_ratio * 100.0,
            );
            let swap_gauge = Gauge::default()
                .gauge_style(Self::gauge_style(theme))
                .ratio(swap_ratio)
                .label(swap_label)
                .use_unicode(true);
            frame.render_widget(swap_gauge, areas[area_idx]);
            area_idx += 1;
        }

        // Process list
        if self.config.show_processes && area_idx < areas.len() {
            let proc_area = areas[area_idx];
            let header = Line::from(vec![
                Span::styled("  # ", theme.label),
                Span::styled("Name", theme.label),
                Span::styled("        Memory", theme.label),
            ]);
            let mut lines = vec![header];

            let max_name_width = proc_area.width.saturating_sub(20) as usize;
            for (i, proc) in self.top_processes.iter().enumerate() {
                if lines.len() >= proc_area.height as usize {
                    break;
                }
                let mut name = proc.name.clone();
                if name.len() > max_name_width {
                    let trunc_to = max_name_width.saturating_sub(1);
                    // Find a valid UTF-8 char boundary to avoid panic
                    let boundary = name
                        .char_indices()
                        .take_while(|&(i, _)| i <= trunc_to)
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    name.truncate(boundary);
                    name.push('\u{2026}');
                }
                let line = Line::from(vec![
                    Span::styled(format!("{:>3} ", i + 1), theme.label),
                    Span::styled(
                        format!("{name:<width$}", width = max_name_width),
                        theme.value,
                    ),
                    Span::styled(
                        format!(" {:>8}", format_bytes(proc.memory_bytes)),
                        theme.value,
                    ),
                ]);
                lines.push(line);
            }

            frame.render_widget(Paragraph::new(lines), proc_area);
        }
    }
}

impl Component for MemoryWidget {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        "Memory"
    }
    fn update(&mut self) -> Result<Option<Action>> {
        Ok(None)
    }

    fn handle_data(&mut self, update: &DataUpdate) -> Result<Option<Action>> {
        if let DataUpdate::Memory(data) = update {
            self.total_mem = data.total_mem;
            self.used_mem = data.used_mem;
            self.total_swap = data.total_swap;
            self.used_swap = data.used_swap;
            self.top_processes = data.top_processes.clone();
            self.top_processes.truncate(self.config.process_count);
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
        if self.config.show_processes {
            (20, 10)
        } else {
            (20, 4)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::MemoryData;

    #[test]
    fn default_config_values() {
        let w = MemoryWidget::new("memory".into(), None).unwrap();
        assert!(w.config.show_swap);
        assert!(w.config.show_processes);
        assert_eq!(w.config.process_count, 5);
    }

    #[test]
    fn config_from_toml() {
        let val: toml::Value = toml::from_str(
            r#"
            show_swap = false
            show_processes = false
            process_count = 10
        "#,
        )
        .unwrap();
        let w = MemoryWidget::new("memory".into(), Some(val)).unwrap();
        assert!(!w.config.show_swap);
        assert!(!w.config.show_processes);
        assert_eq!(w.config.process_count, 10);
    }

    #[test]
    fn unknown_field_errors() {
        let val: toml::Value = toml::from_str(r#"bogus = true"#).unwrap();
        assert!(MemoryWidget::new("memory".into(), Some(val)).is_err());
    }

    #[test]
    fn handle_data_updates_state() {
        let mut w = MemoryWidget::new("memory".into(), None).unwrap();
        let update = DataUpdate::Memory(MemoryData {
            total_mem: 16_000_000_000,
            used_mem: 8_000_000_000,
            total_swap: 4_000_000_000,
            used_swap: 1_000_000_000,
            top_processes: vec![
                ProcessInfo {
                    name: "firefox".into(),
                    pid: 1,
                    memory_bytes: 500_000_000,
                },
                ProcessInfo {
                    name: "chrome".into(),
                    pid: 2,
                    memory_bytes: 400_000_000,
                },
            ],
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.total_mem, 16_000_000_000);
        assert_eq!(w.used_mem, 8_000_000_000);
        assert_eq!(w.top_processes.len(), 2);
    }

    #[test]
    fn handle_data_truncates_processes() {
        let val: toml::Value = toml::from_str(r#"process_count = 1"#).unwrap();
        let mut w = MemoryWidget::new("memory".into(), Some(val)).unwrap();
        let update = DataUpdate::Memory(MemoryData {
            total_mem: 1,
            used_mem: 1,
            total_swap: 0,
            used_swap: 0,
            top_processes: vec![
                ProcessInfo {
                    name: "a".into(),
                    pid: 1,
                    memory_bytes: 100,
                },
                ProcessInfo {
                    name: "b".into(),
                    pid: 2,
                    memory_bytes: 50,
                },
            ],
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.top_processes.len(), 1);
    }

    #[test]
    fn safe_ratio_zero_total() {
        assert_eq!(MemoryWidget::safe_ratio(100, 0), 0.0);
    }

    #[test]
    fn safe_ratio_normal() {
        let r = MemoryWidget::safe_ratio(50, 100);
        assert!((r - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn handle_data_ignores_other_variants() {
        let mut w = MemoryWidget::new("memory".into(), None).unwrap();
        let update = DataUpdate::Cpu(crate::data::types::CpuData {
            per_core: vec![50.0],
            overall: 50.0,
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.total_mem, 0);
    }
}
