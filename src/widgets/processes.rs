use std::any::Any;

use color_eyre::{Result, eyre::eyre};
use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};
use serde::Deserialize;

use crate::action::Action;
use crate::component::Component;
use crate::data::DataUpdate;
use crate::data::types::{ProcessEntry, ProcessStatus};
use crate::event::Event;
use crate::theme::Theme;
use crate::widgets::util::format_bytes;

fn default_sort_by() -> String {
    "cpu".to_string()
}
fn default_sort_order() -> String {
    "desc".to_string()
}
fn default_count() -> usize {
    50
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessConfig {
    #[serde(default = "default_sort_by")]
    sort_by: String,
    #[serde(default = "default_sort_order")]
    sort_order: String,
    #[serde(default = "default_count")]
    count: usize,
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            sort_by: default_sort_by(),
            sort_order: default_sort_order(),
            count: default_count(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SortColumn {
    Cpu,
    Memory,
    Pid,
    Name,
}

impl SortColumn {
    fn next(self) -> Self {
        match self {
            SortColumn::Cpu => SortColumn::Memory,
            SortColumn::Memory => SortColumn::Pid,
            SortColumn::Pid => SortColumn::Name,
            SortColumn::Name => SortColumn::Cpu,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SortColumn::Cpu => "CPU%",
            SortColumn::Memory => "MEM",
            SortColumn::Pid => "PID",
            SortColumn::Name => "Name",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    fn toggle(self) -> Self {
        match self {
            SortOrder::Asc => SortOrder::Desc,
            SortOrder::Desc => SortOrder::Asc,
        }
    }
}

struct SignalConfirm {
    pid: u32,
    name: String,
    result: Option<std::result::Result<(), String>>,
}

pub struct ProcessWidget {
    id: String,
    config: ProcessConfig,
    processes: Vec<ProcessEntry>,
    scroll_offset: usize,
    selected_index: usize,
    sort_by: SortColumn,
    sort_order: SortOrder,
    interacting: bool,
    signal_confirm: Option<SignalConfirm>,
    last_area_height: u16,
}

impl ProcessWidget {
    pub fn new(id: String, config: Option<toml::Value>) -> Result<Self> {
        let config: ProcessConfig = match config {
            Some(value) => value.try_into()?,
            None => ProcessConfig::default(),
        };

        let sort_by = match config.sort_by.as_str() {
            "cpu" => SortColumn::Cpu,
            "memory" => SortColumn::Memory,
            "pid" => SortColumn::Pid,
            "name" => SortColumn::Name,
            other => return Err(eyre!("invalid sort_by: '{other}'")),
        };

        let sort_order = match config.sort_order.as_str() {
            "asc" => SortOrder::Asc,
            "desc" => SortOrder::Desc,
            other => return Err(eyre!("invalid sort_order: '{other}'")),
        };

        if config.count == 0 {
            return Err(eyre!("count must be > 0"));
        }

        Ok(Self {
            id,
            config,
            processes: Vec::new(),
            scroll_offset: 0,
            selected_index: 0,
            sort_by,
            sort_order,
            interacting: false,
            signal_confirm: None,
            last_area_height: 0,
        })
    }

    fn sort_processes(&self, procs: &mut [ProcessEntry]) {
        procs.sort_by(|a, b| {
            let cmp = match self.sort_by {
                SortColumn::Cpu => a
                    .cpu_percent
                    .partial_cmp(&b.cpu_percent)
                    .unwrap_or(std::cmp::Ordering::Equal),
                SortColumn::Memory => a.memory_bytes.cmp(&b.memory_bytes),
                SortColumn::Pid => a.pid.cmp(&b.pid),
                SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            };
            match self.sort_order {
                SortOrder::Asc => cmp,
                SortOrder::Desc => cmp.reverse(),
            }
        });
    }

    fn status_str(status: &ProcessStatus) -> &'static str {
        match status {
            ProcessStatus::Running => "R",
            ProcessStatus::Sleeping => "S",
            ProcessStatus::Stopped => "T",
            ProcessStatus::Zombie => "Z",
            ProcessStatus::Other(_) => "?",
        }
    }

    fn truncate_name(name: &str, max_len: usize) -> String {
        if max_len == 0 {
            return String::new();
        }
        if name.len() <= max_len {
            name.to_string()
        } else if max_len <= 1 {
            name.chars().take(max_len).collect()
        } else {
            let take = max_len.saturating_sub(1);
            let mut s: String = name.chars().take(take).collect();
            s.push('\u{2026}'); // ellipsis
            s
        }
    }

    fn adjust_scroll(&mut self) {
        if self.processes.is_empty() {
            self.selected_index = 0;
            self.scroll_offset = 0;
            return;
        }

        // Clamp selected_index
        if self.selected_index >= self.processes.len() {
            self.selected_index = self.processes.len().saturating_sub(1);
        }

        // Determine visible rows: subtract header row + signal confirm line if active
        let confirm_rows = if self.signal_confirm.is_some() { 1 } else { 0 };
        let visible_rows = self.last_area_height.saturating_sub(1 + confirm_rows) as usize;
        if visible_rows == 0 {
            return;
        }

        // Scroll up if selected is above viewport
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        }

        // Scroll down if selected is below viewport
        if self.selected_index >= self.scroll_offset + visible_rows {
            self.scroll_offset = self
                .selected_index
                .saturating_sub(visible_rows.saturating_sub(1));
        }
    }

    fn draw_content(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        self.last_area_height = area.height;

        // Minimal: text label
        if area.height < 3 || area.width < 15 {
            let label = format!("Processes: {}", self.processes.len());
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(label, theme.value))),
                area,
            );
            return;
        }

        // Medium: compact table (height 3-8)
        if area.height < 9 {
            self.draw_compact_table(frame, area, theme);
            return;
        }

        // Full: full table with all columns
        self.draw_full_table(frame, area, theme);
    }

    fn draw_compact_table(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.processes.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "Waiting for data\u{2026}",
                    theme.label,
                ))),
                area,
            );
            return;
        }

        let name_width = area.width.saturating_sub(18) as usize; // PID(7) + gap + sort_col(~10)

        let sort_header = self.sort_by.label();
        let header = Row::new(vec![
            Cell::from("PID"),
            Cell::from("Name"),
            Cell::from(sort_header),
        ])
        .style(theme.label.add_modifier(Modifier::BOLD));

        let visible_rows = area.height.saturating_sub(1) as usize; // 1 for header
        let rows: Vec<Row> = self
            .processes
            .iter()
            .take(visible_rows)
            .map(|p| {
                let sort_val = match self.sort_by {
                    SortColumn::Cpu => format!("{:.1}%", p.cpu_percent),
                    SortColumn::Memory => format_bytes(p.memory_bytes),
                    SortColumn::Pid => format!("{}", p.pid),
                    SortColumn::Name => Self::truncate_name(&p.name, name_width),
                };
                Row::new(vec![
                    Cell::from(format!("{}", p.pid)),
                    Cell::from(Self::truncate_name(&p.name, name_width)),
                    Cell::from(sort_val),
                ])
                .style(theme.value)
            })
            .collect();

        let widths = [
            Constraint::Length(7),
            Constraint::Min(8),
            Constraint::Length(10),
        ];

        let table = Table::new(rows, widths).header(header).column_spacing(1);

        frame.render_widget(table, area);
    }

    fn draw_full_table(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.processes.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "Waiting for data\u{2026}",
                    theme.label,
                ))),
                area,
            );
            return;
        }

        let name_width = area.width.saturating_sub(25) as usize; // PID(7)+CPU%(7)+MEM%(6)+S(1)+gaps(4)

        // Build header with sort indicator
        let header_cells = vec![
            Self::header_cell("PID", SortColumn::Pid, self.sort_by, self.sort_order),
            Self::header_cell("Name", SortColumn::Name, self.sort_by, self.sort_order),
            Self::header_cell("CPU%", SortColumn::Cpu, self.sort_by, self.sort_order),
            Self::header_cell("MEM%", SortColumn::Memory, self.sort_by, self.sort_order),
            Cell::from("S"),
        ];
        let header = Row::new(header_cells).style(theme.label.add_modifier(Modifier::BOLD));

        // Determine visible area (account for header + potential confirm prompt)
        let confirm_lines = if self.signal_confirm.is_some() { 1 } else { 0 };
        let visible_rows = area.height.saturating_sub(1).saturating_sub(confirm_lines) as usize;

        let rows: Vec<Row> = self
            .processes
            .iter()
            .skip(self.scroll_offset)
            .take(visible_rows)
            .enumerate()
            .map(|(i, p)| {
                let actual_idx = self.scroll_offset + i;
                let style = if self.interacting && actual_idx == self.selected_index {
                    Style::new()
                        .fg(theme.header_fg)
                        .bg(theme.header_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    theme.value
                };

                Row::new(vec![
                    Cell::from(format!("{}", p.pid)),
                    Cell::from(Self::truncate_name(&p.name, name_width)),
                    Cell::from(format!("{:.1}%", p.cpu_percent)),
                    Cell::from(format!("{:.1}%", p.memory_percent)),
                    Cell::from(Self::status_str(&p.status)),
                ])
                .style(style)
            })
            .collect();

        let widths = [
            Constraint::Length(7),
            Constraint::Min(8),
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Length(1),
        ];

        let table = Table::new(rows, widths).header(header).column_spacing(1);

        if confirm_lines > 0 {
            let [table_area, confirm_area] =
                ratatui::layout::Layout::vertical([Constraint::Min(1), Constraint::Length(1)])
                    .areas(area);

            frame.render_widget(table, table_area);
            if let Some(confirm) = &self.signal_confirm {
                let text = if let Some(ref result) = confirm.result {
                    match result {
                        Ok(()) => format!("Sent SIGTERM to {} (PID {})", confirm.name, confirm.pid),
                        Err(e) => format!("Failed: {}", e),
                    }
                } else {
                    format!("Kill {} (PID {})? [y/N]", confirm.name, confirm.pid)
                };
                let style = if confirm.result.as_ref().is_some_and(|r| r.is_err()) {
                    Style::new().fg(theme.error_fg)
                } else {
                    theme.warning
                };
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(text, style))),
                    confirm_area,
                );
            }
        } else {
            frame.render_widget(table, area);
        }
    }

    fn header_cell(
        label: &str,
        column: SortColumn,
        active_sort: SortColumn,
        sort_order: SortOrder,
    ) -> Cell<'static> {
        if column == active_sort {
            let arrow = match sort_order {
                SortOrder::Asc => "\u{25b2}",  // up arrow
                SortOrder::Desc => "\u{25bc}", // down arrow
            };
            Cell::from(format!("{label}{arrow}"))
        } else {
            Cell::from(label.to_string())
        }
    }
}

