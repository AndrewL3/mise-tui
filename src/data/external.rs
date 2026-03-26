use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::data::command::{CommandError, run_command};
use crate::data::types::{
    ActiveState, DataUpdate, PackageUpdate, PackagesData, PackagesResult, ServiceStatus,
    ServicesData, ServicesResult,
};

/// Maximum bytes of output accepted from external commands (64 KiB).
const DEFAULT_MAX_OUTPUT_BYTES: usize = 65_536;

// ---------------------------------------------------------------------------
// Spawn functions
// ---------------------------------------------------------------------------

/// Spawn a tokio task that periodically runs `checkupdates` and sends
/// `DataUpdate::Packages` over `tx`.
///
/// `checkupdates` exits with code 2 when there are no updates available —
/// this is treated as success with an empty update list, not as an error.
pub fn spawn_packages_task(
    instance_id: String,
    interval: Duration,
    timeout: Duration,
    tx: mpsc::Sender<DataUpdate>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let argv: Vec<String> = vec!["checkupdates".to_string()];

        loop {
            let result = match run_command(&argv, timeout, DEFAULT_MAX_OUTPUT_BYTES).await {
                Ok(cmd_result) => Ok(PackagesData {
                    updates: parse_checkupdates(&cmd_result.stdout),
                }),
                Err(CommandError::Failed { exit_code: 2, .. }) => {
                    // Exit code 2 means no updates available — not an error.
                    Ok(PackagesData { updates: vec![] })
                }
                Err(e) => Err(e.to_string()),
            };

            let update = DataUpdate::Packages(PackagesResult {
                instance_id: instance_id.clone(),
                data: result,
            });

            if tx.send(update).await.is_err() {
                break;
            }

            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(interval) => {}
            }
        }
    })
}

