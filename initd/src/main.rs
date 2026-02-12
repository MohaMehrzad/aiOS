//! aiOS Init Daemon — PID 1
//!
//! Responsibilities:
//! - Mount essential filesystems (/proc, /sys, /dev, /tmp, /run)
//! - Read system configuration from /etc/aios/config.toml
//! - Detect hardware (CPU, RAM, GPU, storage, network)
//! - Start and supervise all aiOS services
//! - Reap zombie processes
//! - Handle shutdown signals

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

mod config;
mod hardware;
mod service;

fn main() {
    if let Err(e) = run() {
        eprintln!("FATAL: aios-init failed: {e:#}");
        // PID 1 must not exit — spawn emergency shell
        spawn_emergency_shell();
    }
}

fn run() -> Result<()> {
    // Initialize tracing early
    init_logging()?;

    info!("========================================");
    info!("  aiOS Init v{}", env!("CARGO_PKG_VERSION"));
    info!("========================================");

    // Phase 1: Mount filesystems
    info!("Phase 1: Mounting filesystems...");
    mount_filesystems()?;
    info!("Filesystems mounted");

    // Phase 2: Read configuration
    info!("Phase 2: Loading configuration...");
    let config = config::load_config()?;
    info!("Configuration loaded: hostname={}", config.system.hostname);

    // Set hostname
    set_hostname(&config.system.hostname)?;

    // Phase 3: Hardware detection
    info!("Phase 3: Detecting hardware...");
    let hw = hardware::detect()?;
    info!(
        "Hardware: {} CPUs, {} MB RAM, GPU: {}",
        hw.cpu_count, hw.ram_mb, hw.gpu_detected
    );

    // Phase 3.5: First-boot initialization
    if Path::new("/var/lib/aios/.first-boot").exists() {
        info!("First boot detected — running initialization...");
        run_first_boot()?;
        info!("First boot initialization complete");
    }

    // Phase 4: Start services
    info!("Phase 4: Starting services...");
    let mut supervisor = service::ServiceSupervisor::new(&config);

    // Start aios-runtime (Phase 4+)
    if Path::new("/usr/sbin/aios-runtime").exists() {
        info!("Starting aios-runtime...");
        supervisor.start_service("aios-runtime", "/usr/sbin/aios-runtime", &[])?;
        supervisor.wait_for_health("aios-runtime", Duration::from_secs(30))?;
        info!("aios-runtime online");
    }

    // Start aios-memory (Phase 7+)
    if Path::new("/usr/sbin/aios-memory").exists() {
        info!("Starting aios-memory...");
        supervisor.start_service("aios-memory", "/usr/sbin/aios-memory", &[])?;
        supervisor.wait_for_health("aios-memory", Duration::from_secs(10))?;
        info!("aios-memory online");
    }

    // Start aios-tools (Phase 6+)
    if Path::new("/usr/sbin/aios-tools").exists() {
        info!("Starting aios-tools...");
        supervisor.start_service("aios-tools", "/usr/sbin/aios-tools", &[])?;
        supervisor.wait_for_health("aios-tools", Duration::from_secs(10))?;
        info!("aios-tools online");
    }

    // Start aios-api-gateway (Phase 11+)
    if Path::new("/usr/sbin/aios-api-gateway").exists() {
        info!("Starting aios-api-gateway...");
        supervisor.start_service("aios-api-gateway", "/usr/sbin/aios-api-gateway", &[])?;
        supervisor.wait_for_health("aios-api-gateway", Duration::from_secs(10))?;
        info!("aios-api-gateway online");
    }

    // Start aios-orchestrator (Phase 5+ — starts last, depends on all other services)
    if Path::new("/usr/sbin/aios-orchestrator").exists() {
        info!("Starting aios-orchestrator...");
        supervisor.start_service("aios-orchestrator", "/usr/sbin/aios-orchestrator", &[])?;
        supervisor.wait_for_health("aios-orchestrator", Duration::from_secs(10))?;
        info!("aios-orchestrator online");
    }

    info!("========================================");
    info!("  aiOS Boot Complete");
    info!("  {} services running", supervisor.running_count());
    info!("========================================");

    // Spawn debug shell if configured
    if config.boot.debug_shell {
        info!("Debug shell enabled, spawning /bin/sh on console...");
        spawn_debug_shell();
    }

    // Enter supervisor loop — reap zombies, monitor services
    let shutdown = Arc::new(AtomicBool::new(false));
    setup_signal_handlers(shutdown.clone())?;

    info!("Entering supervisor loop...");
    supervisor_loop(&mut supervisor, &shutdown)?;

    info!("aiOS shutting down...");
    supervisor.stop_all();
    create_clean_shutdown_flag(&config)?;
    info!("Clean shutdown complete");

    Ok(())
}

