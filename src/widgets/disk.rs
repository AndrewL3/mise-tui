use std::any::Any;
use std::collections::{HashMap, VecDeque};

use color_eyre::{Result, eyre::eyre};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Gauge, Paragraph, Sparkline};
use serde::Deserialize;

use crate::action::Action;
use crate::component::Component;
use crate::data::DataUpdate;
use crate::data::types::DiskInfo;
use crate::event::Event;
use crate::theme::Theme;
use crate::widgets::util::{format_bytes, format_throughput, push_capped_f64};

fn default_mode() -> String {
    "capacity".to_string()
}
fn default_mounts() -> Vec<String> {
    vec!["auto".to_string()]
}
fn default_history_length() -> usize {
    60
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiskConfig {
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_mounts")]
    pub mounts: Vec<String>,
    #[serde(default = "default_history_length")]
    pub history_length: usize,
}

impl Default for DiskConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            mounts: default_mounts(),
            history_length: default_history_length(),
        }
    }
}

pub struct DiskWidget {
    id: String,
    pub(crate) config: DiskConfig,
    pub(crate) disks: Vec<DiskInfo>,
    pub(crate) io_history: HashMap<String, (VecDeque<f64>, VecDeque<f64>)>,
}

impl DiskWidget {
    pub fn new(id: String, config: Option<toml::Value>) -> Result<Self> {
        let config: DiskConfig = match config {
            Some(value) => value.try_into()?,
            None => DiskConfig::default(),
        };
        match config.mode.as_str() {
            "capacity" | "io" | "both" => {}
            other => return Err(eyre!("invalid disk mode: '{other}'")),
        }
        if config.history_length == 0 {
            return Err(eyre!("history_length must be > 0"));
        }
        Ok(Self {
            id,
            config,
            disks: Vec::new(),
            io_history: HashMap::new(),
        })
    }

    fn filter_disks(&self, all_disks: &[DiskInfo]) -> Vec<DiskInfo> {
        let is_auto = self.config.mounts.len() == 1 && self.config.mounts[0] == "auto";
        if is_auto {
            all_disks.to_vec()
        } else {
            all_disks
                .iter()
                .filter(|d| self.config.mounts.contains(&d.mount_point))
                .cloned()
                .collect()
        }
    }

    fn gauge_style(theme: &Theme) -> Style {
        Style::new()
            .fg(theme.gauge_fill.fg.unwrap_or_default())
            .bg(theme.gauge_bg.fg.unwrap_or_default())
    }