fn send_signal(pid: u32) -> std::result::Result<(), String> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    kill(Pid::from_raw(pid as i32), Signal::SIGTERM).map_err(|e| match e {
        nix::errno::Errno::EPERM => "Permission denied".to_string(),
        nix::errno::Errno::ESRCH => "Process not found".to_string(),
        other => format!("Signal failed: {other}"),
    })
}

impl Component for ProcessWidget {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        "Processes"
    }
    fn widget_type(&self) -> &str {
        "processes"
    }
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn update(&mut self) -> Result<Option<Action>> {
        Ok(None)
    }

    fn handle_data(&mut self, update: &DataUpdate) -> Result<Option<Action>> {
        if let DataUpdate::Process(data) = update {
            let mut procs = data.processes.clone();
            self.sort_processes(&mut procs);
            procs.truncate(self.config.count);
            self.processes = procs;

            // Clamp selection
            if !self.processes.is_empty() && self.selected_index >= self.processes.len() {
                self.selected_index = self.processes.len().saturating_sub(1);
            }
            self.adjust_scroll();

            // Clear signal confirm result on fresh data
            if let Some(confirm) = &self.signal_confirm
                && confirm.result.is_some()
            {
                self.signal_confirm = None;
            }
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        self.draw_content(frame, area, theme);
    }

    fn handle_event(&mut self, event: &Event) -> Result<Option<Action>> {
        let Event::Key(key) = event else {
            return Ok(None);
        };

        // Not interacting — only respond to Enter
        if !self.interacting {
            if key.code == KeyCode::Enter
                && self.last_area_height >= 9
                && !self.processes.is_empty()
            {
                return Ok(Some(Action::EnterInteract));
            }
            return Ok(None);
        }

        // Render tier gating: if area shrank below full tier, exit interact
        if self.last_area_height < 9 {
            return Ok(Some(Action::ExitInteract));
        }

        // Signal confirmation state
        if let Some(ref mut confirm) = self.signal_confirm {
            if confirm.result.is_some() {
                // Result already showing — any key clears it
                self.signal_confirm = None;
                return Ok(None);
            }
            match key.code {
                KeyCode::Char('y') => {
                    confirm.result = Some(send_signal(confirm.pid));
                    return Ok(None);
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    self.signal_confirm = None;
                    return Ok(None);
                }
                _ => return Ok(None),
            }
        }

        // Browsing keybinds
        match key.code {
            KeyCode::Esc => {
                return Ok(Some(Action::ExitInteract));
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.selected_index + 1 < self.processes.len() {
                    self.selected_index += 1;
                    self.adjust_scroll();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                    self.adjust_scroll();
                }
            }
            KeyCode::Char('s') => {
                self.sort_by = self.sort_by.next();
                let mut procs = std::mem::take(&mut self.processes);
                self.sort_processes(&mut procs);
                self.processes = procs;
            }
            KeyCode::Char('S') => {
                self.sort_order = self.sort_order.toggle();
                let mut procs = std::mem::take(&mut self.processes);
                self.sort_processes(&mut procs);
                self.processes = procs;
            }
            KeyCode::Char('x') => {
                if let Some(proc) = self.processes.get(self.selected_index) {
                    let pid = proc.pid;
                    let name = proc.name.clone();
                    if pid == std::process::id() {
                        self.signal_confirm = Some(SignalConfirm {
                            pid,
                            name,
                            result: Some(Err("Cannot signal own process".to_string())),
                        });
                    } else {
                        self.signal_confirm = Some(SignalConfirm {
                            pid,
                            name,
                            result: None,
                        });
                    }
                    self.adjust_scroll();
                }
            }
            _ => {}
        }
        Ok(None)
    }

    fn min_size(&self) -> (u16, u16) {
        (15, 1)
    }

    fn supports_interact(&self) -> bool {
        true
    }

    fn notify_interact(&mut self, active: bool) {
        self.interacting = active;
        if !active {
            self.signal_confirm = None;
        }
    }

    fn transfer_state(&mut self, old: &dyn Component) {
        if let Some(old_proc) = old.as_any().downcast_ref::<ProcessWidget>() {
            self.scroll_offset = old_proc.scroll_offset;
            self.selected_index = old_proc.selected_index;
            self.sort_by = old_proc.sort_by;
            self.sort_order = old_proc.sort_order;
            // Do NOT transfer `interacting`
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{CpuData, ProcessData};

    fn sample_processes() -> Vec<ProcessEntry> {
        vec![
            ProcessEntry {
                pid: 1,
                name: "systemd".into(),
                cpu_percent: 0.5,
                memory_bytes: 10_000_000,
                memory_percent: 0.1,
                status: ProcessStatus::Running,
            },
            ProcessEntry {
                pid: 100,
                name: "firefox".into(),
                cpu_percent: 25.0,
                memory_bytes: 500_000_000,
                memory_percent: 3.1,
                status: ProcessStatus::Running,
            },
            ProcessEntry {
                pid: 200,
                name: "code".into(),
                cpu_percent: 10.0,
                memory_bytes: 300_000_000,
                memory_percent: 1.9,
                status: ProcessStatus::Sleeping,
            },
        ]
    }

    #[test]
    fn default_config_values() {
        let w = ProcessWidget::new("procs".into(), None).unwrap();
        assert_eq!(w.sort_by, SortColumn::Cpu);
        assert_eq!(w.sort_order, SortOrder::Desc);
        assert_eq!(w.config.count, 50);
    }

    #[test]
    fn config_from_toml() {
        let val: toml::Value = toml::from_str(
            r#"
            sort_by = "memory"
            sort_order = "asc"
            count = 20
        "#,
        )
        .unwrap();
        let w = ProcessWidget::new("procs".into(), Some(val)).unwrap();
        assert_eq!(w.sort_by, SortColumn::Memory);
        assert_eq!(w.sort_order, SortOrder::Asc);
        assert_eq!(w.config.count, 20);
    }

    #[test]
    fn invalid_sort_by_errors() {
        let val: toml::Value = toml::from_str(r#"sort_by = "invalid""#).unwrap();
        assert!(ProcessWidget::new("procs".into(), Some(val)).is_err());
    }

    #[test]
    fn unknown_field_errors() {
        let val: toml::Value = toml::from_str(r#"bogus = true"#).unwrap();
        assert!(ProcessWidget::new("procs".into(), Some(val)).is_err());
    }

    #[test]
    fn handle_data_sorts_by_cpu_desc() {
        let mut w = ProcessWidget::new("procs".into(), None).unwrap();
        let update = DataUpdate::Process(ProcessData {
            processes: sample_processes(),
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.processes.len(), 3);
        assert_eq!(w.processes[0].name, "firefox"); // 25.0% CPU
        assert_eq!(w.processes[1].name, "code"); // 10.0% CPU
        assert_eq!(w.processes[2].name, "systemd"); // 0.5% CPU
    }

    #[test]
    fn handle_data_sorts_by_name_asc() {
        let val: toml::Value = toml::from_str(
            r#"
            sort_by = "name"
            sort_order = "asc"
        "#,
        )
        .unwrap();
        let mut w = ProcessWidget::new("procs".into(), Some(val)).unwrap();
        let update = DataUpdate::Process(ProcessData {
            processes: sample_processes(),
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.processes[0].name, "code");
        assert_eq!(w.processes[1].name, "firefox");
        assert_eq!(w.processes[2].name, "systemd");
    }

    #[test]
    fn handle_data_truncates_to_count() {
        let val: toml::Value = toml::from_str(r#"count = 2"#).unwrap();
        let mut w = ProcessWidget::new("procs".into(), Some(val)).unwrap();
        let update = DataUpdate::Process(ProcessData {
            processes: sample_processes(),
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.processes.len(), 2);
    }

    #[test]
    fn handle_data_clamps_selected_index() {
        let mut w = ProcessWidget::new("procs".into(), None).unwrap();
        w.selected_index = 10;
        let update = DataUpdate::Process(ProcessData {
            processes: sample_processes(),
        });
        w.handle_data(&update).unwrap();
        assert_eq!(w.selected_index, 2); // len - 1
    }

    #[test]
    fn handle_data_ignores_other_variants() {
        let mut w = ProcessWidget::new("procs".into(), None).unwrap();
        let update = DataUpdate::Cpu(CpuData {
            per_core: vec![50.0],
            overall: 50.0,
        });
        w.handle_data(&update).unwrap();
        assert!(w.processes.is_empty());
    }

    #[test]
    fn supports_interact_returns_true() {
        let w = ProcessWidget::new("procs".into(), None).unwrap();
        assert!(w.supports_interact());
    }

    #[test]
    fn draw_does_not_panic_at_small_sizes() {
        let mut w = ProcessWidget::new("procs".into(), None).unwrap();
        let update = DataUpdate::Process(ProcessData {
            processes: sample_processes(),
        });
        w.handle_data(&update).unwrap();

        let backend = ratatui::backend::TestBackend::new(30, 5);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let theme = crate::theme::Theme::default();
        terminal
            .draw(|frame| {
                w.draw(frame, frame.area(), &theme);
            })
            .unwrap();
    }

    #[test]
    fn sort_column_next_cycles() {
        assert_eq!(SortColumn::Cpu.next(), SortColumn::Memory);
        assert_eq!(SortColumn::Memory.next(), SortColumn::Pid);
        assert_eq!(SortColumn::Pid.next(), SortColumn::Name);
        assert_eq!(SortColumn::Name.next(), SortColumn::Cpu);
    }

    #[test]
    fn sort_order_toggle() {
        assert_eq!(SortOrder::Asc.toggle(), SortOrder::Desc);
        assert_eq!(SortOrder::Desc.toggle(), SortOrder::Asc);
    }

    #[test]
    fn truncate_name_short() {
        assert_eq!(ProcessWidget::truncate_name("hi", 10), "hi");
    }

    #[test]
    fn truncate_name_exact() {
        assert_eq!(ProcessWidget::truncate_name("hello", 5), "hello");
    }

    #[test]
    fn truncate_name_long() {
        let result = ProcessWidget::truncate_name("firefox-bin", 8);
        assert_eq!(result, "firefox\u{2026}");
        assert!(result.chars().count() <= 8);
    }

    #[test]
    fn status_str_variants() {
        assert_eq!(ProcessWidget::status_str(&ProcessStatus::Running), "R");
        assert_eq!(ProcessWidget::status_str(&ProcessStatus::Sleeping), "S");
        assert_eq!(ProcessWidget::status_str(&ProcessStatus::Stopped), "T");
        assert_eq!(ProcessWidget::status_str(&ProcessStatus::Zombie), "Z");
        assert_eq!(
            ProcessWidget::status_str(&ProcessStatus::Other("x".into())),
            "?"
        );
    }

    #[test]
    fn transfer_state_copies_sort_not_interacting() {
        let mut old = ProcessWidget::new("procs".into(), None).unwrap();
        old.sort_by = SortColumn::Name;
        old.sort_order = SortOrder::Asc;
        old.scroll_offset = 5;
        old.selected_index = 3;
        old.interacting = true;

        let mut new_w = ProcessWidget::new("procs".into(), None).unwrap();
        new_w.transfer_state(&old);
        assert_eq!(new_w.sort_by, SortColumn::Name);
        assert_eq!(new_w.sort_order, SortOrder::Asc);
        assert_eq!(new_w.scroll_offset, 5);
        assert_eq!(new_w.selected_index, 3);
        assert!(!new_w.interacting); // NOT transferred
    }

    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};

    fn make_key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn make_key_shift(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::SHIFT))
    }

    #[test]
    fn handle_event_enter_interact() {
        let mut w = ProcessWidget::new("processes".into(), None).unwrap();
        w.last_area_height = 20;
        w.processes = sample_processes();
        let result = w.handle_event(&make_key(KeyCode::Enter)).unwrap();
        assert_eq!(result, Some(Action::EnterInteract));
        // Widget does not set interacting itself — App calls notify_interact
        assert!(!w.interacting);
        w.notify_interact(true);
        assert!(w.interacting);
    }

    #[test]
    fn handle_event_enter_blocked_in_medium_tier() {
        let mut w = ProcessWidget::new("processes".into(), None).unwrap();
        w.last_area_height = 5;
        w.processes = sample_processes();
        let result = w.handle_event(&make_key(KeyCode::Enter)).unwrap();
        assert_eq!(result, None);
        assert!(!w.interacting);
    }

    #[test]
    fn handle_event_escape_exits_interact() {
        let mut w = ProcessWidget::new("processes".into(), None).unwrap();
        w.notify_interact(true);
        w.last_area_height = 20;
        w.processes = sample_processes();
        let result = w.handle_event(&make_key(KeyCode::Esc)).unwrap();
        assert_eq!(result, Some(Action::ExitInteract));
        // Widget does not clear interacting itself — App calls notify_interact
        assert!(w.interacting);
        w.notify_interact(false);
        assert!(!w.interacting);
    }

    #[test]
    fn handle_event_j_moves_selection_down() {
        let mut w = ProcessWidget::new("processes".into(), None).unwrap();
        w.notify_interact(true);
        w.last_area_height = 20;
        w.processes = sample_processes();
        w.selected_index = 0;
        w.handle_event(&make_key(KeyCode::Char('j'))).unwrap();
        assert_eq!(w.selected_index, 1);
    }

    #[test]
    fn handle_event_k_moves_selection_up() {
        let mut w = ProcessWidget::new("processes".into(), None).unwrap();
        w.notify_interact(true);
        w.last_area_height = 20;
        w.processes = sample_processes();
        w.selected_index = 2;
        w.handle_event(&make_key(KeyCode::Char('k'))).unwrap();
        assert_eq!(w.selected_index, 1);
    }

    #[test]
    fn handle_event_s_cycles_sort_column() {
        let mut w = ProcessWidget::new("processes".into(), None).unwrap();
        w.notify_interact(true);
        w.last_area_height = 20;
        w.processes = sample_processes();
        assert_eq!(w.sort_by, SortColumn::Cpu);
        w.handle_event(&make_key(KeyCode::Char('s'))).unwrap();
        assert_eq!(w.sort_by, SortColumn::Memory);
    }

    #[test]
    fn handle_event_shift_s_toggles_sort_order() {
        let mut w = ProcessWidget::new("processes".into(), None).unwrap();
        w.notify_interact(true);
        w.last_area_height = 20;
        w.processes = sample_processes();
        assert_eq!(w.sort_order, SortOrder::Desc);
        w.handle_event(&make_key_shift(KeyCode::Char('S'))).unwrap();
        assert_eq!(w.sort_order, SortOrder::Asc);
    }

    #[test]
    fn handle_event_x_enters_signal_confirm() {
        let mut w = ProcessWidget::new("processes".into(), None).unwrap();
        w.notify_interact(true);
        w.last_area_height = 20;
        w.processes = sample_processes();
        w.selected_index = 1;
        w.handle_event(&make_key(KeyCode::Char('x'))).unwrap();
        assert!(w.signal_confirm.is_some());
        assert_eq!(w.signal_confirm.as_ref().unwrap().pid, 100);
    }

    #[test]
    fn handle_event_x_blocks_self_pid() {
        let mut w = ProcessWidget::new("processes".into(), None).unwrap();
        w.notify_interact(true);
        w.last_area_height = 20;
        w.processes = vec![ProcessEntry {
            pid: std::process::id(),
            name: "self".into(),
            cpu_percent: 0.0,
            memory_bytes: 0,
            memory_percent: 0.0,
            status: ProcessStatus::Running,
        }];
        w.selected_index = 0;
        w.handle_event(&make_key(KeyCode::Char('x'))).unwrap();
        let confirm = w.signal_confirm.as_ref().unwrap();
        assert!(confirm.result.as_ref().unwrap().is_err());
    }

    #[test]
    fn handle_event_n_cancels_signal_confirm() {
        let mut w = ProcessWidget::new("processes".into(), None).unwrap();
        w.notify_interact(true);
        w.last_area_height = 20;
        w.processes = sample_processes();
        w.signal_confirm = Some(SignalConfirm {
            pid: 100,
            name: "firefox".into(),
            result: None,
        });
        w.handle_event(&make_key(KeyCode::Char('n'))).unwrap();
        assert!(w.signal_confirm.is_none());
    }

    #[test]
    fn handle_event_escape_cancels_signal_confirm() {
        let mut w = ProcessWidget::new("processes".into(), None).unwrap();
        w.notify_interact(true);
        w.last_area_height = 20;
        w.processes = sample_processes();
        w.signal_confirm = Some(SignalConfirm {
            pid: 100,
            name: "firefox".into(),
            result: None,
        });
        let result = w.handle_event(&make_key(KeyCode::Esc)).unwrap();
        assert!(w.signal_confirm.is_none());
        assert!(w.interacting);
        assert_eq!(result, None);
    }

    #[test]
    fn handle_event_exit_interact_on_resize_below_threshold() {
        let mut w = ProcessWidget::new("processes".into(), None).unwrap();
        w.notify_interact(true);
        w.last_area_height = 5;
        w.processes = sample_processes();
        let result = w.handle_event(&make_key(KeyCode::Char('j'))).unwrap();
        assert_eq!(result, Some(Action::ExitInteract));
        // Widget does not clear interacting itself — App calls notify_interact
        assert!(w.interacting);
        w.notify_interact(false);
        assert!(!w.interacting);
    }
}
