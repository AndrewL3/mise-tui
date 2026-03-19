use std::collections::HashMap;
use std::io;

use color_eyre::Result;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::event::{KeyCode, KeyModifiers},
    style::Style,
    text::Line,
    widgets::{Block, Paragraph},
};

use crate::action::Direction;
use crate::component::Component;
use crate::config::Config;
use crate::event::{Event, EventHandler};
use crate::layout::LayoutEngine;
use crate::registry;
use crate::theme::Theme;

pub struct App {
    should_quit: bool,
    components: HashMap<String, Box<dyn Component>>,
    layout: LayoutEngine,
    theme: Theme,
    focus: Option<(usize, usize)>,
    tick_rate_ms: u64,
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        let theme = Theme::from_config(&config.theme)?;
        let layout = LayoutEngine::from_config(&config.layout)?;

        let mut components: HashMap<String, Box<dyn Component>> = HashMap::new();

        for panel in &config.layout.panels {
            let instance_id = panel.instance_id().to_string();
            let widget_config = config.widgets.get(&instance_id).cloned();

            let descriptor = registry::get_descriptor(&panel.widget_type)
                .expect("unknown widget type should have been caught by validation");

            let component = (descriptor.constructor)(
                instance_id.clone(),
                panel.widget_type.clone(),
                widget_config,
            )?;
            components.insert(instance_id, component);
        }

        // Set initial focus to first occupied cell in reading order
        let focus = layout.occupied_cells().first().copied();

        let tick_rate_ms = config.general.tick_rate;

