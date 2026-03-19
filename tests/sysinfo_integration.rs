use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use mise_tui::data::DataUpdate;
use mise_tui::data::spawn_sysinfo_task;

#[tokio::test]
async fn sysinfo_task_produces_all_data_types() {
    let (tx, mut rx) = mpsc::channel::<DataUpdate>(32);
    let cancel = CancellationToken::new();

    let _handle = spawn_sysinfo_task(tx, cancel.clone());

    let mut got_cpu = false;
    let mut got_memory = false;
    let mut got_network = false;
    let mut got_temps = false;

    let deadline = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = &mut deadline => break,
            Some(update) = rx.recv() => {
                match update {
                    DataUpdate::Cpu(data) => {
                        assert!(!data.per_core.is_empty(), "should have at least 1 CPU core");
                        for &usage in &data.per_core {
                            assert!((0.0..=100.0).contains(&usage), "CPU usage should be 0-100, got {usage}");
                        }
                        got_cpu = true;
                    }
                    DataUpdate::Memory(data) => {
                        assert!(data.total_mem > 0, "total memory should be > 0");
                        assert!(data.used_mem <= data.total_mem, "used <= total");
                        got_memory = true;
                    }
                    DataUpdate::Network(_) => {
                        got_network = true;
                    }
                    DataUpdate::Temps(_) => {
                        got_temps = true;
                    }
                    _ => {}
                }

                if got_cpu && got_memory && got_network && got_temps {
                    break;
                }
            }
        }
    }

    cancel.cancel();

    assert!(got_cpu, "should have received CpuData");
    assert!(got_memory, "should have received MemoryData");
    assert!(got_network, "should have received NetworkData");
    assert!(got_temps, "should have received TempData");
}
