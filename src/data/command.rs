use std::io::ErrorKind;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Result of a successful command execution.
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub stdout: String,
    pub exit_code: Option<i32>,
}

/// Errors that can occur when running an external command.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("command not found: {0}")]
    NotFound(String),

    #[error("command timed out after {0:?}")]
    Timeout(Duration),

    #[error("output exceeded {0} byte cap")]
    OutputTooLarge(usize),

    #[error("command failed with exit code {exit_code}: {stderr}")]
    Failed { exit_code: i32, stderr: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Maximum stderr bytes retained in `CommandError::Failed`.
const MAX_STDERR_DISPLAY: usize = 1024;

/// Execute an external command safely with timeout and output size limits.
///
/// - `argv`: command and arguments (argv\[0\] is the program). Must be non-empty.
/// - `timeout`: maximum wall-clock duration before the process is killed.
/// - `max_output_bytes`: cap on stdout AND stderr individually. If either stream
///   exceeds this, the process is killed and `CommandError::OutputTooLarge` is returned.
///
/// The command runs without a shell (argv-only). stdin is closed immediately.
pub async fn run_command(
    argv: &[String],
    timeout: Duration,
    max_output_bytes: usize,
) -> Result<CommandResult, CommandError> {
    if argv.is_empty() {
        return Err(CommandError::Io(std::io::Error::new(
            ErrorKind::InvalidInput,
            "argv must not be empty",
        )));
    }

    let program = &argv[0];
    let args = &argv[1..];

    let mut child = match Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(CommandError::NotFound(program.clone()));
        }
        Err(e) => return Err(CommandError::Io(e)),
    };

    // Take the pipes before moving child into the async block.
    let stdout_pipe = child.stdout.take().expect("stdout was piped");
    let stderr_pipe = child.stderr.take().expect("stderr was piped");

    let cap = max_output_bytes;

    // Everything that touches `child` happens inside this future.
    // When the timeout fires, this future is dropped, which drops `child`,
    // which sends SIGKILL to the subprocess.
    let work = async move {
        let read_stream = |mut pipe: tokio::process::ChildStdout, label: &'static str| async move {
            let mut buf = [0u8; 4096];
            let mut output = Vec::new();
            loop {
                let n = pipe
                    .read(&mut buf)
                    .await
                    .map_err(|e| (label, StreamError::Io(e)))?;
                if n == 0 {
                    break;
                }
                output.extend_from_slice(&buf[..n]);
                if output.len() > cap {
                    return Err((label, StreamError::TooLarge));
                }
            }
            Ok::<Vec<u8>, (&str, StreamError)>(output)
        };

        let read_stderr = |mut pipe: tokio::process::ChildStderr, label: &'static str| async move {
            let mut buf = [0u8; 4096];
            let mut output = Vec::new();
            loop {
                let n = pipe
                    .read(&mut buf)
                    .await
                    .map_err(|e| (label, StreamError::Io(e)))?;
                if n == 0 {
                    break;
                }
                output.extend_from_slice(&buf[..n]);
                if output.len() > cap {
                    return Err((label, StreamError::TooLarge));
                }
            }
            Ok::<Vec<u8>, (&str, StreamError)>(output)
        };

        let (stdout_result, stderr_result) = tokio::join!(
            read_stream(stdout_pipe, "stdout"),
            read_stderr(stderr_pipe, "stderr"),
        );

        // Check for stream errors (propagate Io, convert TooLarge).
        let stdout_bytes = match stdout_result {
            Ok(v) => v,
            Err((_label, StreamError::Io(e))) => return Err(CommandError::Io(e)),
            Err((_label, StreamError::TooLarge)) => {
                // Kill the child — it may still be running.
                let _ = child.kill().await;
                return Err(CommandError::OutputTooLarge(cap));
            }
        };

        let stderr_bytes = match stderr_result {
            Ok(v) => v,
            Err((_label, StreamError::Io(e))) => return Err(CommandError::Io(e)),
            Err((_label, StreamError::TooLarge)) => {
                let _ = child.kill().await;
                return Err(CommandError::OutputTooLarge(cap));
            }
        };

        let status = child.wait().await?;

        if status.success() {
            let stdout = String::from_utf8_lossy(&stdout_bytes).into_owned();
            Ok(CommandResult {
                stdout,
                exit_code: status.code(),
            })
        } else {
            let exit_code = status.code().unwrap_or(-1);
            let mut stderr = String::from_utf8_lossy(&stderr_bytes).into_owned();
            stderr.truncate(MAX_STDERR_DISPLAY);
            Err(CommandError::Failed { exit_code, stderr })
        }
    };

    match tokio::time::timeout(timeout, work).await {
        Ok(result) => result,
        Err(_elapsed) => Err(CommandError::Timeout(timeout)),
    }
}

/// Internal error type to distinguish I/O failures from cap overflows during stream reads.
enum StreamError {
    Io(std::io::Error),
    TooLarge,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[tokio::test]
    async fn successful_command() {
        let result = run_command(&argv(&["echo", "hello"]), Duration::from_secs(5), 64_000).await;
        let ok = result.unwrap();
        assert_eq!(ok.stdout.trim(), "hello");
        assert_eq!(ok.exit_code, Some(0));
    }

    #[tokio::test]
    async fn nonzero_exit() {
        let result = run_command(&argv(&["false"]), Duration::from_secs(5), 64_000).await;
        assert!(matches!(result, Err(CommandError::Failed { .. })));
    }

    #[tokio::test]
    async fn command_not_found() {
        let result = run_command(
            &argv(&["nonexistent_binary_xyz_12345"]),
            Duration::from_secs(5),
            64_000,
        )
        .await;
        assert!(matches!(result, Err(CommandError::NotFound(_))));
    }

    #[tokio::test]
    async fn timeout_kills_process() {
        let result = run_command(&argv(&["sleep", "10"]), Duration::from_millis(100), 64_000).await;
        assert!(matches!(result, Err(CommandError::Timeout(_))));
    }

    #[tokio::test]
    async fn output_cap_exceeded() {
        let result = run_command(&argv(&["yes"]), Duration::from_secs(5), 100).await;
        assert!(matches!(result, Err(CommandError::OutputTooLarge(_))));
    }

    #[tokio::test]
    async fn empty_argv_returns_error() {
        let result = run_command(&[], Duration::from_secs(5), 64_000).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn stderr_captured_on_failure() {
        let result = run_command(
            &argv(&["sh", "-c", "echo error_msg >&2; exit 1"]),
            Duration::from_secs(5),
            64_000,
        )
        .await;
        match result {
            Err(CommandError::Failed { exit_code, stderr }) => {
                assert_eq!(exit_code, 1);
                assert!(stderr.contains("error_msg"));
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }
}
