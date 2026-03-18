use std::io;

use color_eyre::Result;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::event::{KeyCode, KeyModifiers},
    widgets::Block,
};

use crate::event::{Event, EventHandler};

pub struct App {
    should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self { should_quit: false }
    }

    pub async fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        let mut events = EventHandler::new(std::time::Duration::from_millis(250));

        // Placeholder channels for future data and config sources.
        // Senders are held to keep channels open (receivers stay pending in select!).
        let (_data_tx, mut data_rx) = tokio::sync::mpsc::channel::<()>(1);
        let (_config_tx, mut config_rx) = tokio::sync::mpsc::channel::<()>(1);

        while !self.should_quit {
            tokio::select! {
                event = events.next() => {
                    let event = event?;
                    match event {
                        Event::Tick => {
                            self.draw(terminal)?;
                        }
                        Event::Key(key) => {
                            self.handle_key(key);
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

    fn handle_key(&mut self, key: ratatui::crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            _ => {}
        }
    }

    fn draw(&self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        terminal.draw(|frame| {
            let block = Block::bordered().title(" mise-tui ");
            frame.render_widget(block, frame.area());
        })?;
        Ok(())
    }
}
