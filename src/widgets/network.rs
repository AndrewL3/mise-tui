use std::collections::VecDeque;
use std::time::Instant;

use color_eyre::{Result, eyre::eyre};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Sparkline};
use serde::Deserialize;

use crate::action::Action;
use crate::component::Component;
use crate::data::DataUpdate;
use crate::data::types::InterfaceData;
use crate::event::Event;
use crate::theme::Theme;
use crate::widgets::util::{format_throughput, push_capped};

fn default_interface() -> String {
    "auto".to_string()
}
fn default_history_length() -> usize {
    60
}
fn default_true() -> bool {
    true
}
fn default_unit() -> String {
    "auto".to_string()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkConfig {
    #[serde(default = "default_interface")]
    interface: String,
    #[serde(default = "default_history_length")]
    history_length: usize,
    #[serde(default = "default_true")]
    show_peak: bool,
    #[serde(default = "default_unit")]
    unit: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            interface: default_interface(),
            history_length: default_history_length(),
            show_peak: default_true(),
            unit: default_unit(),
        }
    }
}

enum InterfaceState {
    NoData,
    NotFound,
    Active,
}

pub struct NetworkWidget {
    id: String,
    config: NetworkConfig,
    prev_sent: u64,
    prev_recv: u64,
    prev_time: Option<Instant>,
    prev_iface_names: Vec<String>,
    download_history: VecDeque<u64>,
    upload_history: VecDeque<u64>,
    current_down: f64,
    current_up: f64,
    peak_down: f64,
    peak_up: f64,
    iface_state: InterfaceState,
}

impl NetworkWidget {
    pub fn new(id: String, config: Option<toml::Value>) -> Result<Self> {
        let config: NetworkConfig = match config {
            Some(value) => value.try_into()?,
            None => NetworkConfig::default(),
        };
        match config.unit.as_str() {
            "auto" | "KB/s" | "MB/s" => {}
            other => return Err(eyre!("invalid network unit: '{other}'")),
        }
        if config.history_length == 0 {
            return Err(eyre!("history_length must be > 0"));
        }
        Ok(Self {
            id,
            config,
            prev_sent: 0,
            prev_recv: 0,
            prev_time: None,
            prev_iface_names: Vec::new(),
            download_history: VecDeque::new(),
            upload_history: VecDeque::new(),
            current_down: 0.0,
            current_up: 0.0,
            peak_down: 0.0,
            peak_up: 0.0,
            iface_state: InterfaceState::NoData,
        })
    }

    fn filter_interfaces<'a>(&self, interfaces: &'a [InterfaceData]) -> Vec<&'a InterfaceData> {
        if self.config.interface == "auto" {
            interfaces
                .iter()
                .filter(|i| !i.name.starts_with("lo"))
                .collect()
        } else {
            interfaces
                .iter()
                .filter(|i| i.name == self.config.interface)
                .collect()
        }
    }

    fn draw_content(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        match self.iface_state {
            InterfaceState::NotFound => {
                let msg = format!("Interface '{}' not found", self.config.interface);
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(msg, theme.warning))),
                    area,
                );
                return;
            }
            InterfaceState::NoData => {
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        "Waiting for data\u{2026}",
                        theme.label,
                    ))),
                    area,
                );
                return;
            }
            InterfaceState::Active => {}
        }

        let [dl_area, ul_area, stats_area] = Layout::vertical([
            Constraint::Min(2),
            Constraint::Min(2),
            Constraint::Length(1),
        ])
        .areas(area);

        // Download sparkline
        let dl_data: &[u64] = self.download_history.make_contiguous();
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("DL", theme.label))),
            Rect {
                height: 1,
                ..dl_area
            },
        );
        if dl_area.height > 1 {
            frame.render_widget(
                Sparkline::default().data(dl_data).style(theme.sparkline),
                Rect {
                    y: dl_area.y + 1,
                    height: dl_area.height - 1,
                    ..dl_area
                },
            );
        }

        // Upload sparkline
        let ul_data: &[u64] = self.upload_history.make_contiguous();
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("UL", theme.label))),
            Rect {
                height: 1,
                ..ul_area
            },
        );
        if ul_area.height > 1 {
            frame.render_widget(
                Sparkline::default().data(ul_data).style(theme.gauge_fill),
                Rect {
                    y: ul_area.y + 1,
                    height: ul_area.height - 1,
                    ..ul_area
                },
            );
        }

        // Stats line
        let dl_str = format_throughput(self.current_down, &self.config.unit);
        let ul_str = format_throughput(self.current_up, &self.config.unit);
        let mut spans = vec![
            Span::styled("DL: ", theme.label),
            Span::styled(dl_str, theme.value),
            Span::styled("  UL: ", theme.label),
            Span::styled(ul_str, theme.value),
        ];
        if self.config.show_peak {
            let peak_str = format_throughput(self.peak_down.max(self.peak_up), &self.config.unit);
            spans.push(Span::styled(format!("  (peak: {peak_str})"), theme.label));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), stats_area);
    }
}

