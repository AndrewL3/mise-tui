#[derive(Debug, Clone)]
pub enum DataUpdate {
    Cpu(CpuData),
    Memory(MemoryData),
    Network(NetworkData),
    Temps(TempData),
    Disk(DiskData),
    Process(ProcessData),
    Packages(PackagesResult),
    Services(ServicesResult),
    Hyprland(HyprlandData),
}

impl DataUpdate {
    /// Returns true if this update variant corresponds to the given widget type.
    pub fn matches_widget_type(&self, widget_type: &str) -> bool {
        matches!(
            (self, widget_type),
            (DataUpdate::Cpu(_), "cpu")
                | (DataUpdate::Memory(_), "memory")
                | (DataUpdate::Network(_), "network")
                | (DataUpdate::Temps(_), "temps")
                | (DataUpdate::Disk(_), "disk")
                | (DataUpdate::Process(_), "processes")
                | (DataUpdate::Packages(_), "packages")
                | (DataUpdate::Services(_), "services")
                | (DataUpdate::Hyprland(_), "workspaces")
        )
    }
}

#[derive(Debug, Clone)]
pub struct CpuData {
    pub per_core: Vec<f32>,
    pub overall: f32,
}

#[derive(Debug, Clone)]
pub struct MemoryData {
    pub total_mem: u64,
    pub used_mem: u64,
    pub total_swap: u64,
    pub used_swap: u64,
    pub top_processes: Vec<ProcessInfo>,
}

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub name: String,
    pub pid: u32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct NetworkData {
    pub interfaces: Vec<InterfaceData>,
}

#[derive(Debug, Clone)]
pub struct InterfaceData {
    pub name: String,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Debug, Clone)]
pub struct TempData {
    pub sensors: Vec<SensorData>,
}

#[derive(Debug, Clone)]
pub struct SensorData {
    pub label: String,
    pub temp_celsius: Option<f32>,
    pub max_celsius: Option<f32>,
    pub critical_celsius: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct DiskData {
    pub disks: Vec<DiskInfo>,
}

#[derive(Debug, Clone)]
pub struct DiskInfo {
    pub mount_point: String,
    pub device_name: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub io: Option<DiskIoStats>,
}

#[derive(Debug, Clone)]
pub struct DiskIoStats {
    pub read_bytes_per_sec: f64,
    pub write_bytes_per_sec: f64,
}

#[derive(Debug, Clone)]
pub struct ProcessData {
    pub processes: Vec<ProcessEntry>,
}

#[derive(Debug, Clone)]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub memory_percent: f32,
    pub status: ProcessStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessStatus {
    Running,
    Sleeping,
    Stopped,
    Zombie,
    Other(String),
}

#[derive(Debug, Clone)]
pub struct PackagesResult {
    pub instance_id: String,
    pub data: Result<PackagesData, String>,
}

#[derive(Debug, Clone)]
pub struct PackagesData {
    pub updates: Vec<PackageUpdate>,
}

#[derive(Debug, Clone)]
pub struct PackageUpdate {
    pub name: String,
    pub old_version: String,
    pub new_version: String,
}

#[derive(Debug, Clone)]
pub struct ServicesResult {
    pub instance_id: String,
    pub data: Result<ServicesData, String>,
}

#[derive(Debug, Clone)]
pub struct ServicesData {
    pub services: Vec<ServiceStatus>,
}

#[derive(Debug, Clone)]
pub struct ServiceStatus {
    pub name: String,
    pub active_state: ActiveState,
    pub sub_state: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActiveState {
    Active,
    Inactive,
    Failed,
    Activating,
    Deactivating,
    Other(String),
}

pub type WorkspaceId = i32;

#[derive(Debug, Clone)]
pub struct HyprlandData {
    pub monitors: Vec<MonitorInfo>,
    pub workspaces: Vec<WorkspaceInfo>,
    pub active_workspace: Option<WorkspaceId>,
    pub active_window: Option<String>,
    pub connected: bool,
    pub detected: bool,
}

#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub id: i32,
    pub name: String,
    pub active_workspace_id: WorkspaceId,
}

#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub id: WorkspaceId,
    pub name: String,
    pub monitor: String,
    pub window_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_data_stores_per_core_and_overall() {
        let data = CpuData {
            per_core: vec![25.0, 50.0, 75.0, 100.0],
            overall: 62.5,
        };
        assert_eq!(data.per_core.len(), 4);
        assert!((data.overall - 62.5).abs() < f32::EPSILON);
    }

