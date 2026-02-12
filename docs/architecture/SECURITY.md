# Security Architecture

## Overview

Security in aiOS is fundamentally different from traditional OS security. There's no human to enter passwords. The AI must manage its own security posture — but it also must be constrained so a misbehaving agent can't compromise the whole system.

---

## Threat Model

### External Threats
| Threat | Mitigation |
|---|---|
| Network intrusion | AI-managed firewall, minimal open ports, IDS |
| API key theft | Keys in memory-only keyring, never on disk in plaintext |
| Supply chain attack (malicious packages) | AI reviews package provenance, sandboxed installs |
| DDoS | Rate limiting, traffic analysis, auto-mitigation |
| Man-in-the-middle | mTLS for all external communication |

### Internal Threats
| Threat | Mitigation |
|---|---|
| Rogue agent (compromised by prompt injection) | Capability-based permissions, least privilege |
| Infinite loop / resource exhaustion | Per-agent cgroup limits, watchdog |
| Data exfiltration by agent | Outbound traffic filtering, audit logging |
| Privilege escalation | No agent can grant itself capabilities |
| AI hallucination causing destructive action | High-risk tools require orchestrator confirmation |

### AI-Specific Threats
| Threat | Mitigation |
|---|---|
| Prompt injection via external data | Input sanitization, separate context windows |
| Model poisoning (local models) | Hash verification of model files |
| API response manipulation | Validate API responses against expected schemas |
| Goal misalignment | Audit log + human review capability |

---

## Capability-Based Security

aiOS uses a capability model, not traditional Unix permissions (no rwx, no root).

### How It Works

1. Every agent starts with ZERO capabilities
2. The orchestrator grants capabilities based on agent role
3. Capabilities are specific and granular
4. Capabilities can be time-limited
5. No agent can grant itself or others capabilities
6. Only the orchestrator (via security policy) can grant/revoke

### Capability Format
```
namespace:action[:resource_pattern]
```

### Examples
```
fs:read:*                     # Read any file
fs:read:/etc/*                # Read files in /etc only
fs:write:/etc/nginx/*         # Write to nginx config only
fs:write:/var/log/*           # Write to logs
process:spawn:nginx           # Can only spawn nginx
process:kill:*                # Can kill any process
net:listen:80                 # Can listen on port 80
net:listen:443                # Can listen on port 443
net:connect:*                 # Can connect to any host
pkg:install:*                 # Can install any package
sec:audit:*                   # Can run audits
```

### Agent Default Capabilities

```yaml
# System Agent
aios-agent-system:
  capabilities:
    - fs:read:*
    - fs:write:/etc/*
    - fs:write:/var/*
    - fs:mkdir:*
    - process:spawn:*
    - process:list:*
    - service:*
    - config:*

# Network Agent
aios-agent-network:
  capabilities:
    - net:*
    - firewall:*
    - dns:*
    - fs:read:/etc/network/*
    - fs:write:/etc/network/*

# Security Agent
aios-agent-security:
  capabilities:
    - sec:*
    - audit:*
    - crypto:*
    - fs:read:*              # Can read anything for auditing
    - monitor:*              # Can monitor everything

# Monitor Agent
aios-agent-monitor:
  capabilities:
    - monitor:*
    - metrics:*
    - alert:*
    - fs:read:/var/log/*
    - process:list:*

# Package Agent
aios-agent-package:
  capabilities:
    - pkg:*
    - fs:read:/etc/apt/*
    - fs:write:/etc/apt/*

# Task Agent (gets temporary capabilities per task)
aios-agent-task:
  capabilities: []            # Empty by default
  # Orchestrator grants specific capabilities per task
```

### Temporary Capability Grants
For tasks that need elevated access:
```python
# Orchestrator grants temporary capability
grant = await security.grant_temporary(
    agent="aios-agent-task",
    capability="fs:write:/opt/myapp/*",
    duration="10m",
    reason="Deploying application to /opt/myapp",
    task_id="task-123"
)
# Capability auto-revokes after 10 minutes or task completion
```

---

## Sandboxing

### Agent Sandboxing
Every agent runs in a restricted environment:

```
Per-agent cgroup:
  - CPU: max 200% (2 cores)
  - Memory: max 2GB
  - I/O: max 100MB/s
  - PIDs: max 100

Per-agent namespace:
  - PID namespace (can only see own processes + tools)
  - Mount namespace (limited filesystem view)
  - Network namespace (only orchestrator socket visible)
  - IPC namespace (isolated)

AppArmor profile:
  - Restricts file access to capability-allowed paths
  - Blocks raw socket creation
  - Blocks kernel module loading
  - Blocks ptrace
```

### Task Sandboxing
Untrusted workloads (user-submitted code, downloaded scripts) run in Podman containers:

