use std::any::Any;

use color_eyre::{Result, eyre::eyre};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Gauge, Paragraph};
use serde::Deserialize;

use crate::action::Action;
use crate::component::Component;
use crate::data::DataUpdate;
use crate::data::types::SensorData;
use crate::event::Event;
use crate::theme::Theme;

fn default_mode() -> String {
    "gauges".to_string()
}
fn default_warn() -> f32 {
    70.0
}
fn default_crit() -> f32 {
    85.0
}
fn default_sensors() -> Vec<String> {
    vec!["auto".to_string()]
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TempsConfig {
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default = "default_warn")]
    warn_threshold: f32,
    #[serde(default = "default_crit")]
    crit_threshold: f32,
    #[serde(default = "default_sensors")]
    sensors: Vec<String>,
}

impl Default for TempsConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            warn_threshold: default_warn(),
            crit_threshold: default_crit(),
            sensors: default_sensors(),
        }
    }
}

pub struct TempsWidget {
    id: String,
    config: TempsConfig,
    sensors: Vec<SensorData>,
}

impl TempsWidget {
    pub fn new(id: String, config: Option<toml::Value>) -> Result<Self> {
        let config: TempsConfig = match config {
            Some(value) => value.try_into()?,
            None => TempsConfig::default(),
        };
        match config.mode.as_str() {
            "gauges" | "compact" => {}
            other => return Err(eyre!("invalid temps mode: '{other}'")),
        }
        if config.crit_threshold <= 0.0 {
            return Err(eyre!("crit_threshold must be > 0"));
        }
        Ok(Self {
            id,
            config,
            sensors: Vec::new(),
        })
    }

    fn filter_sensors(&self, sensors: &[SensorData]) -> Vec<SensorData> {
        if self.config.sensors.iter().any(|s| s == "auto") {
            sensors.to_vec()
        } else {
            sensors
                .iter()
                .filter(|s| {
                    let label_lower = s.label.to_lowercase();
                    self.config
                        .sensors
                        .iter()
                        .any(|f| label_lower.contains(&f.to_lowercase()))
                })
                .cloned()
                .collect()
        }
    }

    fn temp_style(&self, temp: Option<f32>, theme: &Theme) -> Style {
        match temp {
            None => theme.label,
            Some(t) if t >= self.config.crit_threshold => theme.critical,
            Some(t) if t >= self.config.warn_threshold => theme.warning,
            Some(_) => theme.ok,
        }
    }

    fn draw_content(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.sensors.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled("No sensor data", theme.label))),
                area,
            );
            return;
        }

        // Minimal: single hottest sensor text
        if area.height < 3 || area.width < 15 {
            if let Some(sensor) = self.sensors.first() {
                let temp_str = match sensor.temp_celsius {
                    Some(t) => format!("{:.0}\u{00B0}C", t),
                    None => "N/A".to_string(),
                };
                let style = self.temp_style(sensor.temp_celsius, theme);
                let label = format!("{}: {}", sensor.label, temp_str);
                frame.render_widget(Paragraph::new(Line::from(Span::styled(label, style))), area);
            }
            return;
        }

        // Medium: top sensors text only (when too small for gauges)
        if area.height < 6 || self.config.mode == "compact" {
            self.draw_compact(frame, area, theme);
            return;
        }

        // Full: gauges
        self.draw_gauges(frame, area, theme);
    }

    fn draw_gauges(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let max_sensors = (area.height as usize / 2).max(1);
        let visible = self.sensors.len().min(max_sensors);

        let constraints: Vec<Constraint> = (0..visible).map(|_| Constraint::Length(2)).collect();
        let areas = Layout::vertical(constraints).split(area);

        for (i, sensor) in self.sensors.iter().take(visible).enumerate() {
            let critical_ceil = sensor
                .critical_celsius
                .unwrap_or(self.config.crit_threshold);

            let (ratio, label) = match sensor.temp_celsius {
                Some(temp) => {
                    let r = (temp as f64 / critical_ceil as f64).clamp(0.0, 1.0);
                    (r, format!("{}: {:.0}\u{00B0}C", sensor.label, temp))
                }
                None => (0.0, format!("{}: N/A", sensor.label)),
            };

            let gauge_style = Style::new()
                .fg(self
                    .temp_style(sensor.temp_celsius, theme)
                    .fg
                    .unwrap_or_default())
                .bg(theme.gauge_bg.fg.unwrap_or_default());

            let gauge = Gauge::default()
                .gauge_style(gauge_style)
                .ratio(ratio)
                .label(label)
                .use_unicode(true);

            frame.render_widget(gauge, areas[i]);
        }
    }

    fn draw_compact(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let max_sensors = area.height as usize;
        let mut lines = Vec::new();

        for sensor in self.sensors.iter().take(max_sensors) {
            let (temp_str, style) = match sensor.temp_celsius {
                Some(temp) => (
                    format!("{:.0}\u{00B0}C", temp),
                    self.temp_style(Some(temp), theme),
                ),
                None => ("N/A".to_string(), theme.label),
            };

            let line = Line::from(vec![
                Span::styled(format!("{:<20}", sensor.label), theme.value),
                Span::styled(format!("{:>6}  ", temp_str), theme.value),
                Span::styled("\u{2588}\u{2588}\u{2588}", style),
            ]);
            lines.push(line);
        }

        frame.render_widget(Paragraph::new(lines), area);
    }
}

