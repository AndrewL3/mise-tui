use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::data::types::{DataUpdate, HyprlandData, MonitorInfo, WorkspaceId, WorkspaceInfo};

/// Parse `hyprctl monitors -j` JSON output into a list of monitors.
pub fn parse_monitors(json: &str) -> Vec<MonitorInfo> {
    let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return vec![];
    };
    values
        .iter()
        .filter_map(|v| {
            Some(MonitorInfo {
                id: v.get("id")?.as_i64()? as i32,
                name: v.get("name")?.as_str()?.to_string(),
                active_workspace_id: v
                    .get("activeWorkspace")
                    .and_then(|w| w.get("id"))
                    .and_then(|id| id.as_i64())?
                    as WorkspaceId,
            })
        })
        .collect()
}

/// Parse `hyprctl workspaces -j` JSON output into a list of workspaces.
pub fn parse_workspaces(json: &str) -> Vec<WorkspaceInfo> {
    let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return vec![];
    };
    values
        .iter()
        .filter_map(|v| {
            Some(WorkspaceInfo {
                id: v.get("id")?.as_i64()? as WorkspaceId,
                name: v.get("name")?.as_str()?.to_string(),
                monitor: v.get("monitor")?.as_str()?.to_string(),
                window_count: v.get("windows")?.as_u64()? as u32,
            })
        })
        .collect()
}

/// Parse `hyprctl activewindow -j` JSON output into a window title.
pub fn parse_active_window(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    value.get("title")?.as_str().map(|s| s.to_string())
}

/// Check if a Hyprland event line is workspace-relevant and should trigger a refresh.
pub fn is_workspace_event(event_line: &str) -> bool {
    let event_type = event_line.split(">>").next().unwrap_or("");
    matches!(
        event_type,
        "workspace"
            | "createworkspace"
            | "createworkspacev2"
            | "destroyworkspace"
            | "destroyworkspacev2"
            | "moveworkspace"
            | "moveworkspacev2"
            | "activewindow"
            | "activewindowv2"
            | "focusedmon"
    )
}

/// Resolve the Hyprland socket directory from environment.
fn socket_dir() -> Option<std::path::PathBuf> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    Some(
        std::path::PathBuf::from(runtime_dir)
            .join("hypr")
            .join(signature),
    )
}

/// Send a request to Hyprland's request socket and return the response.
async fn hyprctl_request(socket_dir: &std::path::Path, command: &str) -> Option<String> {
    let socket_path = socket_dir.join(".socket.sock");
    let mut stream = UnixStream::connect(&socket_path).await.ok()?;
    stream.write_all(command.as_bytes()).await.ok()?;
    stream.shutdown().await.ok()?;

    let mut response = String::new();
    tokio::io::AsyncReadExt::read_to_string(&mut stream, &mut response)
        .await
        .ok()?;
    Some(response)
}

/// Fetch full Hyprland state via the request socket.
async fn fetch_state(socket_dir: &std::path::Path) -> HyprlandData {
    let monitors = match hyprctl_request(socket_dir, "j/monitors").await {
        Some(json) => parse_monitors(&json),
        None => vec![],
    };

    let workspaces = match hyprctl_request(socket_dir, "j/workspaces").await {
        Some(json) => parse_workspaces(&json),
        None => vec![],
    };

    let active_window = match hyprctl_request(socket_dir, "j/activewindow").await {
        Some(json) => parse_active_window(&json),
        None => None,
    };

    let active_workspace = monitors.first().map(|m| m.active_workspace_id);

    HyprlandData {
        monitors,
        workspaces,
        active_workspace,
        active_window,
        connected: true,
        detected: true,
    }
}

