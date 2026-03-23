pub mod cpu;
pub mod disk;
pub mod memory;
pub mod network;
pub mod placeholder;
pub mod processes;
pub mod temps;
pub mod util;

pub use cpu::CpuWidget;
pub use disk::DiskWidget;
pub use memory::MemoryWidget;
pub use network::NetworkWidget;
pub use placeholder::PlaceholderWidget;
pub use processes::ProcessWidget;
pub use temps::TempsWidget;