```
Podman container:
  - Rootless (no host root access)
  - Read-only root filesystem
  - No network by default (granted if needed)
  - Resource limited (CPU, RAM, disk)
  - Temp filesystem destroyed after task completion
  - Monitored by Security Agent
```

---

## Secret Management

### API Keys
- Stored in Linux kernel keyring (in-memory, never on disk)
- Loaded from encrypted file at boot, then file is deleted
- Rotated automatically on schedule
- Per-agent access (agent can only read keys it's authorized for)

### Internal Certificates
- Self-signed CA generated at first boot
- All internal gRPC uses mTLS with per-service certificates
- Certificates auto-rotated every 30 days
- Security Agent manages the CA

### Implementation
```rust
// Boot sequence: load secrets
fn load_secrets() -> Result<()> {
    // Read encrypted secrets file
    let encrypted = fs::read("/etc/aios/secrets.enc")?;
    let key = derive_key_from_tpm_or_passphrase()?;
    let secrets = decrypt(encrypted, key)?;

    // Store in kernel keyring
    for (name, value) in secrets {
        keyring::add("aios", &name, &value)?;
    }

    // Delete encrypted file from disk (it's in memory now)
    fs::remove_file("/etc/aios/secrets.enc")?;

    Ok(())
}
```

---

## Audit System

Every security-relevant action is logged to an append-only audit ledger.

### What Gets Audited
- All tool calls (who, what, when, why)
- All capability grants and revocations
- All agent spawns and shutdowns
- All external API calls
- All network connections (inbound and outbound)
- All file modifications
- All authentication events
- All security policy changes

### Audit Entry Format
```json
{
  "id": "audit-20240101-123456-abcdef",
  "timestamp": "2024-01-01T12:34:56.789Z",
  "event_type": "tool_call",
  "agent": "aios-agent-system",
  "action": "fs.write",
  "target": "/etc/nginx/nginx.conf",
  "capabilities_used": ["fs:write:/etc/nginx/*"],
  "result": "success",
  "risk_level": "medium",
  "details": {
    "bytes_written": 1234,
    "backup_created": true
  }
}
```

### Audit Storage
- Append-only SQLite database
- Hash-chained entries (tamper-evident — each entry includes hash of previous)
- Rotated daily, archived monthly
- Security Agent can query and analyze audit logs

---

## Intrusion Detection

The Security Agent runs continuous intrusion detection:

### Network IDS
- Monitor all inbound connections
- Flag unexpected ports/protocols
- Detect port scanning
- Detect brute force attempts
- Block offending IPs automatically

### Host IDS
- Monitor filesystem for unauthorized changes
- Watch for unexpected processes
- Detect privilege escalation attempts
- Monitor kernel module loading
- Check file integrity (hash comparison)

### AI-Powered Analysis
```python
# Security Agent's continuous loop
async def intrusion_detection_loop():
    while True:
        # Collect signals
        network_events = await monitor.get_network_events(last="5m")
        file_changes = await monitor.get_file_changes(last="5m")
        process_events = await monitor.get_process_events(last="5m")

        # Local model: fast classification
        risk_score = await local_model.classify_risk(
            network_events, file_changes, process_events
        )

        if risk_score > 0.7:
            # Escalate to Claude for analysis
            analysis = await claude.analyze_security_event(
                events={"network": network_events, "files": file_changes, "processes": process_events},
                context=await memory.get_security_context()
            )

            if analysis.threat_confirmed:
                await self.respond_to_threat(analysis)

        await asyncio.sleep(30)  # Check every 30 seconds
```

---

## Network Security

### Default Stance: Deny All Inbound
```
Inbound rules (default):
  - ALLOW: TCP 9090 from management_subnet (management console)
  - ALLOW: TCP 22 from management_subnet (SSH emergency access)
  - DENY: everything else

Outbound rules (default):
  - ALLOW: TCP 443 to api.anthropic.com (Claude API)
  - ALLOW: TCP 443 to api.openai.com (OpenAI API)
  - ALLOW: TCP 443 to *.debian.org (package repos)
  - ALLOW: TCP 53 to configured DNS servers
  - ALLOW: ICMP (ping)
  - DENY: everything else
```

The Network Agent can dynamically add rules as needed (e.g., opening port 80 for nginx), but every rule change is audited.

---

## Emergency Access

Humans need a way in if the AI becomes unresponsive:

### SSH Emergency Console
- Always available on port 22 (management subnet only)
- Key-based authentication (no passwords)
- Grants full root access
- Every command logged to audit system
- AI detects human SSH session and can report on current state

### Serial Console
- Physical serial port or IPMI/BMC console
- Bypasses all network — works even if networking is down
- Boot to recovery mode: `init=/bin/sh` kernel parameter
- Last resort — disables all AI systems

### Management Console Override
- Web UI includes "emergency stop" button
- Halts all agent activity
- Puts system in manual mode
- Requires re-authorization to resume autonomy
