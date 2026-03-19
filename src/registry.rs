use color_eyre::Result;

use crate::component::Component;
use crate::widgets::PlaceholderWidget;

pub type WidgetConstructor =
    fn(id: String, widget_type: String, config: Option<toml::Value>) -> Result<Box<dyn Component>>;

pub struct WidgetDescriptor {
    pub constructor: WidgetConstructor,
}

fn placeholder_constructor(
    id: String,
    widget_type: String,
    _config: Option<toml::Value>,
) -> Result<Box<dyn Component>> {
    Ok(Box::new(PlaceholderWidget::new(id, widget_type)))
}

static PLACEHOLDER_DESCRIPTOR: WidgetDescriptor = WidgetDescriptor {
    constructor: placeholder_constructor,
};

pub fn get_descriptor(widget_type: &str) -> Option<&'static WidgetDescriptor> {
    match widget_type {
        "cpu" | "memory" | "network" | "temps" => Some(&PLACEHOLDER_DESCRIPTOR),
        _ => None,
    }
}

pub fn is_known_type(widget_type: &str) -> bool {
    get_descriptor(widget_type).is_some()
}
