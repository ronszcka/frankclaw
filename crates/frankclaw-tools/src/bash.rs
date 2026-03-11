//! Bash tool: sandboxed shell command execution for AI agents.
//!
//! Executes shell commands as subprocesses with timeout enforcement,
//! output truncation, working directory control, and an optional command
//! allowlist for security-sensitive deployments.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::warn;

use frankclaw_core::error::{FrankClawError, Result};
use frankclaw_core::model::ToolDef;

use crate::{Tool, ToolContext};

/// Maximum output size in characters.
const MAX_OUTPUT_CHARS: usize = 200_000;

/// Default command timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Maximum command timeout in seconds.
const MAX_TIMEOUT_SECS: u64 = 600;

/// Bash tool input parameters.
#[derive(Debug, Deserialize)]
struct BashArgs {
    /// Shell command to execute.
    command: String,
    /// Working directory (optional, defaults to cwd).
    #[serde(default)]
    workdir: Option<String>,
    /// Timeout in seconds (optional, defaults to 120).
    #[serde(default)]
    timeout: Option<u64>,
}

/// Result of a bash command execution.
#[derive(Debug, Serialize)]
struct BashResult {
    /// Exit code (None if killed/timed out).
    exit_code: Option<i32>,
    /// Combined stdout output.
    stdout: String,
    /// Combined stderr output.
    stderr: String,
    /// Whether output was truncated.
    truncated: bool,
    /// Duration in milliseconds.
    duration_ms: u64,
}

/// Security policy for bash tool execution.
#[derive(Debug, Clone)]
pub enum BashPolicy {
    /// Allow all commands (dangerous, for development only).
    AllowAll,
    /// Only allow commands starting with these binaries.
    Allowlist(Vec<String>),
    /// Deny all commands.
    DenyAll,
}

impl Default for BashPolicy {
    fn default() -> Self {
        Self::DenyAll
    }
}

impl BashPolicy {
    /// Check if a command is allowed by the policy.
    fn allows(&self, command: &str) -> bool {
        match self {
            Self::AllowAll => true,
            Self::DenyAll => false,
            Self::Allowlist(allowed) => {
                let first_word = command.split_whitespace().next().unwrap_or("");
                // Strip path prefixes for matching.
                let binary = first_word.rsplit('/').next().unwrap_or(first_word);
                allowed.iter().any(|a| a == binary)
            }
        }
    }

    /// Create a policy from environment variable `FRANKCLAW_BASH_POLICY`.
    ///
    /// Values:
    /// - "allow-all" → AllowAll
    /// - "deny-all" or unset → DenyAll
    /// - Comma-separated list → Allowlist
    pub fn from_env() -> Self {
        match std::env::var("FRANKCLAW_BASH_POLICY").ok().as_deref() {
            Some("allow-all") => Self::AllowAll,
            Some("deny-all") | None => Self::DenyAll,
            Some(list) => {
                let allowed: Vec<String> = list
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if allowed.is_empty() {
                    Self::DenyAll
                } else {
                    Self::Allowlist(allowed)
                }
            }
        }
    }
}

/// Bash tool for executing shell commands.
pub struct BashTool {
    policy: BashPolicy,
}

impl BashTool {
    pub fn new(policy: BashPolicy) -> Self {
        Self { policy }
    }

    pub fn from_env() -> Self {
        Self {
            policy: BashPolicy::from_env(),
        }
    }
}

#[async_trait]
impl Tool for BashTool {
    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "bash".into(),
            description: "Execute a shell command and return its output.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "workdir": {
                        "type": "string",
                        "description": "Working directory (optional)"
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in seconds (default 120, max 600)"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn invoke(&self, args: serde_json::Value, _ctx: ToolContext) -> Result<serde_json::Value> {
        let args: BashArgs = serde_json::from_value(args).map_err(|e| {
            FrankClawError::InvalidRequest {
                msg: format!("invalid bash args: {e}"),
            }
        })?;

