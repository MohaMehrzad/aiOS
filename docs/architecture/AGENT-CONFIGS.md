# Agent Configuration Files

## Overview

Each agent has a TOML configuration file in `/etc/aios/agents/`. These files define the agent's identity, resource limits, capabilities, and behavior.

---

## Config Location

```
/etc/aios/agents/
├── system.toml
├── network.toml
├── security.toml
├── monitor.toml
├── package.toml
├── storage.toml
├── task.toml
└── dev.toml
```

---

## Config Schema

```toml
# /etc/aios/agents/system.toml

[agent]
name = "aios-agent-system"
type = "system"
enabled = true
binary = "/usr/lib/aios/agents/system_agent.py"
runtime = "python"                  # "python" or "rust"

[agent.description]
role = "Manages files, processes, and services"
system_prompt_file = "/etc/aios/agents/prompts/system.txt"

# Capabilities granted to this agent
[capabilities]
permanent = [
    "fs:read:*",
    "fs:write:/etc/*",
    "fs:write:/var/*",
    "fs:write:/home/*",
    "fs:mkdir:*",
    "fs:delete:/tmp/*",
    "fs:delete:/var/tmp/*",
    "fs:chmod:*",
    "fs:chown:*",
    "fs:symlink:*",
    "process:spawn:*",
    "process:list:*",
    "process:kill:*",
    "process:signal:*",
    "service:start:*",
    "service:stop:*",
    "service:restart:*",
    "service:status:*",
    "service:enable:*",
    "service:disable:*",
    "config:read:*",
    "config:write:*",
]
# Capabilities that can be temporarily granted by orchestrator
requestable = [
    "fs:write:/boot/*",
    "fs:delete:*",
]

# Resource limits (cgroup)
[resources]
cpu_cores = 2.0                     # Max CPU cores (200% of one core)
memory_mb = 2048                    # Max memory in MB
max_pids = 100                      # Max concurrent processes
io_mbps = 100                       # Max I/O in MB/s

# Behavior
[behavior]
intelligence_default = "tactical"   # Default intelligence level for this agent
escalation_threshold = 0.7          # Confidence below this → escalate to higher model
max_concurrent_tasks = 5
task_timeout_seconds = 300          # 5 minutes per task
heartbeat_interval_seconds = 5

# Restart policy
[restart]
max_restarts = 5
restart_window_seconds = 300        # Reset counter after 5 minutes
restart_delay_seconds = 2           # Wait before restarting
```

---

## Per-Agent Config Examples

### Network Agent
```toml
# /etc/aios/agents/network.toml
[agent]
name = "aios-agent-network"
type = "network"
enabled = true
binary = "/usr/lib/aios/agents/network_agent.py"
runtime = "python"

[capabilities]
permanent = [
    "net:interfaces:*",
    "net:configure:*",
    "net:check_port:*",
    "net:http_request:*",
    "net:dns_lookup:*",
    "net:ping:*",
    "net:listen:*",
    "net:connections:*",
    "net:bandwidth:*",
    "net:route_add:*",
    "net:route_del:*",
    "net:route_list:*",
    "firewall:rules:*",
    "firewall:allow:*",
    "firewall:deny:*",
    "firewall:remove:*",
    "dns:configure:*",
    "dns:add_record:*",
    "dns:blocklist:*",
    "fs:read:/etc/network/*",
    "fs:write:/etc/network/*",
    "fs:read:/etc/unbound/*",
    "fs:write:/etc/unbound/*",
]

[resources]
cpu_cores = 1.0
memory_mb = 1024
max_pids = 50
io_mbps = 50
```

### Security Agent
```toml
# /etc/aios/agents/security.toml
[agent]
name = "aios-agent-security"
type = "security"
enabled = true
binary = "/usr/lib/aios/agents/security_agent.py"
runtime = "python"

[capabilities]
permanent = [
    "sec:audit:*",
    "sec:scan:*",
    "sec:grant:*",
    "sec:revoke:*",
    "sec:cert_generate:*",
    "sec:cert_rotate:*",
    "crypto:*",
    "fs:read:*",
    "monitor:*",
    "process:list:*",
    "net:connections:*",
]

[behavior]
intelligence_default = "tactical"
escalation_threshold = 0.5         # Lower threshold — security needs more careful decisions
max_concurrent_tasks = 3
task_timeout_seconds = 600

[resources]
cpu_cores = 2.0
memory_mb = 2048
max_pids = 50
io_mbps = 100
```

### Monitor Agent
```toml
# /etc/aios/agents/monitor.toml
[agent]
name = "aios-agent-monitor"
type = "monitor"
enabled = true
binary = "/usr/lib/aios/agents/monitor_agent.py"
runtime = "python"

[capabilities]
permanent = [
    "monitor:cpu:*",
    "monitor:memory:*",
    "monitor:disk:*",
    "monitor:gpu:*",
    "monitor:network:*",
    "monitor:health:*",
    "monitor:watch:*",
    "metrics:*",
    "alert:*",
    "fs:read:/var/log/*",
    "fs:read:/proc/*",
    "fs:read:/sys/*",
    "process:list:*",
]

[behavior]
intelligence_default = "operational"  # Monitoring is mostly simple analysis
escalation_threshold = 0.8
max_concurrent_tasks = 10
task_timeout_seconds = 60

[resources]
cpu_cores = 1.0
memory_mb = 1024
max_pids = 30
io_mbps = 50
```

### Task Agent (Dynamic Capabilities)
```toml
# /etc/aios/agents/task.toml
[agent]
name = "aios-agent-task"
type = "task"
enabled = true
binary = "/usr/lib/aios/agents/task_agent.py"
runtime = "python"

# Task agent starts with NO permanent capabilities
# Orchestrator grants temporary capabilities per task
[capabilities]
permanent = []
requestable = ["*"]                  # Can request anything temporarily

[behavior]
intelligence_default = "tactical"
max_concurrent_tasks = 3
task_timeout_seconds = 600

[resources]
cpu_cores = 2.0
memory_mb = 4096                     # More memory for diverse tasks
max_pids = 200
io_mbps = 100
```

---

## System Prompt Files

Each agent has a system prompt file referenced by `system_prompt_file`:

```
/etc/aios/agents/prompts/
├── system.txt
├── network.txt
├── security.txt
├── monitor.txt
├── package.txt
├── storage.txt
├── task.txt
└── dev.txt
```

These are plain text files containing the agent's system prompt. They are loaded by the agent runtime at startup and used for every AI inference call the agent makes.

---

## Loading Agent Configs

The orchestrator loads all agent configs at startup:

```rust
pub fn load_agent_configs(config_dir: &Path) -> Result<Vec<AgentConfig>> {
    let mut configs = Vec::new();
    for entry in fs::read_dir(config_dir)? {
        let path = entry?.path();
        if path.extension().map_or(false, |e| e == "toml") {
            let content = fs::read_to_string(&path)?;
            let config: AgentConfig = toml::from_str(&content)?;
            if config.agent.enabled {
                configs.push(config);
            }
        }
    }
    Ok(configs)
}
```
