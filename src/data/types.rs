#[derive(Debug, Clone)]
pub enum DataUpdate {
    Cpu(CpuData),
    Memory(MemoryData),
    Network(NetworkData),
    Temps(TempData),
    External {
        instance_id: String,
        result: Result<ExternalData, String>,
    },
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
pub struct ExternalData {}

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
        ];
        assert_eq!(updates.len(), 4);
    }
}