        // Security check.
        if !self.policy.allows(&args.command) {
            return Err(FrankClawError::AgentRuntime {
                msg: format!(
                    "bash command not allowed by policy: '{}'",
                    args.command.chars().take(100).collect::<String>()
                ),
            });
        }

        // Validate and sanitize working directory.
        let workdir = if let Some(ref dir) = args.workdir {
            let path = PathBuf::from(dir);
            if !path.is_dir() {
                return Err(FrankClawError::InvalidRequest {
                    msg: format!("working directory does not exist: {dir}"),
                });
            }
            Some(path)
        } else {
            None
        };

        let timeout_secs = args
            .timeout
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(MAX_TIMEOUT_SECS);
        let timeout = Duration::from_secs(timeout_secs);

        let result = execute_command(&args.command, workdir.as_ref(), timeout).await?;

        serde_json::to_value(&result).map_err(|e| FrankClawError::Internal {
            msg: format!("failed to serialize bash result: {e}"),
        })
    }
}

/// Execute a shell command with timeout and output capture.
async fn execute_command(
    command: &str,
    workdir: Option<&PathBuf>,
    timeout: Duration,
) -> Result<BashResult> {
    let start = std::time::Instant::now();

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::null());

    // Prevent inheriting dangerous environment variables.
    cmd.env_remove("HISTFILE");
    cmd.env_remove("BASH_HISTORY");

    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }

    let child = cmd.spawn().map_err(|e| FrankClawError::AgentRuntime {
        msg: format!("failed to spawn shell: {e}"),
    })?;

    // Wait with timeout.
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(FrankClawError::AgentRuntime {
                msg: format!("command execution error: {e}"),
            });
        }
        Err(_) => {
            warn!(command = %command.chars().take(80).collect::<String>(), "bash command timed out");
            return Ok(BashResult {
                exit_code: None,
                stdout: String::new(),
                stderr: format!("command timed out after {}s", timeout.as_secs()),
                truncated: false,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }
    };

    let duration_ms = start.elapsed().as_millis() as u64;

    let raw_stdout = String::from_utf8_lossy(&output.stdout);
    let raw_stderr = String::from_utf8_lossy(&output.stderr);

    let (stdout, stdout_truncated) = truncate_output(&raw_stdout);
    let (stderr, stderr_truncated) = truncate_output(&raw_stderr);

    Ok(BashResult {
        exit_code: output.status.code(),
        stdout,
        stderr,
        truncated: stdout_truncated || stderr_truncated,
        duration_ms,
    })
}