    #[test]
    fn memory_data_stores_all_fields() {
        let data = MemoryData {
            total_mem: 16_000_000_000,
            used_mem: 8_000_000_000,
            total_swap: 4_000_000_000,
            used_swap: 1_000_000_000,
            top_processes: vec![ProcessInfo {
                name: "firefox".to_string(),
                pid: 1234,
                memory_bytes: 500_000_000,
            }],
        };
        assert_eq!(data.top_processes.len(), 1);
        assert_eq!(data.top_processes[0].name, "firefox");
    }

    #[test]
    fn network_data_stores_interfaces() {
        let data = NetworkData {
            interfaces: vec![InterfaceData {
                name: "eth0".to_string(),
                bytes_sent: 1024,
                bytes_received: 2048,
            }],
        };
        assert_eq!(data.interfaces[0].name, "eth0");
    }

    #[test]
    fn temp_data_stores_sensors() {
        let data = TempData {
            sensors: vec![SensorData {
                label: "CPU Temp".to_string(),
                temp_celsius: Some(55.0),
                max_celsius: Some(100.0),
                critical_celsius: Some(105.0),
            }],
        };
        assert!(data.sensors[0].max_celsius.is_some());
    }

    #[test]
    fn empty_collections_represent_missing_sources() {
        let cpu = CpuData {
            per_core: vec![],
            overall: 0.0,
        };
        let net = NetworkData { interfaces: vec![] };
        let temps = TempData { sensors: vec![] };
        assert!(cpu.per_core.is_empty());
        assert!(net.interfaces.is_empty());
        assert!(temps.sensors.is_empty());
    }

    #[test]
    fn data_update_enum_wraps_all_variants() {
        let updates = vec![
            DataUpdate::Cpu(CpuData {
                per_core: vec![50.0],
                overall: 50.0,
            }),
            DataUpdate::Memory(MemoryData {
                total_mem: 1,
                used_mem: 1,
                total_swap: 0,
                used_swap: 0,
                top_processes: vec![],
            }),
            DataUpdate::Network(NetworkData { interfaces: vec![] }),
            DataUpdate::Temps(TempData { sensors: vec![] }),
            DataUpdate::Disk(DiskData { disks: vec![] }),
            DataUpdate::Process(ProcessData { processes: vec![] }),
            DataUpdate::Packages(PackagesResult {
                instance_id: "packages".to_string(),
                data: Ok(PackagesData { updates: vec![] }),
            }),
            DataUpdate::Services(ServicesResult {
                instance_id: "services".to_string(),
                data: Ok(ServicesData { services: vec![] }),
            }),
            DataUpdate::Hyprland(HyprlandData {
                monitors: vec![],
                workspaces: vec![],
                active_workspace: None,
                active_window: None,
                connected: false,
                detected: false,
            }),
        ];
        assert_eq!(updates.len(), 9);
    }

    #[test]
    fn disk_data_stores_disks() {
        let data = DiskData {
            disks: vec![DiskInfo {
                mount_point: "/".to_string(),
                device_name: "sda1".to_string(),
                total_bytes: 500_000_000_000,
                available_bytes: 200_000_000_000,
                io: Some(DiskIoStats {
                    read_bytes_per_sec: 1_000_000.0,
                    write_bytes_per_sec: 500_000.0,
                }),
            }],
        };
        assert_eq!(data.disks.len(), 1);
        assert_eq!(data.disks[0].mount_point, "/");
        assert!(data.disks[0].io.is_some());
    }

    #[test]
    fn disk_info_io_none_for_unresolvable() {
        let info = DiskInfo {
            mount_point: "/mnt/lvm".to_string(),
            device_name: "dm-0".to_string(),
            total_bytes: 100_000_000,
            available_bytes: 50_000_000,
            io: None,
        };
        assert!(info.io.is_none());
    }

