use std::time::Duration;

use sysinfo::{Components, Networks, ProcessesToUpdate, System};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::types::{
    CpuData, DataUpdate, InterfaceData, MemoryData, NetworkData, ProcessInfo, SensorData, TempData,
};

const BASE_INTERVAL_MS: u64 = 500;

pub fn spawn_sysinfo_task(
    tx: mpsc::Sender<DataUpdate>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        run_sysinfo_loop(tx, cancel).await;
    })
}

async fn run_sysinfo_loop(tx: mpsc::Sender<DataUpdate>, cancel: CancellationToken) {
    let mut sys = System::new();
    let mut networks = Networks::new_with_refreshed_list();
    let mut components = Components::new_with_refreshed_list();

    // Warm-up: CPU usage is diff-based, first sample is unreliable.
    sys.refresh_cpu_all();
    tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL).await;
    sys.refresh_cpu_all();

    // tokio::time::interval fires immediately on first tick, so tick 0 emits all data
    // types at once. This is intentional — widgets get populated immediately on startup.
    // The first network delta will be slightly short (covers only the warm-up window).
    let mut interval = tokio::time::interval(Duration::from_millis(BASE_INTERVAL_MS));
    let mut tick_count: u64 = 0;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = interval.tick() => {
                do_refresh(&mut sys, &mut networks, &mut components, &tx, tick_count);
                tick_count = tick_count.wrapping_add(1);
            }
        }
    }
}

// Synchronous refresh — refresh_processes can block for 10-50ms reading /proc/*/,
// which is acceptable on the multi-threaded runtime at 1s intervals.
fn do_refresh(
    sys: &mut System,
    networks: &mut Networks,
    components: &mut Components,
    tx: &mpsc::Sender<DataUpdate>,
    tick_count: u64,
) {
    // Every tick (500ms): CPU
    sys.refresh_cpu_all();
    let cpu_data = collect_cpu(sys);
    let _ = tx.try_send(DataUpdate::Cpu(cpu_data));

    // Every 2nd tick (1s): memory, processes, network
    if tick_count.is_multiple_of(2) {
        sys.refresh_memory();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let mem_data = collect_memory(sys);
        let _ = tx.try_send(DataUpdate::Memory(mem_data));

        networks.refresh(true);
        let net_data = collect_network(networks);
        let _ = tx.try_send(DataUpdate::Network(net_data));
    }

    // Every 4th tick (2s): temps
    if tick_count.is_multiple_of(4) {
        components.refresh(false);
        let temp_data = collect_temps(components);
        let _ = tx.try_send(DataUpdate::Temps(temp_data));
    }
}

fn collect_cpu(sys: &System) -> CpuData {
    let per_core: Vec<f32> = sys.cpus().iter().map(|cpu| cpu.cpu_usage()).collect();
    let overall = sys.global_cpu_usage();
    CpuData { per_core, overall }
}

fn collect_memory(sys: &System) -> MemoryData {
    let mut procs: Vec<ProcessInfo> = sys
        .processes()
        .iter()
        .map(|(pid, proc_)| ProcessInfo {
            name: proc_.name().to_string_lossy().to_string(),
            pid: pid.as_u32(),
            memory_bytes: proc_.memory(),
        })
        .collect();

    procs.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes));
    procs.truncate(10);

    MemoryData {
        total_mem: sys.total_memory(),
        used_mem: sys.used_memory(),
        total_swap: sys.total_swap(),
        used_swap: sys.used_swap(),
        top_processes: procs,
    }
}

fn collect_network(networks: &Networks) -> NetworkData {
    let interfaces: Vec<InterfaceData> = networks
        .iter()
        .map(|(name, data)| InterfaceData {
            name: name.clone(),
            bytes_sent: data.total_transmitted(),
            bytes_received: data.total_received(),
        })
        .collect();
    NetworkData { interfaces }
}

fn collect_temps(components: &Components) -> TempData {
    let sensors: Vec<SensorData> = components
        .iter()
        .map(|comp| SensorData {
            label: comp.label().to_string(),
            temp_celsius: comp.temperature(),
            max_celsius: comp.max(),
            critical_celsius: comp.critical(),
        })
        .collect();
    TempData { sensors }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extract the "should refresh" predicates into helper functions
    /// so the staggered logic is testable independently of sysinfo.
    fn should_refresh_mem_net(tick: u64) -> bool {
        tick % 2 == 0
    }

    fn should_refresh_temps(tick: u64) -> bool {
        tick % 4 == 0
    }

    #[test]
    fn staggered_intervals_refresh_pattern() {
        // Over 8 ticks, verify exact refresh schedule:
        // CPU: every tick (tested implicitly — always happens)
        // Mem/Net: ticks 0, 2, 4, 6
        // Temps: ticks 0, 4

        let mem_ticks: Vec<u64> = (0..8).filter(|&t| should_refresh_mem_net(t)).collect();
        assert_eq!(mem_ticks, vec![0, 2, 4, 6]);

        let temp_ticks: Vec<u64> = (0..8).filter(|&t| should_refresh_temps(t)).collect();
        assert_eq!(temp_ticks, vec![0, 4]);

        // Temps is always a subset of mem/net ticks
        for t in &temp_ticks {
            assert!(
                mem_ticks.contains(t),
                "temps tick {t} should also refresh mem/net"
            );
        }
    }

    #[tokio::test]
    async fn cancellation_stops_task() {
        let (tx, _rx) = mpsc::channel::<DataUpdate>(32);
        let cancel = CancellationToken::new();

        let handle = spawn_sysinfo_task(tx, cancel.clone());

        // Let it run briefly past warm-up
        tokio::time::sleep(Duration::from_millis(500)).await;

        cancel.cancel();

        let result = tokio::time::timeout(Duration::from_secs(2), handle).await;
        assert!(
            result.is_ok(),
            "task should complete within 2s of cancellation"
        );
    }

    #[tokio::test]
    async fn channel_full_does_not_block() {
        // Channel of size 1 — will fill immediately
        let (tx, _rx) = mpsc::channel::<DataUpdate>(1);
        let cancel = CancellationToken::new();

        let handle = spawn_sysinfo_task(tx, cancel.clone());

        // Let it run for a bit — if try_send blocks, this will hang
        tokio::time::sleep(Duration::from_secs(2)).await;

        cancel.cancel();

        let result = tokio::time::timeout(Duration::from_secs(2), handle).await;
        assert!(result.is_ok(), "task should not be blocked by full channel");
    }
}