/// Spawn a tokio task that periodically runs `systemctl show` and sends
/// `DataUpdate::Services` over `tx`.
pub fn spawn_services_task(
    instance_id: String,
    scope: String,
    services: Vec<String>,
    interval: Duration,
    timeout: Duration,
    tx: mpsc::Sender<DataUpdate>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut argv: Vec<String> = vec!["systemctl".to_string()];
        if scope == "user" {
            argv.push("--user".to_string());
        }
        argv.push("show".to_string());
        argv.push("--property=ActiveState,SubState".to_string());
        argv.push("--".to_string());
        for svc in &services {
            argv.push(svc.clone());
        }

        loop {
            let result = match run_command(&argv, timeout, DEFAULT_MAX_OUTPUT_BYTES).await {
                Ok(cmd_result) => Ok(ServicesData {
                    services: parse_systemctl_show(&cmd_result.stdout, &services),
                }),
                Err(e) => Err(e.to_string()),
            };

            let update = DataUpdate::Services(ServicesResult {
                instance_id: instance_id.clone(),
                data: result,
            });

            if tx.send(update).await.is_err() {
                break;
            }

            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(interval) => {}
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------

/// Parse the output of `checkupdates`.
///
/// Each line has the format: `package_name old_version -> new_version`.
/// Malformed or empty lines are silently skipped.
fn parse_checkupdates(output: &str) -> Vec<PackageUpdate> {
    let mut updates = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Split on " -> " to separate left part from new_version.
        let Some((left, new_version)) = line.split_once(" -> ") else {
            continue;
        };
        let new_version = new_version.trim();
        if new_version.is_empty() {
            continue;
        }
        // The left part is "name old_version". Split on the last space.
        let left = left.trim();
        let Some(last_space) = left.rfind(' ') else {
            continue;
        };
        let name = left[..last_space].trim();
        let old_version = left[last_space + 1..].trim();
        if name.is_empty() || old_version.is_empty() {
            continue;
        }
        updates.push(PackageUpdate {
            name: name.to_string(),
            old_version: old_version.to_string(),
            new_version: new_version.to_string(),
        });
    }
    updates
}

/// Parse the output of `systemctl show --property=ActiveState,SubState -- svc1 svc2 ...`.
///
/// Output blocks are separated by `\n\n`. Each block contains key=value lines.
/// Block N corresponds to `service_names[N]`. If `ActiveState` is missing,
/// `ActiveState::Other("unknown")` is used.
fn parse_systemctl_show(output: &str, service_names: &[String]) -> Vec<ServiceStatus> {
    let blocks: Vec<&str> = if output.trim().is_empty() {
        vec![]
    } else {
        output.split("\n\n").collect()
    };

    service_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let block = blocks.get(i).unwrap_or(&"");
            let mut active_state_str: Option<&str> = None;
            let mut sub_state_str: &str = "";

            for line in block.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("ActiveState=") {
                    active_state_str = Some(val);
                } else if let Some(val) = line.strip_prefix("SubState=") {
                    sub_state_str = val;
                }
            }

            let active_state = match active_state_str {
                Some("active") => ActiveState::Active,
                Some("inactive") => ActiveState::Inactive,
                Some("failed") => ActiveState::Failed,
                Some("activating") => ActiveState::Activating,
                Some("deactivating") => ActiveState::Deactivating,
                Some(other) => ActiveState::Other(other.to_string()),
                None => ActiveState::Other("unknown".to_string()),
            };

            ServiceStatus {
                name: name.clone(),
                active_state,
                sub_state: sub_state_str.to_string(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- checkupdates parser tests --

    #[test]
    fn parse_checkupdates_standard() {
        let output = "linux 6.8.1-arch1-1 -> 6.8.2-arch1-1\nfirefox 124.0-1 -> 125.0-1\n";
        let updates = parse_checkupdates(output);
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].name, "linux");
        assert_eq!(updates[0].old_version, "6.8.1-arch1-1");
        assert_eq!(updates[0].new_version, "6.8.2-arch1-1");
        assert_eq!(updates[1].name, "firefox");
        assert_eq!(updates[1].old_version, "124.0-1");
        assert_eq!(updates[1].new_version, "125.0-1");
    }

    #[test]
    fn parse_checkupdates_empty() {
        let updates = parse_checkupdates("");
        assert!(updates.is_empty());
    }

    #[test]
    fn parse_checkupdates_malformed_skipped() {
        let output = "good-pkg 1.0 -> 2.0\nmalformed line without arrow\n-> also bad\n";
        let updates = parse_checkupdates(output);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].name, "good-pkg");
    }

    #[test]
    fn parse_checkupdates_trailing_whitespace() {
        let output = "  linux 6.8.1 -> 6.8.2  \n";
        let updates = parse_checkupdates(output);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].name, "linux");
        assert_eq!(updates[0].old_version, "6.8.1");
        assert_eq!(updates[0].new_version, "6.8.2");
    }

    // -- systemctl show parser tests --

    #[test]
    fn parse_systemctl_two_active() {
        let output = "ActiveState=active\nSubState=running\n\nActiveState=active\nSubState=running";
        let names = vec!["sshd.service".to_string(), "nginx.service".to_string()];
        let statuses = parse_systemctl_show(output, &names);
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].name, "sshd.service");
        assert_eq!(statuses[0].active_state, ActiveState::Active);
        assert_eq!(statuses[0].sub_state, "running");
        assert_eq!(statuses[1].name, "nginx.service");
        assert_eq!(statuses[1].active_state, ActiveState::Active);
    }

    #[test]
    fn parse_systemctl_mixed_states() {
        let output = "ActiveState=active\nSubState=running\n\nActiveState=failed\nSubState=failed\n\nActiveState=inactive\nSubState=dead";
        let names = vec![
            "sshd.service".to_string(),
            "bad.service".to_string(),
            "stopped.service".to_string(),
        ];
        let statuses = parse_systemctl_show(output, &names);
        assert_eq!(statuses.len(), 3);
        assert_eq!(statuses[0].active_state, ActiveState::Active);
        assert_eq!(statuses[1].active_state, ActiveState::Failed);
        assert_eq!(statuses[1].sub_state, "failed");
        assert_eq!(statuses[2].active_state, ActiveState::Inactive);
        assert_eq!(statuses[2].sub_state, "dead");
    }

    #[test]
    fn parse_systemctl_missing_property() {
        let output = "SubState=running";
        let names = vec!["mystery.service".to_string()];
        let statuses = parse_systemctl_show(output, &names);
        assert_eq!(statuses.len(), 1);
        assert_eq!(
            statuses[0].active_state,
            ActiveState::Other("unknown".to_string())
        );
        assert_eq!(statuses[0].sub_state, "running");
    }

    #[test]
    fn parse_systemctl_empty() {
        let statuses = parse_systemctl_show("", &[]);
        assert!(statuses.is_empty());
    }
}
