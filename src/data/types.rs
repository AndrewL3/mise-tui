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
pub struct CpuData {}

#[derive(Debug, Clone)]
pub struct MemoryData {}

#[derive(Debug, Clone)]
pub struct NetworkData {}

#[derive(Debug, Clone)]
pub struct TempData {}

#[derive(Debug, Clone)]
pub struct ExternalData {}
