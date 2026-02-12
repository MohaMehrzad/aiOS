//! process.kill — Kill a process by PID

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Input {
    pid: u32,
    #[serde(default = "default_signal")]
    signal: i32,
}

fn default_signal() -> i32 {
    9 // SIGKILL
}

#[derive(Serialize)]
struct Output {
    killed: bool,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let pid = nix::unistd::Pid::from_raw(input.pid as i32);
    let signal = nix::sys::signal::Signal::try_from(input.signal)
        .with_context(|| format!("Invalid signal number: {}", input.signal))?;

    let killed = match nix::sys::signal::kill(pid, signal) {
        Ok(()) => true,
        Err(nix::errno::Errno::ESRCH) => false, // No such process
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to kill process {}: {}",
                input.pid,
                e
            ))
        }
    };

    let result = Output { killed };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
