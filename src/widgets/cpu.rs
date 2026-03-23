use std::any::Any;
use std::collections::VecDeque;

use color_eyre::{Result, eyre::eyre};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Bar, BarChart, BarGroup, Gauge, Paragraph, Sparkline};
use serde::Deserialize;

use crate::action::Action;
use crate::component::Component;
use crate::data::DataUpdate;
use crate::event::Event;
use crate::theme::Theme;
use crate::widgets::util::push_capped;

fn default_mode() -> String {
    "sparklines".to_string()
}
fn default_history_length() -> usize {
    60
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CpuConfig {
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default = "default_history_length")]
    history_length: usize,
    #[serde(default = "default_true")]
    show_per_core: bool,
}

impl Default for CpuConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            history_length: default_history_length(),
            show_per_core: default_true(),
        }
    }
}

pub struct CpuWidget {
    id: String,
    config: CpuConfig,
    per_core: Vec<f32>,
    overall: f32,
    history: Vec<VecDeque<u64>>,
    overall_history: VecDeque<u64>,
}

impl CpuWidget {
    pub fn new(id: String, config: Option<toml::Value>) -> Result<Self> {
        let config: CpuConfig = match config {
            Some(value) => value.try_into()?,
            None => CpuConfig::default(),
        };
        match config.mode.as_str() {
            "sparklines" | "bars" | "gauge" => {}
            other => return Err(eyre!("invalid cpu mode: '{other}'")),
        }
        if config.history_length == 0 {
            return Err(eyre!("history_length must be > 0"));
        }
        Ok(Self {
            id,
            config,
            per_core: Vec::new(),
            overall: 0.0,
            history: Vec::new(),
            overall_history: VecDeque::new(),
        })
    }

    fn gauge_style(theme: &Theme) -> Style {
        Style::new()
            .fg(theme.gauge_fill.fg.unwrap_or_default())
            .bg(theme.gauge_bg.fg.unwrap_or_default())
    }

    fn draw_content(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Minimal: just text label
        if area.height < 3 || area.width < 15 {
            let label = format!("CPU: {:.0}%", self.overall);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(label, theme.value))),
                area,
            );
            return;
        }

        // Medium: single overall sparkline (when too small for full mode)
        let full_threshold = match self.config.mode.as_str() {
            "sparklines" => 6,
            _ => 4,
        };
        if area.height < full_threshold {
            let label = format!("CPU: {:.0}%", self.overall);
            let data: &[u64] = self.overall_history.make_contiguous();
            let [label_area, spark_area] =
                Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(label, theme.label))),
                label_area,
            );
            frame.render_widget(
                Sparkline::default()
                    .data(data)
                    .style(theme.sparkline)
                    .max(100),
                spark_area,
            );
            return;
        }

        // Full mode
        match self.config.mode.as_str() {
            "sparklines" => self.draw_sparklines(frame, area, theme),
            "bars" => self.draw_bars(frame, area, theme),
            "gauge" => self.draw_gauge(frame, area, theme),
            _ => {}
        }
    }

    fn draw_sparklines(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.config.show_per_core || self.per_core.is_empty() {
            let data: &[u64] = self.overall_history.make_contiguous();
            let label = format!("CPU: {:.0}%", self.overall);
            let sparkline = Sparkline::default()
                .data(data)
                .style(theme.sparkline)
                .max(100);
            let label_line = Paragraph::new(Line::from(Span::styled(label, theme.label)));
            if area.height >= 2 {
                let [label_area, spark_area] =
                    Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
                frame.render_widget(label_line, label_area);
                frame.render_widget(sparkline, spark_area);
            } else {
                frame.render_widget(sparkline, area);
            }
            return;
        }

        let max_cores = area.height as usize / 2;
        let visible = self.per_core.len().min(max_cores);
        let overflow = self.per_core.len().saturating_sub(max_cores);

        let mut constraints: Vec<Constraint> = Vec::new();
        for _ in 0..visible {
            constraints.push(Constraint::Length(1));
            constraints.push(Constraint::Length(1));
        }
        if overflow > 0 {
            constraints.push(Constraint::Length(1));
        }

        let areas = Layout::vertical(constraints).split(area);
        let mut area_idx = 0;

        for i in 0..visible {
            let usage = self.per_core.get(i).copied().unwrap_or(0.0);
            let label = format!("Core {i}: {usage:.0}%");
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(label, theme.label))),
                areas[area_idx],
            );
            area_idx += 1;

            if let Some(buf) = self.history.get_mut(i) {
                let data: &[u64] = buf.make_contiguous();
                let sparkline = Sparkline::default()
                    .data(data)
                    .style(theme.sparkline)
                    .max(100);
                frame.render_widget(sparkline, areas[area_idx]);
            }
            area_idx += 1;
        }

        if overflow > 0 {
            let msg = format!("...+{overflow} more");
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(msg, theme.label))),
                areas[area_idx],
            );
        }
    }

    fn draw_bars(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let bars: Vec<Bar> = if self.config.show_per_core && !self.per_core.is_empty() {
            self.per_core
                .iter()
                .enumerate()
                .map(|(i, &usage)| {
                    Bar::default()
                        .value(usage as u64)
                        .label(format!("C{i}"))
                        .style(theme.gauge_fill)
                })
                .collect()
        } else {
            vec![
                Bar::default()
                    .value(self.overall as u64)
                    .label("CPU")
                    .style(theme.gauge_fill),
            ]
        };

        let group = BarGroup::default().bars(&bars);
        let chart = BarChart::default()
            .data(group)
            .bar_width(3)
            .bar_gap(1)
            .max(100);

        frame.render_widget(chart, area);
    }

    fn draw_gauge(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let ratio = (self.overall as f64 / 100.0).clamp(0.0, 1.0);
        let label = format!("{:.0}%", self.overall);
        let gauge = Gauge::default()
            .gauge_style(Self::gauge_style(theme))
            .ratio(ratio)
            .label(label)
            .use_unicode(true);

        if self.config.show_per_core && !self.per_core.is_empty() && area.height >= 3 {
            let [gauge_area, summary_area] =
                Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(area);
            frame.render_widget(gauge, gauge_area);

            let summary: String = self
                .per_core
                .iter()
                .enumerate()
                .map(|(i, &u)| format!("C{i}:{u:.0}%"))
                .collect::<Vec<_>>()
                .join("  ");
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(summary, theme.label))),
                summary_area,
            );
        } else {
            frame.render_widget(gauge, area);
        }
    }
}