/// Truncate output to MAX_OUTPUT_CHARS, keeping the tail.
fn truncate_output(output: &str) -> (String, bool) {
    if output.len() <= MAX_OUTPUT_CHARS {
        return (output.to_string(), false);
    }

    // Keep the last MAX_OUTPUT_CHARS characters with a truncation marker.
    let skip = output.len() - MAX_OUTPUT_CHARS + 50; // 50 chars for the marker
    let truncated = format!(
        "[...truncated {} chars...]\n{}",
        skip,
        &output[skip..]
    );
    (truncated, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_deny_all_blocks_everything() {
        let policy = BashPolicy::DenyAll;
        assert!(!policy.allows("ls"));
        assert!(!policy.allows("rm -rf /"));
    }

    #[test]
    fn policy_allow_all_permits_everything() {
        let policy = BashPolicy::AllowAll;
        assert!(policy.allows("ls"));
        assert!(policy.allows("rm -rf /"));
    }

    #[test]
    fn policy_allowlist_filters_by_binary() {
        let policy = BashPolicy::Allowlist(vec!["ls".into(), "cat".into(), "grep".into()]);
        assert!(policy.allows("ls -la"));
        assert!(policy.allows("cat /etc/passwd"));
        assert!(policy.allows("/usr/bin/cat file.txt"));
        assert!(!policy.allows("rm -rf /"));
        assert!(!policy.allows("curl https://evil.com"));
    }

    #[test]
    fn policy_allowlist_empty_command() {
        let policy = BashPolicy::Allowlist(vec!["ls".into()]);
        assert!(!policy.allows(""));
    }

    #[test]
    fn truncate_output_short() {
        let (output, truncated) = truncate_output("hello world");
        assert_eq!(output, "hello world");
        assert!(!truncated);
    }

    #[test]
    fn truncate_output_long() {
        let long = "x".repeat(MAX_OUTPUT_CHARS + 1000);
        let (output, truncated) = truncate_output(&long);
        assert!(truncated);
        assert!(output.len() <= MAX_OUTPUT_CHARS + 100); // ~MAX with marker
        assert!(output.starts_with("[...truncated"));
    }

    #[tokio::test]
    async fn execute_echo() {
        let result = execute_command("echo hello", None, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout.trim(), "hello");
        assert!(result.stderr.is_empty());
        assert!(!result.truncated);
    }

    #[tokio::test]
    async fn execute_with_exit_code() {
        let result = execute_command("exit 42", None, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(result.exit_code, Some(42));
    }

    #[tokio::test]
    async fn execute_with_stderr() {
        let result = execute_command("echo error >&2", None, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stderr.trim(), "error");
    }

    #[tokio::test]
    async fn execute_with_timeout() {
        let result = execute_command("sleep 60", None, Duration::from_secs(1))
            .await
            .unwrap();
        assert!(result.exit_code.is_none());
        assert!(result.stderr.contains("timed out"));
    }

    #[tokio::test]
    async fn execute_with_workdir() {
        let result = execute_command("pwd", Some(&PathBuf::from("/tmp")), Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(result.exit_code, Some(0));
        // On some systems /tmp may be a symlink.
        let output = result.stdout.trim();
        assert!(output == "/tmp" || output.ends_with("/tmp"));
    }

    #[tokio::test]
    async fn bash_tool_respects_deny_policy() {
        let tool = BashTool::new(BashPolicy::DenyAll);
        let args = serde_json::json!({ "command": "echo hello" });
        let ctx = test_context();
        let err = tool.invoke(args, ctx).await.unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[tokio::test]
    async fn bash_tool_executes_with_allow_all() {
        let tool = BashTool::new(BashPolicy::AllowAll);
        let args = serde_json::json!({ "command": "echo test123" });
        let ctx = test_context();
        let result = tool.invoke(args, ctx).await.unwrap();
        assert_eq!(result["exit_code"], 0);
        assert!(result["stdout"].as_str().unwrap().contains("test123"));
    }

    fn test_context() -> ToolContext {
        use frankclaw_core::types::AgentId;
        use frankclaw_core::session::SessionStore;

        struct NoopStore;
        #[async_trait]
        impl SessionStore for NoopStore {
            async fn get(&self, _key: &frankclaw_core::types::SessionKey) -> Result<Option<frankclaw_core::session::SessionEntry>> { Ok(None) }
            async fn upsert(&self, _entry: &frankclaw_core::session::SessionEntry) -> Result<()> { Ok(()) }
            async fn delete(&self, _key: &frankclaw_core::types::SessionKey) -> Result<()> { Ok(()) }
            async fn list(&self, _agent_id: &frankclaw_core::types::AgentId, _limit: usize, _offset: usize) -> Result<Vec<frankclaw_core::session::SessionEntry>> { Ok(vec![]) }
            async fn append_transcript(&self, _key: &frankclaw_core::types::SessionKey, _entry: &frankclaw_core::session::TranscriptEntry) -> Result<()> { Ok(()) }
            async fn get_transcript(&self, _key: &frankclaw_core::types::SessionKey, _limit: usize, _before: Option<u64>) -> Result<Vec<frankclaw_core::session::TranscriptEntry>> { Ok(vec![]) }
            async fn clear_transcript(&self, _key: &frankclaw_core::types::SessionKey) -> Result<()> { Ok(()) }
            async fn maintenance(&self, _config: &frankclaw_core::session::PruningConfig) -> Result<u64> { Ok(0) }
        }

        ToolContext {
            agent_id: AgentId::new("test"),
            session_key: None,
            sessions: std::sync::Arc::new(NoopStore),
        }
    }
}
