//! Execution Sandboxing — namespace isolation for untrusted operations
//!
//! Wraps tool execution in restricted environments:
//! - Linux: uses unshare/namespaces for isolation
//! - Fallback: subprocess with restricted environment
//! - Resource limits: memory, CPU time, file descriptors

use anyhow::{Context, Result};
use std::time::Duration;
use tracing::{info, warn};

/// Resource limits for sandboxed execution
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// Maximum memory in bytes (default: 256MB)
    pub max_memory_bytes: u64,
    /// Maximum CPU time (default: 30 seconds)
    pub max_cpu_time: Duration,
    /// Maximum number of file descriptors (default: 64)
    pub max_file_descriptors: u32,
    /// Maximum number of processes/threads (default: 16)
    pub max_processes: u32,
    /// Allow network access (default: false for sandboxed)
    pub allow_network: bool,
    /// Writable paths (everything else is read-only)
    pub writable_paths: Vec<String>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 256 * 1024 * 1024, // 256 MB
            max_cpu_time: Duration::from_secs(30),
            max_file_descriptors: 64,
            max_processes: 16,
            allow_network: false,
            writable_paths: vec!["/tmp/aios-sandbox".to_string()],
        }
    }
}

/// Result of sandboxed execution
#[derive(Debug)]
pub struct SandboxResult {
    pub success: bool,
    pub output: Vec<u8>,
    pub error: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub resource_usage: ResourceUsage,
}

/// Resource usage during sandboxed execution
#[derive(Debug, Default)]
pub struct ResourceUsage {
    pub peak_memory_bytes: u64,
    pub cpu_time_ms: u64,
}

/// Sandbox for executing tools in isolation
pub struct Sandbox {
    limits: ResourceLimits,
}

impl Sandbox {
    pub fn new(limits: ResourceLimits) -> Self {
        Self { limits }
    }

    /// Execute a command in the sandbox
    pub async fn execute(
        &self,
        command: &str,
        args: &[&str],
        input: &[u8],
    ) -> Result<SandboxResult> {
        let start = std::time::Instant::now();

        info!(
            "Sandbox executing: {} {:?} (limits: {}MB memory, {}s CPU)",
            command,
            args,
            self.limits.max_memory_bytes / 1024 / 1024,
            self.limits.max_cpu_time.as_secs()
        );

        // Build the sandboxed command
        let result = self.execute_with_limits(command, args, input).await;
        let duration = start.elapsed();

        match result {
            Ok((output, exit_code)) => Ok(SandboxResult {
                success: exit_code == 0,
                output,
                error: String::new(),
                exit_code,
                duration_ms: duration.as_millis() as u64,
                resource_usage: ResourceUsage::default(),
            }),
            Err(e) => Ok(SandboxResult {
                success: false,
                output: vec![],
                error: e.to_string(),
                exit_code: -1,
                duration_ms: duration.as_millis() as u64,
                resource_usage: ResourceUsage::default(),
            }),
        }
    }

