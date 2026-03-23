use std::any::Any;

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

    /// The widget type name (e.g. "cpu", "memory"). Used during config reload
    /// to compare widget types and decide whether state can be transferred.
    fn widget_type(&self) -> &str;

    /// Return self as `&dyn Any` to enable downcasting in `transfer_state`.
    fn as_any(&self) -> &dyn Any;

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

    /// Copy relevant state (history buffers, baselines) from an old widget
    /// instance into self. Called during config hot-reload when a widget of
    /// the same type is rebuilt so that sparklines and throughput baselines
    /// survive the reload. The default no-op is correct for stateless widgets.
    fn transfer_state(&mut self, _old: &dyn Component) {}

    /// Whether this widget supports interact mode (Enter to engage,
    /// Escape to disengage). Default false — only interactive widgets
    /// like the process list override this.
    fn supports_interact(&self) -> bool {
        false
    }

    /// Called by App to notify the widget that interact mode has been
    /// entered or exited. Only meaningful for widgets where supports_interact() is true.
    fn notify_interact(&mut self, _active: bool) {}
}