    fn draw_content(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Minimal: just text label
        if area.height < 3 || area.width < 15 {
            let label = if self.disks.is_empty() {
                "Disk".to_string()
            } else {
                let d = &self.disks[0];
                let used = d.total_bytes.saturating_sub(d.available_bytes);
                let pct = if d.total_bytes > 0 {
                    (used as f64 / d.total_bytes as f64 * 100.0) as u64
                } else {
                    0
                };
                format!("{}: {}%", d.mount_point, pct)
            };
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(label, theme.value))),
                area,
            );
            return;
        }

        // Medium: capacity gauges only (height < 8)
        if area.height < 8 {
            self.draw_capacity_gauges(frame, area, theme);
            return;
        }

        // Full: based on mode
        match self.config.mode.as_str() {
            "capacity" => self.draw_capacity_gauges(frame, area, theme),
            "io" => self.draw_io(frame, area, theme),
            "both" => self.draw_both(frame, area, theme),
            _ => {}
        }
    }

    fn draw_capacity_gauges(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.disks.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "Waiting for data\u{2026}",
                    theme.label,
                ))),
                area,
            );
            return;
        }

        // Each disk gets a label line + gauge line = 2 rows
        let max_disks = (area.height as usize) / 2;
        let visible = self.disks.len().min(max_disks.max(1));

        let mut constraints: Vec<Constraint> = Vec::new();
        for _ in 0..visible {
            constraints.push(Constraint::Length(1)); // label
            constraints.push(Constraint::Length(1)); // gauge
        }
        if self.disks.len() > visible {
            constraints.push(Constraint::Length(1)); // overflow
        }
        constraints.push(Constraint::Min(0)); // absorb remainder

        let areas = Layout::vertical(constraints).split(area);
        let mut area_idx = 0;

        for disk in self.disks.iter().take(visible) {
            let used = disk.total_bytes.saturating_sub(disk.available_bytes);
            let ratio = if disk.total_bytes > 0 {
                (used as f64 / disk.total_bytes as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };

            let label = format!(
                "{} ({}) {}/{}",
                disk.mount_point,
                disk.device_name,
                format_bytes(used),
                format_bytes(disk.total_bytes),
            );
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(label, theme.label))),
                areas[area_idx],
            );
            area_idx += 1;

            let gauge_label = format!("{:.0}%", ratio * 100.0);
            let gauge = Gauge::default()
                .gauge_style(Self::gauge_style(theme))
                .ratio(ratio)
                .label(gauge_label)
                .use_unicode(true);
            frame.render_widget(gauge, areas[area_idx]);
            area_idx += 1;
        }

        if self.disks.len() > visible {
            let overflow = self.disks.len() - visible;
            let msg = format!("...+{overflow} more");
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(msg, theme.label))),
                areas[area_idx],
            );
        }
    }

    fn draw_io(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.disks.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "Waiting for data\u{2026}",
                    theme.label,
                ))),
                area,
            );
            return;
        }

        // Each disk: label (1) + read sparkline (1) + write sparkline (1) = 3 rows
        let rows_per_disk = 3;
        let max_disks = (area.height as usize) / rows_per_disk;
        let visible = self.disks.len().min(max_disks.max(1));

        let mut constraints: Vec<Constraint> = Vec::new();
        for _ in 0..visible {
            constraints.push(Constraint::Length(1)); // label
            constraints.push(Constraint::Length(1)); // read sparkline
            constraints.push(Constraint::Length(1)); // write sparkline
        }
        constraints.push(Constraint::Min(0));

        let areas = Layout::vertical(constraints).split(area);
        let mut area_idx = 0;

        for disk in self.disks.iter().take(visible) {
            if let Some(io) = &disk.io {
                let label = format!(
                    "{} R:{} W:{}",
                    disk.mount_point,
                    format_throughput(io.read_bytes_per_sec, "auto"),
                    format_throughput(io.write_bytes_per_sec, "auto"),
                );
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(label, theme.label))),
                    areas[area_idx],
                );
                area_idx += 1;

                if let Some((read_hist, write_hist)) = self.io_history.get_mut(&disk.mount_point) {
                    let read_u64: Vec<u64> = read_hist.iter().map(|&v| v as u64).collect();
                    frame.render_widget(
                        Sparkline::default().data(&read_u64).style(theme.sparkline),
                        areas[area_idx],
                    );
                    area_idx += 1;

                    let write_u64: Vec<u64> = write_hist.iter().map(|&v| v as u64).collect();
                    frame.render_widget(
                        Sparkline::default()
                            .data(&write_u64)
                            .style(theme.gauge_fill),
                        areas[area_idx],
                    );
                    area_idx += 1;
                } else {
                    area_idx += 2;
                }
            } else {
                let label = format!("{}: I/O unavailable", disk.mount_point);
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(label, theme.warning))),
                    areas[area_idx],
                );
                area_idx += 3; // skip label + both sparkline rows
            }
        }
    }

    fn draw_both(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.disks.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "Waiting for data\u{2026}",
                    theme.label,
                ))),
                area,
            );
            return;
        }

        // Split area: top half for capacity, bottom half for I/O
        let [cap_area, io_area] =
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);

        self.draw_capacity_gauges(frame, cap_area, theme);
        self.draw_io(frame, io_area, theme);
    }
}