impl Component for CpuWidget {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        "CPU"
    }
    fn widget_type(&self) -> &str {
        "cpu"
    }
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn update(&mut self) -> Result<Option<Action>> {
        Ok(None)
    }

    fn handle_data(&mut self, update: &DataUpdate) -> Result<Option<Action>> {
        if let DataUpdate::Cpu(data) = update {
            self.per_core = data.per_core.clone();
            self.overall = data.overall;

            if self.history.len() != data.per_core.len() {
                self.history = vec![VecDeque::new(); data.per_core.len()];
            }

            for (i, &usage) in data.per_core.iter().enumerate() {
                if let Some(buf) = self.history.get_mut(i) {
                    push_capped(buf, usage as u64, self.config.history_length);
                }
            }

            push_capped(
                &mut self.overall_history,
                data.overall as u64,
                self.config.history_length,
            );
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
        if let Some(old_cpu) = old.as_any().downcast_ref::<CpuWidget>() {
            self.per_core = old_cpu.per_core.clone();
            self.overall = old_cpu.overall;
            self.history = old_cpu.history.clone();
            self.overall_history = old_cpu.overall_history.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let w = CpuWidget::new("cpu".into(), None).unwrap();
        assert_eq!(w.config.mode, "sparklines");
        assert_eq!(w.config.history_length, 60);
        assert!(w.config.show_per_core);
    }

    #[test]
    fn config_from_toml() {
        let val: toml::Value = toml::from_str(
            r#"
            mode = "bars"
            history_length = 30
            show_per_core = false
        "#,
        )
        .unwrap();
        let w = CpuWidget::new("cpu".into(), Some(val)).unwrap();
        assert_eq!(w.config.mode, "bars");
        assert_eq!(w.config.history_length, 30);
        assert!(!w.config.show_per_core);
    }

    #[test]
    fn invalid_mode_errors() {
        let val: toml::Value = toml::from_str(r#"mode = "invalid""#).unwrap();
        assert!(CpuWidget::new("cpu".into(), Some(val)).is_err());
    }

    #[test]
    fn unknown_field_errors() {
        let val: toml::Value = toml::from_str(r#"bogus = true"#).unwrap();
        assert!(CpuWidget::new("cpu".into(), Some(val)).is_err());
    }

    #[test]
    fn zero_history_length_errors() {
        let val: toml::Value = toml::from_str(r#"history_length = 0"#).unwrap();
        assert!(CpuWidget::new("cpu".into(), Some(val)).is_err());
    }

    #[test]
    fn handle_data_updates_state() {
        let mut w = CpuWidget::new("cpu".into(), None).unwrap();
        let update = DataUpdate::Cpu(crate::data::types::CpuData {
            per_core: vec![25.0, 50.0, 75.0, 100.0],
            overall: 62.5,
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.per_core.len(), 4);
        assert!((w.overall - 62.5).abs() < f32::EPSILON);
        assert_eq!(w.history.len(), 4);
        assert_eq!(w.overall_history.len(), 1);
    }

    #[test]
    fn handle_data_caps_history() {
        let val: toml::Value = toml::from_str(r#"history_length = 3"#).unwrap();
        let mut w = CpuWidget::new("cpu".into(), Some(val)).unwrap();
        let update = DataUpdate::Cpu(crate::data::types::CpuData {
            per_core: vec![10.0],
            overall: 10.0,
        });
        for _ in 0..5 {
            w.handle_data(&update).unwrap();
        }
        assert_eq!(w.history[0].len(), 3);
        assert_eq!(w.overall_history.len(), 3);
    }

    #[test]
    fn handle_data_ignores_other_variants() {
        let mut w = CpuWidget::new("cpu".into(), None).unwrap();
        let update = DataUpdate::Memory(crate::data::types::MemoryData {
            total_mem: 0,
            used_mem: 0,
            total_swap: 0,
            used_swap: 0,
            top_processes: vec![],
        });
        w.handle_data(&update).unwrap();
        assert!(w.per_core.is_empty());
    }

    #[test]
    fn transfer_state_preserves_history() {
        let mut old = CpuWidget::new("cpu".into(), None).unwrap();
        let update = DataUpdate::Cpu(crate::data::types::CpuData {
            per_core: vec![25.0, 50.0],
            overall: 37.5,
        });
        old.handle_data(&update).unwrap();
        let mut new = CpuWidget::new("cpu".into(), None).unwrap();
        assert!(new.history.is_empty());
        new.transfer_state(&old);
        assert_eq!(new.history.len(), 2);
        assert_eq!(new.overall_history.len(), 1);
        assert!((new.overall - 37.5).abs() < f32::EPSILON);
    }

    #[test]
    fn draw_does_not_panic_at_small_sizes() {
        let mut w = CpuWidget::new("cpu".into(), None).unwrap();
        let update = DataUpdate::Cpu(crate::data::types::CpuData {
            per_core: vec![25.0, 50.0],
            overall: 37.5,
        });
        w.handle_data(&update).unwrap();

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