    #[test]
    fn process_data_stores_entries() {
        let data = ProcessData {
            processes: vec![ProcessEntry {
                pid: 1234,
                name: "firefox".to_string(),
                cpu_percent: 12.5,
                memory_bytes: 500_000_000,
                memory_percent: 3.1,
                status: ProcessStatus::Running,
            }],
        };
        assert_eq!(data.processes.len(), 1);
        assert_eq!(data.processes[0].pid, 1234);
    }

    #[test]
    fn process_status_variants() {
        let statuses = vec![
            ProcessStatus::Running,
            ProcessStatus::Sleeping,
            ProcessStatus::Stopped,
            ProcessStatus::Zombie,
            ProcessStatus::Other("unknown".to_string()),
        ];
        assert_eq!(statuses.len(), 5);
    }

    #[test]
    fn matches_widget_type_disk() {
        let update = DataUpdate::Disk(DiskData { disks: vec![] });
        assert!(update.matches_widget_type("disk"));
        assert!(!update.matches_widget_type("cpu"));
    }

    #[test]
    fn matches_widget_type_processes() {
        let update = DataUpdate::Process(ProcessData { processes: vec![] });
        assert!(update.matches_widget_type("processes"));
        assert!(!update.matches_widget_type("memory"));
    }

    #[test]
    fn packages_result_stores_data() {
        let result = PackagesResult {
            instance_id: "packages".to_string(),
            data: Ok(PackagesData {
                updates: vec![PackageUpdate {
                    name: "linux".to_string(),
                    old_version: "6.8.1".to_string(),
                    new_version: "6.8.2".to_string(),
                }],
            }),
        };
        assert_eq!(result.instance_id, "packages");
        assert!(result.data.is_ok());
        assert_eq!(result.data.unwrap().updates.len(), 1);
    }

    #[test]
    fn packages_result_stores_error() {
        let result = PackagesResult {
            instance_id: "packages".to_string(),
            data: Err("checkupdates not found".to_string()),
        };
        assert!(result.data.is_err());
    }

    #[test]
    fn services_result_stores_data() {
        let result = ServicesResult {
            instance_id: "services".to_string(),
            data: Ok(ServicesData {
                services: vec![ServiceStatus {
                    name: "sshd".to_string(),
                    active_state: ActiveState::Active,
                    sub_state: "running".to_string(),
                }],
            }),
        };
        assert!(result.data.is_ok());
    }

    #[test]
    fn active_state_variants() {
        let states = vec![
            ActiveState::Active,
            ActiveState::Inactive,
            ActiveState::Failed,
            ActiveState::Activating,
            ActiveState::Deactivating,
            ActiveState::Other("maintenance".to_string()),
        ];
        assert_eq!(states.len(), 6);
    }

    #[test]
    fn matches_widget_type_packages() {
        let update = DataUpdate::Packages(PackagesResult {
            instance_id: "packages".to_string(),
            data: Ok(PackagesData { updates: vec![] }),
        });
        assert!(update.matches_widget_type("packages"));
        assert!(!update.matches_widget_type("cpu"));
    }

    #[test]
    fn matches_widget_type_services() {
        let update = DataUpdate::Services(ServicesResult {
            instance_id: "services".to_string(),
            data: Ok(ServicesData { services: vec![] }),
        });
        assert!(update.matches_widget_type("services"));
        assert!(!update.matches_widget_type("memory"));
    }

    #[test]
    fn hyprland_data_stores_state() {
        let data = HyprlandData {
            monitors: vec![MonitorInfo {
                id: 0,
                name: "DP-1".to_string(),
                active_workspace_id: 1,
            }],
            workspaces: vec![WorkspaceInfo {
                id: 1,
                name: "1".to_string(),
                monitor: "DP-1".to_string(),
                window_count: 3,
            }],
            active_workspace: Some(1),
            active_window: Some("Firefox".to_string()),
            connected: true,
            detected: true,
        };
        assert_eq!(data.monitors.len(), 1);
        assert_eq!(data.workspaces[0].window_count, 3);
        assert!(data.connected);
    }

    #[test]
    fn matches_widget_type_workspaces() {
        let update = DataUpdate::Hyprland(HyprlandData {
            monitors: vec![],
            workspaces: vec![],
            active_workspace: None,
            active_window: None,
            connected: false,
            detected: false,
        });
        assert!(update.matches_widget_type("workspaces"));
        assert!(!update.matches_widget_type("cpu"));
    }
}