/// Spawn the Hyprland IPC task. Returns a JoinHandle.
///
/// The task connects to Hyprland's event socket for real-time workspace updates,
/// with a 1s fallback poll. If Hyprland is not detected (no `HYPRLAND_INSTANCE_SIGNATURE`),
/// sends a single "not detected" update and exits.
pub fn spawn_hyprland_task(
    tx: mpsc::Sender<DataUpdate>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Check if Hyprland is available
        let sock_dir = match socket_dir() {
            Some(d) => d,
            None => {
                let _ = tx
                    .send(DataUpdate::Hyprland(HyprlandData {
                        monitors: vec![],
                        workspaces: vec![],
                        active_workspace: None,
                        active_window: None,
                        connected: false,
                        detected: false,
                    }))
                    .await;
                return;
            }
        };

        // Fetch initial state
        let state = fetch_state(&sock_dir).await;
        if tx.send(DataUpdate::Hyprland(state)).await.is_err() {
            return;
        }

        // Connect to event socket with reconnection loop
        let mut backoff_secs = 1u64;
        loop {
            let event_socket_path = sock_dir.join(".socket2.sock");
            if let Ok(stream) = UnixStream::connect(&event_socket_path).await {
                backoff_secs = 1;
                let reader = BufReader::new(stream);
                let mut lines = reader.lines();
                let mut poll_interval = tokio::time::interval(Duration::from_secs(1));
                poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => return,
                        line = lines.next_line() => {
                            match line {
                                Ok(Some(event_line)) => {
                                    if is_workspace_event(&event_line) {
                                        let state = fetch_state(&sock_dir).await;
                                        if tx.send(DataUpdate::Hyprland(state)).await.is_err() {
                                            return;
                                        }
                                    }
                                }
                                Ok(None) | Err(_) => break,
                            }
                        }
                        _ = poll_interval.tick() => {
                            let state = fetch_state(&sock_dir).await;
                            if tx.send(DataUpdate::Hyprland(state)).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            }

            // Send disconnected state
            let _ = tx
                .send(DataUpdate::Hyprland(HyprlandData {
                    monitors: vec![],
                    workspaces: vec![],
                    active_workspace: None,
                    active_window: None,
                    connected: false,
                    detected: true,
                }))
                .await;

            // Backoff before retry
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {}
            }
            backoff_secs = (backoff_secs * 2).min(30);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_monitors_valid() {
        let json = r#"[
            {
                "id": 0,
                "name": "DP-1",
                "activeWorkspace": {"id": 1, "name": "1"}
            },
            {
                "id": 1,
                "name": "HDMI-A-1",
                "activeWorkspace": {"id": 3, "name": "3"}
            }
        ]"#;
        let monitors = parse_monitors(json);
        assert_eq!(monitors.len(), 2);
        assert_eq!(monitors[0].name, "DP-1");
        assert_eq!(monitors[0].active_workspace_id, 1);
        assert_eq!(monitors[1].name, "HDMI-A-1");
        assert_eq!(monitors[1].active_workspace_id, 3);
    }

    #[test]
    fn parse_monitors_empty() {
        assert!(parse_monitors("[]").is_empty());
    }

    #[test]
    fn parse_monitors_invalid_json() {
        assert!(parse_monitors("not json").is_empty());
    }

    #[test]
    fn parse_workspaces_valid() {
        let json = r#"[
            {
                "id": 1,
                "name": "1",
                "monitor": "DP-1",
                "windows": 3
            },
            {
                "id": 2,
                "name": "2",
                "monitor": "DP-1",
                "windows": 0
            }
        ]"#;
        let workspaces = parse_workspaces(json);
        assert_eq!(workspaces.len(), 2);
        assert_eq!(workspaces[0].id, 1);
        assert_eq!(workspaces[0].window_count, 3);
        assert_eq!(workspaces[1].window_count, 0);
    }

    #[test]
    fn parse_workspaces_empty() {
        assert!(parse_workspaces("[]").is_empty());
    }

    #[test]
    fn parse_active_window_valid() {
        let json = r#"{"class": "firefox", "title": "GitHub - Mozilla Firefox"}"#;
        let title = parse_active_window(json);
        assert_eq!(title.as_deref(), Some("GitHub - Mozilla Firefox"));
    }

    #[test]
    fn parse_active_window_no_window() {
        let json = r#"{}"#;
        assert!(parse_active_window(json).is_none());
    }

    #[test]
    fn parse_active_window_invalid_json() {
        assert!(parse_active_window("not json").is_none());
    }

    #[test]
    fn workspace_event_detection() {
        assert!(is_workspace_event("workspace>>1"));
        assert!(is_workspace_event("createworkspace>>2"));
        assert!(is_workspace_event("destroyworkspace>>3"));
        assert!(is_workspace_event("moveworkspace>>4,DP-1"));
        assert!(is_workspace_event("activewindow>>firefox,Tab"));
        assert!(is_workspace_event("focusedmon>>DP-1,1"));
        assert!(!is_workspace_event("openwindow>>abc123"));
        assert!(!is_workspace_event("closewindow>>abc123"));
        assert!(!is_workspace_event(""));
    }
}