impl Component for DiskWidget {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        "Disk"
    }
    fn widget_type(&self) -> &str {
        "disk"
    }
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn update(&mut self) -> Result<Option<Action>> {
        Ok(None)
    }

    fn handle_data(&mut self, update: &DataUpdate) -> Result<Option<Action>> {
        if let DataUpdate::Disk(data) = update {
            self.disks = self.filter_disks(&data.disks);

            for disk in &self.disks {
                if let Some(io) = &disk.io {
                    let (read_hist, write_hist) = self
                        .io_history
                        .entry(disk.mount_point.clone())
                        .or_insert_with(|| (VecDeque::new(), VecDeque::new()));
                    push_capped_f64(read_hist, io.read_bytes_per_sec, self.config.history_length);
                    push_capped_f64(
                        write_hist,
                        io.write_bytes_per_sec,
                        self.config.history_length,
                    );
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
        if let Some(old_disk) = old.as_any().downcast_ref::<DiskWidget>() {
            self.io_history = old_disk.io_history.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{DiskData, DiskIoStats};

    #[test]
    fn default_config_values() {
        let w = DiskWidget::new("disk".into(), None).unwrap();
        assert_eq!(w.config.mode, "capacity");
        assert_eq!(w.config.history_length, 60);
    }

    #[test]
    fn config_from_toml() {
        let val: toml::Value = toml::from_str(
            r#"
            mode = "both"
            mounts = ["/", "/home"]
            history_length = 30
        "#,
        )
        .unwrap();
        let w = DiskWidget::new("disk".into(), Some(val)).unwrap();
        assert_eq!(w.config.mode, "both");
        assert_eq!(w.config.mounts, vec!["/", "/home"]);
    }

    #[test]
    fn invalid_mode_errors() {
        let val: toml::Value = toml::from_str(r#"mode = "invalid""#).unwrap();
        assert!(DiskWidget::new("disk".into(), Some(val)).is_err());
    }

    #[test]
    fn unknown_field_errors() {
        let val: toml::Value = toml::from_str(r#"bogus = true"#).unwrap();
        assert!(DiskWidget::new("disk".into(), Some(val)).is_err());
    }

    #[test]
    fn handle_data_updates_state() {
        let mut w = DiskWidget::new("disk".into(), None).unwrap();
        let update = DataUpdate::Disk(DiskData {
            disks: vec![DiskInfo {
                mount_point: "/".into(),
                device_name: "sda1".into(),
                total_bytes: 500_000_000_000,
                available_bytes: 200_000_000_000,
                io: Some(DiskIoStats {
                    read_bytes_per_sec: 1_000_000.0,
                    write_bytes_per_sec: 500_000.0,
                }),
            }],
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.disks.len(), 1);
        assert_eq!(w.io_history.len(), 1);
    }

    #[test]
    fn handle_data_filters_mounts() {
        let val: toml::Value = toml::from_str(r#"mounts = ["/home"]"#).unwrap();
        let mut w = DiskWidget::new("disk".into(), Some(val)).unwrap();
        let update = DataUpdate::Disk(DiskData {
            disks: vec![
                DiskInfo {
                    mount_point: "/".into(),
                    device_name: "sda1".into(),
                    total_bytes: 100,
                    available_bytes: 50,
                    io: None,
                },
                DiskInfo {
                    mount_point: "/home".into(),
                    device_name: "sda2".into(),
                    total_bytes: 200,
                    available_bytes: 100,
                    io: None,
                },
            ],
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.disks.len(), 1);
        assert_eq!(w.disks[0].mount_point, "/home");
    }

    #[test]
    fn handle_data_ignores_other_variants() {
        let mut w = DiskWidget::new("disk".into(), None).unwrap();
        let update = DataUpdate::Cpu(crate::data::types::CpuData {
            per_core: vec![50.0],
            overall: 50.0,
        });
        w.handle_data(&update).unwrap();
        assert!(w.disks.is_empty());
    }

    #[test]
    fn io_history_capped() {
        let val: toml::Value = toml::from_str(r#"history_length = 3"#).unwrap();
        let mut w = DiskWidget::new("disk".into(), Some(val)).unwrap();
        let update = DataUpdate::Disk(DiskData {
            disks: vec![DiskInfo {
                mount_point: "/".into(),
                device_name: "sda1".into(),
                total_bytes: 100,
                available_bytes: 50,
                io: Some(DiskIoStats {
                    read_bytes_per_sec: 100.0,
                    write_bytes_per_sec: 200.0,
                }),
            }],
        });
        for _ in 0..5 {
            w.handle_data(&update).unwrap();
        }
        let (read_hist, write_hist) = w.io_history.get("/").unwrap();
        assert_eq!(read_hist.len(), 3);
        assert_eq!(write_hist.len(), 3);
    }

    #[test]
    fn transfer_state_preserves_io_history() {
        let mut old = DiskWidget::new("disk".into(), None).unwrap();
        let update = DataUpdate::Disk(DiskData {
            disks: vec![DiskInfo {
                mount_point: "/".into(),
                device_name: "sda1".into(),
                total_bytes: 100,
                available_bytes: 50,
                io: Some(DiskIoStats {
                    read_bytes_per_sec: 100.0,
                    write_bytes_per_sec: 200.0,
                }),
            }],
        });
        old.handle_data(&update).unwrap();
        let mut new_w = DiskWidget::new("disk".into(), None).unwrap();
        assert!(new_w.io_history.is_empty());
        new_w.transfer_state(&old);
        assert_eq!(new_w.io_history.len(), 1);
    }

    #[test]
    fn draw_does_not_panic_at_small_sizes() {
        let mut w = DiskWidget::new("disk".into(), None).unwrap();
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
