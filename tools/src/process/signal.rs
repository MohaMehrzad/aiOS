//! process.signal — Send a named signal to a process

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Input {
    pid: u32,
    signal: String,
}

#[derive(Serialize)]
struct Output {
    sent: bool,
}

/// Convert a signal name string to a nix Signal
fn parse_signal_name(name: &str) -> Result<nix::sys::signal::Signal> {
    use nix::sys::signal::Signal;

    // Normalise: strip optional "SIG" prefix, uppercase
    let normalised = name.trim().to_uppercase();
    let normalised = normalised.strip_prefix("SIG").unwrap_or(&normalised);

    match normalised {
        "HUP" => Ok(Signal::SIGHUP),
        "INT" => Ok(Signal::SIGINT),
        "QUIT" => Ok(Signal::SIGQUIT),
        "ILL" => Ok(Signal::SIGILL),
        "TRAP" => Ok(Signal::SIGTRAP),
        "ABRT" | "IOT" => Ok(Signal::SIGABRT),
        "BUS" => Ok(Signal::SIGBUS),
        "FPE" => Ok(Signal::SIGFPE),
        "KILL" => Ok(Signal::SIGKILL),
        "USR1" => Ok(Signal::SIGUSR1),
        "SEGV" => Ok(Signal::SIGSEGV),
        "USR2" => Ok(Signal::SIGUSR2),
        "PIPE" => Ok(Signal::SIGPIPE),
        "ALRM" => Ok(Signal::SIGALRM),
        "TERM" => Ok(Signal::SIGTERM),
        "CHLD" => Ok(Signal::SIGCHLD),
        "CONT" => Ok(Signal::SIGCONT),
        "STOP" => Ok(Signal::SIGSTOP),
        "TSTP" => Ok(Signal::SIGTSTP),
        "TTIN" => Ok(Signal::SIGTTIN),
        "TTOU" => Ok(Signal::SIGTTOU),
        "URG" => Ok(Signal::SIGURG),
        "XCPU" => Ok(Signal::SIGXCPU),
        "XFSZ" => Ok(Signal::SIGXFSZ),
        "VTALRM" => Ok(Signal::SIGVTALRM),
        "PROF" => Ok(Signal::SIGPROF),
        "WINCH" => Ok(Signal::SIGWINCH),
        "IO" => Ok(Signal::SIGIO),
        "SYS" => Ok(Signal::SIGSYS),
        _ => {
            // Try parsing as a number
            if let Ok(num) = normalised.parse::<i32>() {
                nix::sys::signal::Signal::try_from(num)
                    .with_context(|| format!("Invalid signal number: {num}"))
            } else {
                Err(anyhow::anyhow!("Unknown signal name: {}", name))
            }
        }
    }
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let signal = parse_signal_name(&input.signal)?;
    let pid = nix::unistd::Pid::from_raw(input.pid as i32);

    let sent = match nix::sys::signal::kill(pid, signal) {
        Ok(()) => true,
        Err(nix::errno::Errno::ESRCH) => false, // No such process
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to send signal {} to pid {}: {}",
                input.signal,
                input.pid,
                e
            ))
        }
    };

    let result = Output { sent };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
