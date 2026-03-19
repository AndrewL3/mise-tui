use color_eyre::Result;
use ratatui::{Frame, layout::Rect};

use crate::action::Action;
use crate::data::DataUpdate;
use crate::event::Event;
use crate::theme::Theme;

pub trait Component {
    /// Unique instance identifier (e.g. "cpu", "net-wifi")
    fn id(&self) -> &str;

    /// Human-readable name for display in panel title
    fn name(&self) -> &str;

    /// Called on each tick for internal state management. NOT for data fetching.
    fn update(&mut self) -> Result<Option<Action>>;

    /// Receive a data update pushed from an async polling task.
    fn handle_data(&mut self, update: &DataUpdate) -> Result<Option<Action>>;

    /// Render the component's content into the given inner area.
    /// App renders panel chrome (border, title, focus styling) and passes
    /// the interior content Rect. Widgets never draw their own borders.
    fn draw(&mut self, frame: &mut Frame, area: Rect, theme: &Theme);

    /// Handle an input event. Return an Action to communicate with App.
    fn handle_event(&mut self, event: &Event) -> Result<Option<Action>>;

    /// Minimum size this component needs to render usefully (interior content area).
    fn min_size(&self) -> (u16, u16) {
        (10, 3)
    }
}