impl Component for TempsWidget {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        "Temps"
    }
    fn widget_type(&self) -> &str {
        "temps"
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn update(&mut self) -> Result<Option<Action>> {
        Ok(None)
    }

    fn handle_data(&mut self, update: &DataUpdate) -> Result<Option<Action>> {
        if let DataUpdate::Temps(data) = update {
            self.sensors = self.filter_sensors(&data.sensors);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::TempData;

    #[test]
    fn default_config_values() {
        let w = TempsWidget::new("temps".into(), None).unwrap();
        assert_eq!(w.config.mode, "gauges");
        assert!((w.config.warn_threshold - 70.0).abs() < f32::EPSILON);
        assert!((w.config.crit_threshold - 85.0).abs() < f32::EPSILON);
        assert_eq!(w.config.sensors, vec!["auto"]);
    }

    #[test]
    fn config_from_toml() {
        let val: toml::Value = toml::from_str(
            r#"
            mode = "compact"
            warn_threshold = 60
            crit_threshold = 90
            sensors = ["CPU"]
        "#,
        )
        .unwrap();
        let w = TempsWidget::new("temps".into(), Some(val)).unwrap();
        assert_eq!(w.config.mode, "compact");
        assert_eq!(w.config.sensors, vec!["CPU"]);
    }

    #[test]
    fn invalid_mode_errors() {
        let val: toml::Value = toml::from_str(r#"mode = "invalid""#).unwrap();
        assert!(TempsWidget::new("temps".into(), Some(val)).is_err());
    }

    #[test]
    fn zero_crit_threshold_errors() {
        let val: toml::Value = toml::from_str(r#"crit_threshold = 0"#).unwrap();
        assert!(TempsWidget::new("temps".into(), Some(val)).is_err());
    }

    #[test]
    fn unknown_field_errors() {
        let val: toml::Value = toml::from_str(r#"bogus = true"#).unwrap();
        assert!(TempsWidget::new("temps".into(), Some(val)).is_err());
    }

    #[test]
    fn filter_sensors_auto_returns_all() {
        let w = TempsWidget::new("temps".into(), None).unwrap();
        let sensors = vec![
            SensorData {
                label: "CPU".into(),
                temp_celsius: Some(50.0),
                max_celsius: None,
                critical_celsius: None,
            },
            SensorData {
                label: "GPU".into(),
                temp_celsius: Some(60.0),
                max_celsius: None,
                critical_celsius: None,
            },
        ];
        assert_eq!(w.filter_sensors(&sensors).len(), 2);
    }

    #[test]
    fn filter_sensors_specific_label_substring() {
        let val: toml::Value = toml::from_str(r#"sensors = ["CPU"]"#).unwrap();
        let w = TempsWidget::new("temps".into(), Some(val)).unwrap();
        let sensors = vec![
            SensorData {
                label: "CPU Package".into(),
                temp_celsius: Some(50.0),
                max_celsius: None,
                critical_celsius: None,
            },
            SensorData {
                label: "GPU Core".into(),
                temp_celsius: Some(60.0),
                max_celsius: None,
                critical_celsius: None,
            },
        ];
        let filtered = w.filter_sensors(&sensors);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].label, "CPU Package");
    }

    #[test]
    fn filter_sensors_case_insensitive() {
        let val: toml::Value = toml::from_str(r#"sensors = ["cpu"]"#).unwrap();
        let w = TempsWidget::new("temps".into(), Some(val)).unwrap();
        let sensors = vec![SensorData {
            label: "CPU Package".into(),
            temp_celsius: Some(50.0),
            max_celsius: None,
            critical_celsius: None,
        }];
        assert_eq!(w.filter_sensors(&sensors).len(), 1);
    }

    #[test]
    fn handle_data_updates_sensors() {
        let mut w = TempsWidget::new("temps".into(), None).unwrap();
        let update = DataUpdate::Temps(TempData {
            sensors: vec![SensorData {
                label: "CPU".into(),
                temp_celsius: Some(55.0),
                max_celsius: Some(100.0),
                critical_celsius: Some(105.0),
            }],
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.sensors.len(), 1);
    }

    #[test]
    fn handle_data_ignores_other_variants() {
        let mut w = TempsWidget::new("temps".into(), None).unwrap();
        let update = DataUpdate::Cpu(crate::data::types::CpuData {
            per_core: vec![50.0],
            overall: 50.0,
        });
        w.handle_data(&update).unwrap();
        assert!(w.sensors.is_empty());
    }

    #[test]
    fn temp_style_thresholds() {
        let w = TempsWidget::new("temps".into(), None).unwrap();
        let theme = Theme::default();
        assert_eq!(w.temp_style(Some(50.0), &theme).fg, theme.ok.fg);
        assert_eq!(w.temp_style(Some(75.0), &theme).fg, theme.warning.fg);
        assert_eq!(w.temp_style(Some(85.0), &theme).fg, theme.critical.fg);
        assert_eq!(w.temp_style(None, &theme).fg, theme.label.fg);
    }

    #[test]
    fn gauge_ratio_clamped_when_temp_exceeds_critical() {
        let w = TempsWidget::new("temps".into(), None).unwrap();
        let sensor = SensorData {
            label: "Hot".into(),
            temp_celsius: Some(200.0),
            max_celsius: None,
            critical_celsius: Some(100.0),
        };
        let critical_ceil = sensor.critical_celsius.unwrap_or(w.config.crit_threshold);
        let ratio = (200.0_f64 / critical_ceil as f64).clamp(0.0, 1.0);
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn handle_data_with_none_temp_no_panic() {
        let mut w = TempsWidget::new("temps".into(), None).unwrap();
        let update = DataUpdate::Temps(TempData {
            sensors: vec![SensorData {
                label: "Dead Sensor".into(),
                temp_celsius: None,
                max_celsius: None,
                critical_celsius: None,
            }],
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.sensors.len(), 1);
        assert!(w.sensors[0].temp_celsius.is_none());
    }

    #[test]
    fn draw_does_not_panic_at_small_sizes() {
        let mut w = TempsWidget::new("temps".into(), None).unwrap();
        let update = DataUpdate::Temps(TempData {
            sensors: vec![SensorData {
                label: "CPU".into(),
                temp_celsius: Some(62.0),
                max_celsius: None,
                critical_celsius: Some(100.0),
            }],
        });
        w.handle_data(&update).unwrap();

        let backend = ratatui::backend::TestBackend::new(15, 2);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let theme = crate::theme::Theme::default();
        terminal
            .draw(|frame| {
                w.draw(frame, frame.area(), &theme);
            })
            .unwrap();
    }
}