/// Mount essential virtual filesystems
fn mount_filesystems() -> Result<()> {
    let mounts = [
        ("proc", "/proc", "proc", ""),
        ("sysfs", "/sys", "sysfs", ""),
        ("devtmpfs", "/dev", "devtmpfs", ""),
        ("tmpfs", "/tmp", "tmpfs", "size=256M"),
        ("tmpfs", "/run", "tmpfs", "size=128M,mode=0755"),
    ];

    for (source, target, fstype, options) in &mounts {
        let target_path = Path::new(target);
        if !target_path.exists() {
            fs::create_dir_all(target_path)
                .with_context(|| format!("Failed to create mount point {target}"))?;
        }

        // Skip if already mounted
        if is_mounted(target) {
            continue;
        }

        mount(source, target, fstype, options)
            .with_context(|| format!("Failed to mount {fstype} on {target}"))?;
    }

    // Create /dev/pts if needed
    let devpts = Path::new("/dev/pts");
    if !devpts.exists() {
        fs::create_dir_all(devpts)?;
        mount("devpts", "/dev/pts", "devpts", "gid=5,mode=620")?;
    }

    // Create /dev/shm if needed
    let devshm = Path::new("/dev/shm");
    if !devshm.exists() {
        fs::create_dir_all(devshm)?;
        mount("tmpfs", "/dev/shm", "tmpfs", "size=64M")?;
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn mount(source: &str, target: &str, fstype: &str, options: &str) -> Result<()> {
    use nix::mount::{mount as nix_mount, MsFlags};
    let flags = MsFlags::empty();
    let opts: Option<&str> = if options.is_empty() {
        None
    } else {
        Some(options)
    };
    nix_mount(Some(source), target, Some(fstype), flags, opts)
        .with_context(|| format!("nix::mount failed for {target}"))?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn mount(source: &str, target: &str, fstype: &str, _options: &str) -> Result<()> {
    info!("mount({source}, {target}, {fstype}) — skipped on non-Linux");
    Ok(())
}

fn is_mounted(target: &str) -> bool {
    fs::read_to_string("/proc/mounts")
        .map(|mounts| {
            mounts
                .lines()
                .any(|line| line.split_whitespace().nth(1) == Some(target))
        })
        .unwrap_or(false)
}

fn set_hostname(hostname: &str) -> Result<()> {
    nix::unistd::sethostname(hostname)
        .with_context(|| format!("Failed to set hostname to {hostname}"))?;
    Ok(())
}

fn init_logging() -> Result<()> {
    // Try to create log directory
    let _ = fs::create_dir_all("/var/log/aios");

    // Set up tracing subscriber
    let subscriber = tracing_subscriber::fmt()
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .with_level(true)
        .compact()
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set tracing subscriber")?;
    Ok(())
}

fn setup_signal_handlers(shutdown: Arc<AtomicBool>) -> Result<()> {
    // SIGCHLD — reap zombie processes (PID 1 duty)
    std::thread::spawn(move || loop {
        // Reap any zombie children
        loop {
            match nix::sys::wait::waitpid(
                nix::unistd::Pid::from_raw(-1),
                Some(nix::sys::wait::WaitPidFlag::WNOHANG),
            ) {
                Ok(nix::sys::wait::WaitStatus::StillAlive) => break,
                Ok(status) => {
                    info!("Reaped child process: {:?}", status);
                }
                Err(nix::errno::Errno::ECHILD) => break, // No children
                Err(e) => {
                    warn!("waitpid error: {}", e);
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    });

    // Register SIGTERM/SIGINT for shutdown
    let shutdown_clone = shutdown.clone();
    ctrlc_handler(shutdown_clone);

    Ok(())
}

fn ctrlc_handler(shutdown: Arc<AtomicBool>) {
    // Simple signal handler — set shutdown flag
    unsafe {
        nix::sys::signal::signal(
            nix::sys::signal::Signal::SIGTERM,
            nix::sys::signal::SigHandler::Handler(handle_sigterm),
        )
        .ok();
        nix::sys::signal::signal(
            nix::sys::signal::Signal::SIGINT,
            nix::sys::signal::SigHandler::Handler(handle_sigterm),
        )
        .ok();
    }
    // Store the shutdown flag globally
    SHUTDOWN_FLAG
        .lock()
        .map(|mut guard| {
            *guard = Some(shutdown);
        })
        .ok();
}

static SHUTDOWN_FLAG: std::sync::Mutex<Option<Arc<AtomicBool>>> = std::sync::Mutex::new(None);

extern "C" fn handle_sigterm(_sig: nix::libc::c_int) {
    if let Ok(guard) = SHUTDOWN_FLAG.lock() {
        if let Some(flag) = guard.as_ref() {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

fn supervisor_loop(
    supervisor: &mut service::ServiceSupervisor,
    shutdown: &Arc<AtomicBool>,
) -> Result<()> {
    while !shutdown.load(Ordering::SeqCst) {
        // Check service health
        supervisor.check_and_restart_services();

        // Sleep for health check interval
        std::thread::sleep(Duration::from_secs(10));
    }
    Ok(())
}

fn create_clean_shutdown_flag(config: &config::AiosConfig) -> Result<()> {
    let flag_path = &config.boot.clean_shutdown_flag;
    if let Some(parent) = Path::new(flag_path).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(flag_path, "clean")?;
    Ok(())
}

fn spawn_debug_shell() {
    match Command::new("/bin/sh").spawn() {
        Ok(_child) => info!("Debug shell spawned"),
        Err(e) => warn!("Failed to spawn debug shell: {}", e),
    }
}

fn run_first_boot() -> Result<()> {
    let first_boot_script = "/usr/lib/aios/first-boot.sh";
    if Path::new(first_boot_script).exists() {
        info!("Running first-boot script: {}", first_boot_script);
        let status = Command::new("/bin/sh")
            .arg(first_boot_script)
            .status()
            .with_context(|| format!("Failed to execute {first_boot_script}"))?;
        if !status.success() {
            anyhow::bail!(
                "First-boot script exited with status: {}",
                status.code().unwrap_or(-1)
            );
        }
    } else {
        // Perform minimal first-boot inline
        info!("No first-boot script found, performing inline initialization...");
        let dirs = [
            "/var/lib/aios/memory",
            "/var/lib/aios/models",
            "/var/lib/aios/tasks",
            "/var/lib/aios/scratch",
            "/var/lib/aios/cache",
            "/var/lib/aios/vectors",
            "/var/lib/aios/ledger",
            "/var/lib/aios/runtime",
            "/var/log/aios",
            "/etc/aios/keys",
        ];
        for dir in &dirs {
            fs::create_dir_all(dir)
                .with_context(|| format!("Failed to create directory {dir}"))?;
        }
        info!("Created aiOS directory structure");
    }

    // Remove first-boot flag
    let _ = fs::remove_file("/var/lib/aios/.first-boot");

    // Create initialized marker
    let _ = fs::create_dir_all("/var/lib/aios");
    fs::write(
        "/var/lib/aios/initialized",
        format!(
            "initialized_at={}\nversion={}",
            chrono_timestamp(),
            env!("CARGO_PKG_VERSION")
        ),
    )
    .ok();

    info!("First boot complete. System autonomous.");
    Ok(())
}

fn chrono_timestamp() -> String {
    // Simple timestamp without external dependency
    use std::time::SystemTime;
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_secs().to_string(),
        Err(_) => "0".to_string(),
    }
}

fn spawn_emergency_shell() {
    eprintln!("Attempting to spawn emergency shell...");
    let _ = Command::new("/bin/sh").status();
}