        Ok(Self {
            should_quit: false,
            components,
            layout,
            theme,
            focus,
            tick_rate_ms,
        })
    }

    pub async fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        let mut events = EventHandler::new(std::time::Duration::from_millis(self.tick_rate_ms));

        // Placeholder channels for future data and config sources.
        let (_data_tx, mut data_rx) = tokio::sync::mpsc::channel::<()>(1);
        let (_config_tx, mut config_rx) = tokio::sync::mpsc::channel::<()>(1);

        while !self.should_quit {
            tokio::select! {
                event = events.next() => {
                    let event = event?;
                    match event {
                        Event::Tick => {
                            let mut actions = Vec::new();
                            for component in self.components.values_mut() {
                                if let Some(action) = component.update()? {
                                    actions.push(action);
                                }
                            }
                            for action in actions {
                                self.handle_action(action);
                            }
                            self.draw(terminal)?;
                        }
                        Event::Key(key) => {
                            self.handle_key(key)?;
                        }
                        Event::Resize(..) => {
                            self.draw(terminal)?;
                        }
                    }
                }
                _ = data_rx.recv() => {
                    // M2: data updates routed to components via handle_data()
                }
                _ = config_rx.recv() => {
                    // M3: config reload events trigger layout rebuild
                }
            }
        }

        Ok(())
    }

    fn handle_key(&mut self, key: ratatui::crossterm::event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Tab => self.focus_next(),
            KeyCode::BackTab => self.focus_prev(),
            KeyCode::Up => self.focus_direction(Direction::Up),
            KeyCode::Down => self.focus_direction(Direction::Down),
            KeyCode::Left => self.focus_direction(Direction::Left),
            KeyCode::Right => self.focus_direction(Direction::Right),
            _ => {
                // Dispatch to focused component
                if let Some(focus_pos) = self.focus
                    && let Some(instance_id) = self.layout.instance_at(focus_pos.0, focus_pos.1)
                    && let Some(component) = self.components.get_mut(instance_id)
                {
                    let event = Event::Key(key);
                    if let Some(action) = component.handle_event(&event)? {
                        self.handle_action(action);
                    }
                }
            }
        }
        Ok(())
    }

    fn handle_action(&mut self, action: crate::action::Action) {
        match action {
            crate::action::Action::Quit => self.should_quit = true,
            _ => {}
        }
    }

    fn focus_next(&mut self) {
        let cells = self.layout.occupied_cells();
        if cells.is_empty() {
            return;
        }

        self.focus = Some(match self.focus {
            Some(current) => {
                if let Some(pos) = cells.iter().position(|c| *c == current) {
                    cells[(pos + 1) % cells.len()]
                } else {
                    cells[0]
                }
            }
            None => cells[0],
        });
    }

    fn focus_prev(&mut self) {
        let cells = self.layout.occupied_cells();
        if cells.is_empty() {
            return;
        }

        self.focus = Some(match self.focus {
            Some(current) => {
                if let Some(pos) = cells.iter().position(|c| *c == current) {
                    if pos == 0 {
                        cells[cells.len() - 1]
                    } else {
                        cells[pos - 1]
                    }
                } else {
                    cells[0]
                }
            }
            None => cells[0],
        });
    }

    fn focus_direction(&mut self, direction: Direction) {
        let current = match self.focus {
            Some(pos) => pos,
            None => return,
        };

        let (rows, cols) = self.layout.grid_dimensions();

        let target = match direction {
            Direction::Up => {
                // Search rows above in current column (decreasing row)
                (0..current.0)
                    .rev()
                    .find(|&r| self.layout.instance_at(r, current.1).is_some())
                    .map(|r| (r, current.1))
            }
            Direction::Down => {
                // Search rows below in current column (increasing row)
                ((current.0 + 1)..rows)
                    .find(|&r| self.layout.instance_at(r, current.1).is_some())
                    .map(|r| (r, current.1))
            }
            Direction::Left => {
                // Search cols left in current row (decreasing col)
                (0..current.1)
                    .rev()
                    .find(|&c| self.layout.instance_at(current.0, c).is_some())
                    .map(|c| (current.0, c))
            }
            Direction::Right => {
                // Search cols right in current row (increasing col)
                ((current.1 + 1)..cols)
                    .find(|&c| self.layout.instance_at(current.0, c).is_some())
                    .map(|c| (current.0, c))
            }
        };

        if let Some(t) = target {
            self.focus = Some(t);
        }
    }

    fn draw(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        terminal.draw(|frame| {
            let area = frame.area();
            let (header_rect, _grid_rect, footer_rect) = self.layout.split_chrome(area);

            // Render header
            if let Some(header_area) = header_rect {
                let header_style = Style::new()
                    .fg(self.theme.header_fg)
                    .bg(self.theme.header_bg);
                let header = Paragraph::new(Line::from(" mise-tui ").style(header_style))
                    .style(header_style);
                frame.render_widget(header, header_area);
            }

            // Render grid cells
            let all_rects = self.layout.resolve_all_rects(area);
            let (rows, cols) = self.layout.grid_dimensions();

            for row in 0..rows {
                for col in 0..cols {
                    let cell_rect = match all_rects.get(&(row, col)) {
                        Some(r) => *r,
                        None => continue,
                    };

                    let is_focused = self.focus == Some((row, col));
                    let border_style = if is_focused {
                        self.theme.border_focused
                    } else {
                        self.theme.border
                    };

                    let instance_id = self.layout.instance_at(row, col);

                    let block = if let Some(id) = instance_id {
                        let name = self
                            .components
                            .get(id)
                            .map(|c| c.name().to_string())
                            .unwrap_or_default();
                        Block::bordered()
                            .title(format!(" {} ", name))
                            .title_style(self.theme.title)
                            .border_style(border_style)
                    } else {
                        Block::bordered().border_style(border_style)
                    };

                    let inner = block.inner(cell_rect);
                    frame.render_widget(block, cell_rect);

                    // Render component content in the inner area
                    if let Some(id) = instance_id
                        && let Some(component) = self.components.get_mut(id)
                    {
                        component.draw(frame, inner, &self.theme);
                    }
                }
            }

            // Render footer
            if let Some(footer_area) = footer_rect {
                let footer_style = Style::new()
                    .fg(self.theme.header_fg)
                    .bg(self.theme.header_bg);
                let footer = Paragraph::new(
                    Line::from(" q: quit | Tab/Shift+Tab: cycle focus | Arrow keys: navigate ")
                        .style(footer_style),
                )
                .style(footer_style);
                frame.render_widget(footer, footer_area);
            }
        })?;
        Ok(())
    }
}
