pub mod command;
pub mod external;
pub mod hyprland;
pub mod system;
pub mod types;

pub use external::{spawn_packages_task, spawn_services_task};
pub use hyprland::spawn_hyprland_task;
pub use system::spawn_sysinfo_task;
pub use types::DataUpdate;
