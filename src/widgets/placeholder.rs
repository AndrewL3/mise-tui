use std::any::Any;

use color_eyre::Result;
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::widgets::Paragraph;

use crate::action::Action;
use crate::component::Component;
use crate::data::DataUpdate;
use crate::event::Event;
use crate::theme::Theme;

pub struct PlaceholderWidget {
    id: String,
    widget_type: String,
}

impl PlaceholderWidget {
    pub fn new(id: String, widget_type: String) -> Self {
        Self { id, widget_type }
    }
}

impl Component for PlaceholderWidget {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.widget_type
    }

    fn widget_type(&self) -> &str {
        &self.widget_type
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn update(&mut self) -> Result<Option<Action>> {
        Ok(None)
    }

    fn handle_data(&mut self, _update: &DataUpdate) -> Result<Option<Action>> {
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, _theme: &Theme) {
        let label = if self.id == self.widget_type {
            self.widget_type.clone()
        } else {
            format!("{} ({})", self.widget_type, self.id)
        };
        let paragraph = Paragraph::new(label).alignment(Alignment::Center);
        frame.render_widget(paragraph, area);
    }

    fn handle_event(&mut self, _event: &Event) -> Result<Option<Action>> {
        Ok(None)
    }
}