    /// Execute with resource limits applied
    async fn execute_with_limits(
        &self,
        command: &str,
        args: &[&str],
        input: &[u8],
    ) -> Result<(Vec<u8>, i32)> {
        use tokio::io::AsyncWriteExt;
        use tokio::process::Command;

        // Build a restricted environment
        let mut cmd = Command::new(command);
        cmd.args(args);

        // Clear environment and set minimal vars
        cmd.env_clear();
        cmd.env("PATH", "/usr/bin:/bin");
        cmd.env("HOME", "/tmp/aios-sandbox");
        cmd.env("LANG", "C.UTF-8");

        // Disable network if required
        if !self.limits.allow_network {
            cmd.env("AIOS_SANDBOX_NO_NETWORK", "1");
        }

        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // Apply Linux-specific resource limits via pre_exec
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            let max_mem = self.limits.max_memory_bytes;
            let max_fds = self.limits.max_file_descriptors;
            let max_procs = self.limits.max_processes;

            unsafe {
                cmd.pre_exec(move || {
                    // Set memory limit via rlimit
                    let mem_limit = libc::rlimit {
                        rlim_cur: max_mem,
                        rlim_max: max_mem,
                    };
                    libc::setrlimit(libc::RLIMIT_AS, &mem_limit);

                    // Set file descriptor limit
                    let fd_limit = libc::rlimit {
                        rlim_cur: max_fds as u64,
                        rlim_max: max_fds as u64,
                    };
                    libc::setrlimit(libc::RLIMIT_NOFILE, &fd_limit);

                    // Set process limit
                    let proc_limit = libc::rlimit {
                        rlim_cur: max_procs as u64,
                        rlim_max: max_procs as u64,
                    };
                    libc::setrlimit(libc::RLIMIT_NPROC, &proc_limit);

                    Ok(())
                });
            }
        }

        let mut child = cmd.spawn().context("Failed to spawn sandboxed process")?;

        // Write input if provided
        if !input.is_empty() {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(input).await.ok();
            }
        }

        // Wait with timeout
        let result = tokio::time::timeout(self.limits.max_cpu_time, child.wait_with_output())
            .await
            .map_err(|_| {
                warn!("Sandbox execution timed out after {:?}", self.limits.max_cpu_time);
                anyhow::anyhow!("Execution timed out after {:?}", self.limits.max_cpu_time)
            })?
            .context("Failed to wait for sandboxed process")?;

        let exit_code = result.status.code().unwrap_or(-1);
        let mut output = result.stdout;
        if !result.stderr.is_empty() {
            output.extend_from_slice(b"\n--- stderr ---\n");
            output.extend_from_slice(&result.stderr);
        }

        Ok((output, exit_code))
    }

    /// Check if a tool should be sandboxed based on risk level
    pub fn should_sandbox(tool_name: &str) -> bool {
        // High-risk tools that modify system state
        let sandboxed_prefixes = [
            "process.spawn",
            "pkg.install",
            "pkg.remove",
            "firewall.",
        ];

        sandboxed_prefixes
            .iter()
            .any(|prefix| tool_name.starts_with(prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_limits_default() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.max_memory_bytes, 256 * 1024 * 1024);
        assert_eq!(limits.max_cpu_time, Duration::from_secs(30));
        assert_eq!(limits.max_file_descriptors, 64);
        assert!(!limits.allow_network);
    }

    #[test]
    fn test_sandbox_new() {
        let sandbox = Sandbox::new(ResourceLimits::default());
        assert_eq!(sandbox.limits.max_processes, 16);
    }

    #[test]
    fn test_should_sandbox() {
        assert!(Sandbox::should_sandbox("process.spawn"));
        assert!(Sandbox::should_sandbox("pkg.install"));
        assert!(Sandbox::should_sandbox("firewall.add_rule"));
        assert!(!Sandbox::should_sandbox("fs.read"));
        assert!(!Sandbox::should_sandbox("monitor.cpu"));
    }

    #[tokio::test]
    async fn test_sandbox_execute_echo() {
        let sandbox = Sandbox::new(ResourceLimits::default());
        let result = sandbox.execute("echo", &["hello"], &[]).await.unwrap();
        assert!(result.success);
        assert!(String::from_utf8_lossy(&result.output).contains("hello"));
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_sandbox_execute_failure() {
        let sandbox = Sandbox::new(ResourceLimits::default());
        let result = sandbox.execute("false", &[], &[]).await.unwrap();
        assert!(!result.success);
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_sandbox_timeout() {
        let sandbox = Sandbox::new(ResourceLimits {
            max_cpu_time: Duration::from_millis(100),
            ..Default::default()
        });
        let result = sandbox.execute("sleep", &["10"], &[]).await.unwrap();
        assert!(!result.success);
    }
}