impl Component for NetworkWidget {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        "Network"
    }
    fn update(&mut self) -> Result<Option<Action>> {
        Ok(None)
    }

    fn handle_data(&mut self, update: &DataUpdate) -> Result<Option<Action>> {
        if let DataUpdate::Network(data) = update {
            let filtered = self.filter_interfaces(&data.interfaces);

            if filtered.is_empty() && self.config.interface != "auto" {
                self.iface_state = InterfaceState::NotFound;
                return Ok(None);
            }

            let total_recv: u64 = filtered.iter().map(|i| i.bytes_received).sum();
            let total_sent: u64 = filtered.iter().map(|i| i.bytes_sent).sum();

            // Detect interface set changes (WiFi reconnect, VPN, Docker)
            // and reset baselines to avoid false throughput spikes.
            let mut current_names: Vec<String> = filtered.iter().map(|i| i.name.clone()).collect();
            current_names.sort();
            let iface_set_changed = current_names != self.prev_iface_names;

            if !iface_set_changed && let Some(prev_time) = self.prev_time {
                let elapsed = prev_time.elapsed().as_secs_f64();
                if elapsed > 0.0 {
                    let recv_delta = total_recv.saturating_sub(self.prev_recv);
                    let sent_delta = total_sent.saturating_sub(self.prev_sent);

                    self.current_down = recv_delta as f64 / elapsed;
                    self.current_up = sent_delta as f64 / elapsed;

                    self.peak_down = self.peak_down.max(self.current_down);
                    self.peak_up = self.peak_up.max(self.current_up);

                    push_capped(
                        &mut self.download_history,
                        self.current_down as u64,
                        self.config.history_length,
                    );
                    push_capped(
                        &mut self.upload_history,
                        self.current_up as u64,
                        self.config.history_length,
                    );
                }
            }

            self.prev_recv = total_recv;
            self.prev_sent = total_sent;
            self.prev_time = Some(Instant::now());
            self.prev_iface_names = current_names;
            self.iface_state = InterfaceState::Active;
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
        (20, 5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::NetworkData;

    #[test]
    fn default_config_values() {
        let w = NetworkWidget::new("network".into(), None).unwrap();
        assert_eq!(w.config.interface, "auto");
        assert_eq!(w.config.history_length, 60);
        assert!(w.config.show_peak);
        assert_eq!(w.config.unit, "auto");
    }

    #[test]
    fn config_from_toml() {
        let val: toml::Value = toml::from_str(
            r#"
            interface = "wlan0"
            history_length = 30
            show_peak = false
            unit = "MB/s"
        "#,
        )
        .unwrap();
        let w = NetworkWidget::new("network".into(), Some(val)).unwrap();
        assert_eq!(w.config.interface, "wlan0");
        assert!(!w.config.show_peak);
    }

    #[test]
    fn invalid_unit_errors() {
        let val: toml::Value = toml::from_str(r#"unit = "GB/s""#).unwrap();
        assert!(NetworkWidget::new("network".into(), Some(val)).is_err());
    }

    #[test]
    fn unknown_field_errors() {
        let val: toml::Value = toml::from_str(r#"bogus = true"#).unwrap();
        assert!(NetworkWidget::new("network".into(), Some(val)).is_err());
    }

    #[test]
    fn zero_history_length_errors() {
        let val: toml::Value = toml::from_str(r#"history_length = 0"#).unwrap();
        assert!(NetworkWidget::new("network".into(), Some(val)).is_err());
    }

    #[test]
    fn filter_interfaces_auto_excludes_loopback() {
        let w = NetworkWidget::new("network".into(), None).unwrap();
        let interfaces = vec![
            InterfaceData {
                name: "lo".into(),
                bytes_sent: 100,
                bytes_received: 200,
            },
            InterfaceData {
                name: "eth0".into(),
                bytes_sent: 300,
                bytes_received: 400,
            },
        ];
        let filtered = w.filter_interfaces(&interfaces);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "eth0");
    }

    #[test]
    fn filter_interfaces_specific_name() {
        let val: toml::Value = toml::from_str(r#"interface = "wlan0""#).unwrap();
        let w = NetworkWidget::new("network".into(), Some(val)).unwrap();
        let interfaces = vec![
            InterfaceData {
                name: "eth0".into(),
                bytes_sent: 100,
                bytes_received: 200,
            },
            InterfaceData {
                name: "wlan0".into(),
                bytes_sent: 300,
                bytes_received: 400,
            },
        ];
        let filtered = w.filter_interfaces(&interfaces);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "wlan0");
    }

    #[test]
    fn handle_data_first_reading_no_throughput() {
        let mut w = NetworkWidget::new("network".into(), None).unwrap();
        let update = DataUpdate::Network(NetworkData {
            interfaces: vec![InterfaceData {
                name: "eth0".into(),
                bytes_sent: 1000,
                bytes_received: 2000,
            }],
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.current_down, 0.0);
        assert_eq!(w.current_up, 0.0);
        assert!(w.download_history.is_empty());
    }

    #[test]
    fn handle_data_second_reading_computes_throughput() {
        let mut w = NetworkWidget::new("network".into(), None).unwrap();
        let update1 = DataUpdate::Network(NetworkData {
            interfaces: vec![InterfaceData {
                name: "eth0".into(),
                bytes_sent: 1000,
                bytes_received: 2000,
            }],
        });
        w.handle_data(&update1).unwrap();

        // Force prev_time to 1 second ago for deterministic test
        w.prev_time = Some(Instant::now() - std::time::Duration::from_secs(1));

        let update2 = DataUpdate::Network(NetworkData {
            interfaces: vec![InterfaceData {
                name: "eth0".into(),
                bytes_sent: 2000,
                bytes_received: 3000,
            }],
        });
        w.handle_data(&update2).unwrap();

        assert!(w.current_down > 900.0 && w.current_down < 1100.0);
        assert!(w.current_up > 900.0 && w.current_up < 1100.0);
        assert_eq!(w.download_history.len(), 1);
    }

    #[test]
    fn handle_data_ignores_other_variants() {
        let mut w = NetworkWidget::new("network".into(), None).unwrap();
        let update = DataUpdate::Cpu(crate::data::types::CpuData {
            per_core: vec![50.0],
            overall: 50.0,
        });
        w.handle_data(&update).unwrap();
        assert!(w.prev_time.is_none());
    }

    #[test]
    fn interface_set_change_resets_baseline() {
        let mut w = NetworkWidget::new("network".into(), None).unwrap();

        // First reading: eth0 only
        let update1 = DataUpdate::Network(NetworkData {
            interfaces: vec![InterfaceData {
                name: "eth0".into(),
                bytes_sent: 1000,
                bytes_received: 2000,
            }],
        });
        w.handle_data(&update1).unwrap();
        w.prev_time = Some(Instant::now() - std::time::Duration::from_secs(1));

        // Second reading: eth0 + wlan0 appears (VPN/WiFi) — interface set changed
        let update2 = DataUpdate::Network(NetworkData {
            interfaces: vec![
                InterfaceData {
                    name: "eth0".into(),
                    bytes_sent: 2000,
                    bytes_received: 3000,
                },
                InterfaceData {
                    name: "wlan0".into(),
                    bytes_sent: 50000,
                    bytes_received: 100000,
                },
            ],
        });
        w.handle_data(&update2).unwrap();

        // Should NOT have computed throughput (baseline reset, no spike)
        assert_eq!(w.current_down, 0.0);
        assert_eq!(w.current_up, 0.0);
        assert!(w.download_history.is_empty());
    }
}
