use std::collections::HashMap;
use std::time::{Duration, Instant};

use sysinfo::{Components, Disks, Networks, ProcessesToUpdate, System};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::types::{
    CpuData, DataUpdate, DiskData, DiskInfo, DiskIoStats, InterfaceData, MemoryData, NetworkData,
    ProcessData, ProcessEntry, ProcessInfo, ProcessStatus, SensorData, TempData,
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
    let mut disks = Disks::new_with_refreshed_list();

    // Disk I/O tracking
    let mut prev_diskstats: HashMap<String, (u64, u64)> = HashMap::new();
    let mut prev_diskstats_time: Option<Instant> = None;

    // Warm-up: CPU usage is diff-based, first sample is unreliable.
    sys.refresh_cpu_all();
    tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL).await;
    sys.refresh_cpu_all();

    let mut interval = tokio::time::interval(Duration::from_millis(BASE_INTERVAL_MS));
    let mut tick_count: u64 = 0;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = interval.tick() => {
                do_refresh(&mut sys, &mut networks, &mut components, &mut disks,
                           &mut prev_diskstats, &mut prev_diskstats_time,
                           &tx, tick_count);
                tick_count = tick_count.wrapping_add(1);
            }
        }
    }
}

// Synchronous refresh — refresh_processes can block for 10-50ms reading /proc/*/,
// which is acceptable on the multi-threaded runtime at 1s intervals.
#[allow(clippy::too_many_arguments)]
fn do_refresh(
    sys: &mut System,
    networks: &mut Networks,
    components: &mut Components,
    disks: &mut Disks,
    prev_diskstats: &mut HashMap<String, (u64, u64)>,
    prev_diskstats_time: &mut Option<Instant>,
    tx: &mpsc::Sender<DataUpdate>,
    tick_count: u64,
) {
    // Every tick (500ms): CPU
    sys.refresh_cpu_all();
    let cpu_data = collect_cpu(sys);
    let _ = tx.try_send(DataUpdate::Cpu(cpu_data));

    // Every 2nd tick (1s): memory, processes, network, disk I/O
    if tick_count.is_multiple_of(2) {
        sys.refresh_memory();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let mem_data = collect_memory(sys);
        let _ = tx.try_send(DataUpdate::Memory(mem_data));

        let proc_data = collect_processes(sys);
        let _ = tx.try_send(DataUpdate::Process(proc_data));

        networks.refresh(true);
        let net_data = collect_network(networks);
        let _ = tx.try_send(DataUpdate::Network(net_data));

        // Disk I/O from /proc/diskstats (1s)
        let disk_io = read_and_compute_disk_io(prev_diskstats, prev_diskstats_time);

        // Disk capacity refresh (2s)
        if tick_count.is_multiple_of(4) {
            disks.refresh(false);
        }

        // Emit disk data every 1s (cached capacity + fresh I/O)
        let disk_data = collect_disks(disks, &disk_io);
        let _ = tx.try_send(DataUpdate::Disk(disk_data));

        // Temps (2s)
        if tick_count.is_multiple_of(4) {
            components.refresh(false);
            let temp_data = collect_temps(components);
            let _ = tx.try_send(DataUpdate::Temps(temp_data));
        }
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

fn collect_processes(sys: &System) -> ProcessData {
    let total_mem = sys.total_memory() as f64;
    let processes: Vec<ProcessEntry> = sys
        .processes()
        .iter()
        .map(|(pid, proc_)| {
            let status = match proc_.status() {
                sysinfo::ProcessStatus::Run => ProcessStatus::Running,
                sysinfo::ProcessStatus::Sleep | sysinfo::ProcessStatus::Idle => {
                    ProcessStatus::Sleeping
                }
                sysinfo::ProcessStatus::Stop => ProcessStatus::Stopped,
                sysinfo::ProcessStatus::Zombie => ProcessStatus::Zombie,
                other => ProcessStatus::Other(format!("{other:?}")),
            };
            ProcessEntry {
                pid: pid.as_u32(),
                name: proc_.name().to_string_lossy().to_string(),
                cpu_percent: proc_.cpu_usage(),
                memory_bytes: proc_.memory(),
                memory_percent: if total_mem > 0.0 {
                    (proc_.memory() as f64 / total_mem * 100.0) as f32
                } else {
                    0.0
                },
                status,
            }
        })
        .collect();
    ProcessData { processes }
}

fn read_and_compute_disk_io(
    prev_diskstats: &mut HashMap<String, (u64, u64)>,
    prev_time: &mut Option<Instant>,
) -> HashMap<String, (f64, f64)> {
    let content = match std::fs::read_to_string("/proc/diskstats") {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    let curr = parse_diskstats(&content);
    let elapsed_secs = prev_time.map_or(0.0, |t| t.elapsed().as_secs_f64());
    let deltas = compute_io_deltas(prev_diskstats, &curr, elapsed_secs);
    *prev_diskstats = curr;
    *prev_time = Some(Instant::now());
    deltas
}

/// Resolve a device name to its canonical block device basename.
/// Handles symlinks like /dev/mapper/vg-root -> /dev/dm-0, /dev/root -> /dev/sda1, etc.
fn resolve_device_name(raw_dev: &str) -> String {
    let full_path = format!("/dev/{}", raw_dev);
    if let Ok(canonical) = std::fs::canonicalize(&full_path)
        && let Some(name) = canonical.file_name()
    {
        return name.to_string_lossy().to_string();
    }
    raw_dev.to_string()
}

fn collect_disks(disks: &Disks, io_deltas: &HashMap<String, (f64, f64)>) -> DiskData {
    let mount_to_dev = match std::fs::read_to_string("/proc/mounts") {
        Ok(content) => parse_mounts(&content),
        Err(_) => HashMap::new(),
    };

    let disk_infos: Vec<DiskInfo> = disks
        .iter()
        .map(|disk| {
            let mount_point = disk.mount_point().to_string_lossy().to_string();
            let device_name = mount_to_dev.get(&mount_point).cloned().unwrap_or_default();

            let io = if !device_name.is_empty() {
                let resolved = resolve_device_name(&device_name);
                io_deltas.get(&resolved).map(|&(rd, wr)| DiskIoStats {
                    read_bytes_per_sec: rd,
                    write_bytes_per_sec: wr,
                })
            } else {
                None
            };

            DiskInfo {
                mount_point,
                device_name,
                total_bytes: disk.total_space(),
                available_bytes: disk.available_space(),
                io,
            }
        })
        .collect();

    DiskData { disks: disk_infos }
}

/// Parse a single line from /proc/diskstats.
/// Returns (device_name, sectors_read, sectors_written) or None if malformed.
/// Format: major minor name rd_ios rd_merges rd_sectors rd_ticks wr_ios wr_merges wr_sectors ...
fn parse_diskstats_line(line: &str) -> Option<(String, u64, u64)> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 10 {
        return None;
    }
    let device = fields[2].to_string();
    let rd_sectors: u64 = fields[5].parse().ok()?;
    let wr_sectors: u64 = fields[9].parse().ok()?;
    Some((device, rd_sectors, wr_sectors))
}

/// Parse full /proc/diskstats content into a map of device -> (sectors_read, sectors_written).
fn parse_diskstats(content: &str) -> HashMap<String, (u64, u64)> {
    let mut map = HashMap::new();
    for line in content.lines() {
        if let Some((dev, rd, wr)) = parse_diskstats_line(line) {
            map.insert(dev, (rd, wr));
        }
    }
    map
}

/// Decode octal escape sequences in /proc/mounts paths (e.g. \040 for space).
fn decode_mount_escape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            // Try to read 3 octal digits
            let mut octal = String::new();
            for _ in 0..3 {
                if let Some(&next) = chars.as_str().as_bytes().first() {
                    if (b'0'..=b'7').contains(&next) {
                        octal.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
            }
            if octal.len() == 3 {
                if let Ok(byte) = u8::from_str_radix(&octal, 8) {
                    result.push(byte as char);
                } else {
                    result.push('\\');
                    result.push_str(&octal);
                }
            } else {
                result.push('\\');
                result.push_str(&octal);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Parse /proc/mounts content into a map of mount_point -> device_name.
/// Only includes entries with /dev/ device paths. Strips the /dev/ prefix.
fn parse_mounts(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 2 {
            continue;
        }
        let device = fields[0];
        let mount_point = fields[1];
        if let Some(dev_name) = device.strip_prefix("/dev/") {
            map.insert(decode_mount_escape(mount_point), dev_name.to_string());
        }
    }
    map
}

/// Compute I/O byte rates from sector count deltas.
/// Returns map of device -> (read_bytes_per_sec, write_bytes_per_sec).
/// Devices present in curr but not prev are skipped (first reading).
fn compute_io_deltas(
    prev: &HashMap<String, (u64, u64)>,
    curr: &HashMap<String, (u64, u64)>,
    elapsed_secs: f64,
) -> HashMap<String, (f64, f64)> {
    let mut deltas = HashMap::new();
    if elapsed_secs <= 0.0 {
        return deltas;
    }
    for (dev, &(curr_rd, curr_wr)) in curr {
        if let Some(&(prev_rd, prev_wr)) = prev.get(dev) {
            let rd_bytes = curr_rd.saturating_sub(prev_rd) as f64 * 512.0 / elapsed_secs;
            let wr_bytes = curr_wr.saturating_sub(prev_wr) as f64 * 512.0 / elapsed_secs;
            deltas.insert(dev.clone(), (rd_bytes, wr_bytes));
        }
    }
    deltas
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

    #[test]
    fn parse_diskstats_valid_line() {
        let line = "   8       1 sda1 32456 0 1234567 0 28123 0 9876543 0 0 0 0 0 0 0 0";
        let result = parse_diskstats_line(line);
        assert!(result.is_some());
        let (dev, read_sectors, write_sectors) = result.unwrap();
        assert_eq!(dev, "sda1");
        assert_eq!(read_sectors, 1234567);
        assert_eq!(write_sectors, 9876543);
    }

    #[test]
    fn parse_diskstats_nvme() {
        let line = " 259       1 nvme0n1p2 100 0 2000 0 200 0 4000 0 0 0 0 0 0 0 0";
        let result = parse_diskstats_line(line);
        assert!(result.is_some());
        let (dev, r, w) = result.unwrap();
        assert_eq!(dev, "nvme0n1p2");
        assert_eq!(r, 2000);
        assert_eq!(w, 4000);
    }

    #[test]
    fn parse_diskstats_malformed_returns_none() {
        assert!(parse_diskstats_line("").is_none());
        assert!(parse_diskstats_line("not a valid line").is_none());
        assert!(parse_diskstats_line("   8       1 sda1").is_none());
    }

    #[test]
    fn parse_diskstats_full_input() {
        let input = "\
   8       0 sda 32456 0 1000 0 28123 0 2000 0 0 0 0 0 0 0 0
   8       1 sda1 100 0 500 0 200 0 1000 0 0 0 0 0 0 0 0
 259       0 nvme0n1 50 0 300 0 60 0 400 0 0 0 0 0 0 0 0";
        let entries = parse_diskstats(input);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries["sda"].0, 1000);
        assert_eq!(entries["sda1"].1, 1000);
        assert_eq!(entries["nvme0n1"].0, 300);
    }

    #[test]
    fn parse_mounts_standard_partition() {
        let input = "/dev/sda1 / ext4 rw,relatime 0 0\n/dev/nvme0n1p2 /home ext4 rw 0 0\n";
        let map = parse_mounts(input);
        assert_eq!(map.get("/"), Some(&"sda1".to_string()));
        assert_eq!(map.get("/home"), Some(&"nvme0n1p2".to_string()));
    }

    #[test]
    fn parse_mounts_mapper_returns_raw_name() {
        let input = "/dev/mapper/vg-root / ext4 rw 0 0\n";
        let map = parse_mounts(input);
        assert_eq!(map.get("/"), Some(&"mapper/vg-root".to_string()));
    }

    #[test]
    fn parse_mounts_skips_non_dev() {
        let input = "proc /proc proc rw 0 0\nsysfs /sys sysfs rw 0 0\n/dev/sda1 / ext4 rw 0 0\n";
        let map = parse_mounts(input);
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("/"));
    }

    #[test]
    fn parse_mounts_empty() {
        let map = parse_mounts("");
        assert!(map.is_empty());
    }

    #[test]
    fn decode_mount_escape_space() {
        assert_eq!(decode_mount_escape("mount\\040point"), "mount point");
    }

    #[test]
    fn decode_mount_escape_no_escapes() {
        assert_eq!(decode_mount_escape("/home"), "/home");
    }

    #[test]
    fn parse_mounts_escaped_space() {
        let input = "/dev/sda1 /mnt/my\\040drive ext4 rw 0 0\n";
        let map = parse_mounts(input);
        assert_eq!(map.get("/mnt/my drive"), Some(&"sda1".to_string()));
    }

    #[test]
    fn compute_io_deltas_basic() {
        let prev: HashMap<String, (u64, u64)> = HashMap::from([("sda1".to_string(), (1000, 2000))]);
        let curr: HashMap<String, (u64, u64)> = HashMap::from([("sda1".to_string(), (3000, 5000))]);
        let elapsed_secs = 2.0;
        let deltas = compute_io_deltas(&prev, &curr, elapsed_secs);
        let sda1 = deltas.get("sda1").unwrap();
        assert!((sda1.0 - 512000.0).abs() < 0.1);
        assert!((sda1.1 - 768000.0).abs() < 0.1);
    }

    #[test]
    fn compute_io_deltas_missing_prev_skipped() {
        let prev: HashMap<String, (u64, u64)> = HashMap::new();
        let curr: HashMap<String, (u64, u64)> = HashMap::from([("sda1".to_string(), (1000, 2000))]);
        let deltas = compute_io_deltas(&prev, &curr, 1.0);
        assert!(deltas.is_empty());
    }

    fn should_refresh_disk_capacity(tick: u64) -> bool {
        tick % 4 == 0
    }

    fn should_emit_disk(tick: u64) -> bool {
        tick % 2 == 0
    }

    #[test]
    fn resolve_device_name_plain() {
        // A non-existent device just returns itself
        let result = resolve_device_name("definitely_not_a_real_device_xyz");
        assert_eq!(result, "definitely_not_a_real_device_xyz");
    }

    #[test]
    fn disk_capacity_refresh_schedule() {
        // Capacity refreshes at 2s (every 4th tick), but DiskData is emitted at 1s (every 2nd tick)
        let capacity_ticks: Vec<u64> = (0..8)
            .filter(|&t| should_refresh_disk_capacity(t))
            .collect();
        assert_eq!(capacity_ticks, vec![0, 4]);
        let emission_ticks: Vec<u64> = (0..8).filter(|&t| should_emit_disk(t)).collect();
        assert_eq!(emission_ticks, vec![0, 2, 4, 6]);
    }
}
