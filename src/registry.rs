use color_eyre::Result;

use crate::component::Component;
use crate::widgets::{
    CpuWidget, DiskWidget, MemoryWidget, NetworkWidget, PackagesWidget, PlaceholderWidget,
    ProcessWidget, ServicesWidget, TempsWidget, WorkspacesWidget,
};

pub type WidgetConstructor =
    fn(id: String, widget_type: String, config: Option<toml::Value>) -> Result<Box<dyn Component>>;

pub struct WidgetDescriptor {
    pub constructor: WidgetConstructor,
}

fn cpu_constructor(
    id: String,
    _widget_type: String,
    config: Option<toml::Value>,
) -> Result<Box<dyn Component>> {
    Ok(Box::new(CpuWidget::new(id, config)?))
}

fn memory_constructor(
    id: String,
    _widget_type: String,
    config: Option<toml::Value>,
) -> Result<Box<dyn Component>> {
    Ok(Box::new(MemoryWidget::new(id, config)?))
}

fn network_constructor(
    id: String,
    _widget_type: String,
    config: Option<toml::Value>,
) -> Result<Box<dyn Component>> {
    Ok(Box::new(NetworkWidget::new(id, config)?))
}

fn temps_constructor(
    id: String,
    _widget_type: String,
    config: Option<toml::Value>,
) -> Result<Box<dyn Component>> {
    Ok(Box::new(TempsWidget::new(id, config)?))
}

fn disk_constructor(
    id: String,
    _widget_type: String,
    config: Option<toml::Value>,
) -> Result<Box<dyn Component>> {
    Ok(Box::new(DiskWidget::new(id, config)?))
}

fn processes_constructor(
    id: String,
    _widget_type: String,
    config: Option<toml::Value>,
) -> Result<Box<dyn Component>> {
    Ok(Box::new(ProcessWidget::new(id, config)?))
}

fn packages_constructor(
    id: String,
    _widget_type: String,
    config: Option<toml::Value>,
) -> Result<Box<dyn Component>> {
    Ok(Box::new(PackagesWidget::new(id, config)?))
}

fn services_constructor(
    id: String,
    _widget_type: String,
    config: Option<toml::Value>,
) -> Result<Box<dyn Component>> {
    Ok(Box::new(ServicesWidget::new(id, config)?))
}

fn workspaces_constructor(
    id: String,
    _widget_type: String,
    config: Option<toml::Value>,
) -> Result<Box<dyn Component>> {
    Ok(Box::new(WorkspacesWidget::new(id, config)?))
}

#[allow(dead_code)]
fn placeholder_constructor(
    id: String,
    widget_type: String,
    _config: Option<toml::Value>,
) -> Result<Box<dyn Component>> {
    Ok(Box::new(PlaceholderWidget::new(id, widget_type)))
}

static CPU_DESCRIPTOR: WidgetDescriptor = WidgetDescriptor {
    constructor: cpu_constructor,
};
static MEMORY_DESCRIPTOR: WidgetDescriptor = WidgetDescriptor {
    constructor: memory_constructor,
};
static NETWORK_DESCRIPTOR: WidgetDescriptor = WidgetDescriptor {
    constructor: network_constructor,
};
static TEMPS_DESCRIPTOR: WidgetDescriptor = WidgetDescriptor {
    constructor: temps_constructor,
};
static DISK_DESCRIPTOR: WidgetDescriptor = WidgetDescriptor {
    constructor: disk_constructor,
};
static PROCESSES_DESCRIPTOR: WidgetDescriptor = WidgetDescriptor {
    constructor: processes_constructor,
};
static PACKAGES_DESCRIPTOR: WidgetDescriptor = WidgetDescriptor {
    constructor: packages_constructor,
};
static SERVICES_DESCRIPTOR: WidgetDescriptor = WidgetDescriptor {
    constructor: services_constructor,
};
static WORKSPACES_DESCRIPTOR: WidgetDescriptor = WidgetDescriptor {
    constructor: workspaces_constructor,
};

#[allow(dead_code)]
static PLACEHOLDER_DESCRIPTOR: WidgetDescriptor = WidgetDescriptor {
    constructor: placeholder_constructor,
};

pub fn get_descriptor(widget_type: &str) -> Option<&'static WidgetDescriptor> {
    match widget_type {
        "cpu" => Some(&CPU_DESCRIPTOR),
        "memory" => Some(&MEMORY_DESCRIPTOR),
        "network" => Some(&NETWORK_DESCRIPTOR),
        "temps" => Some(&TEMPS_DESCRIPTOR),
        "disk" => Some(&DISK_DESCRIPTOR),
        "processes" => Some(&PROCESSES_DESCRIPTOR),
        "packages" => Some(&PACKAGES_DESCRIPTOR),
        "services" => Some(&SERVICES_DESCRIPTOR),
        "workspaces" => Some(&WORKSPACES_DESCRIPTOR),
        _ => None,
    }
}

pub fn is_known_type(widget_type: &str) -> bool {
    get_descriptor(widget_type).is_some()
}

pub fn known_types() -> Vec<&'static str> {
    let mut types = vec![
        "cpu", "disk", "memory", "network",
        "packages", "processes", "services", "temps", "workspaces",
    ];
    types.sort();
    types
}
