# Phase 9: Security Subsystem

## Goal
Implement capability-based security, agent sandboxing, secret management, audit ledger, and AI-powered intrusion detection.

## Prerequisites
- Phase 6 complete (tool registry)
- Phase 7 complete (memory system for audit storage)
- Read [architecture/SECURITY.md](../architecture/SECURITY.md)

---

## Step-by-Step

### Step 9.1: Implement Capability System

**Claude Code prompt**: "Implement the capability-based security system — capability parsing, agent capability storage, permission checking on every tool call, and temporary grants"

```rust
// security/src/capabilities.rs

pub struct Capability {
    pub namespace: String,   // "fs"
    pub action: String,      // "write"
    pub resource: String,    // "/etc/nginx/*" or "*"
}

impl Capability {
    pub fn parse(s: &str) -> Result<Self> {
        // Parse "fs:write:/etc/nginx/*" format
    }

    pub fn matches(&self, required: &Capability) -> bool {
        // Check if this capability satisfies the requirement
        // Supports wildcards: "fs:write:*" matches "fs:write:/etc/nginx/conf"
        // Supports glob: "fs:write:/etc/*" matches "fs:write:/etc/nginx/conf"
    }
}

pub struct CapabilityManager {
    agent_capabilities: HashMap<String, Vec<Capability>>,
    temporary_grants: Vec<TemporaryGrant>,
}

impl CapabilityManager {
    pub fn check(&self, agent: &str, required: &[Capability]) -> Result<()> {
        // 1. Get agent's permanent capabilities
        // 2. Check temporary grants (remove expired ones)
        // 3. Verify all required capabilities are satisfied
        // 4. Return Ok or Err(InsufficientCapabilities)
    }

    pub fn grant_temporary(&mut self, agent: &str, cap: Capability,
                           duration: Duration, reason: &str) -> GrantId {
        // Create time-limited capability grant
    }

    pub fn revoke_temporary(&mut self, grant_id: &GrantId) {
        // Explicitly revoke a temporary grant
    }
}
```

### Step 9.2: Integrate Capabilities with Tool Registry

**Claude Code prompt**: "Update the tool execution pipeline to check agent capabilities before every tool call, rejecting unauthorized requests"

The executor from Phase 6 already has a `check_perms` step — now implement it for real:
```rust
// In tools/src/executor.rs, step 2:
let agent_id = &request.agent_id;
let required_caps = &tool.required_capabilities;
capability_manager.check(agent_id, required_caps)?;
```

### Step 9.3: Implement Agent Sandboxing

**Claude Code prompt**: "Implement cgroup-based resource limits and namespace isolation for each agent process"

```rust
// security/src/sandbox.rs

pub struct AgentSandbox {
    pub agent_name: String,
    pub cgroup_path: String,
}

impl AgentSandbox {
    pub fn create(agent_name: &str, limits: &ResourceLimits) -> Result<Self> {
        // 1. Create cgroup v2 for agent
        let cgroup_path = format!("/sys/fs/cgroup/aios/{}", agent_name);
        fs::create_dir_all(&cgroup_path)?;

        // 2. Set resource limits
        // CPU: max 200% (2 cores)
        fs::write(format!("{}/cpu.max", cgroup_path),
                   format!("{} 100000", limits.cpu_microseconds))?;

        // Memory: max 2GB
        fs::write(format!("{}/memory.max", cgroup_path),
                   limits.memory_bytes.to_string())?;

        // PIDs: max 100
        fs::write(format!("{}/pids.max", cgroup_path),
                   limits.max_pids.to_string())?;

        // I/O: max 100MB/s
        fs::write(format!("{}/io.max", cgroup_path),
                   format!("default rbps={} wbps={}",
                           limits.io_bytes_per_sec, limits.io_bytes_per_sec))?;

        Ok(Self { agent_name: agent_name.to_string(), cgroup_path })
    }

    pub fn add_process(&self, pid: u32) -> Result<()> {
        fs::write(format!("{}/cgroup.procs", self.cgroup_path), pid.to_string())?;
        Ok(())
    }
}
```

### Step 9.4: Implement AppArmor Profiles

**Claude Code prompt**: "Create AppArmor profiles for each agent type, restricting file access to only what the agent needs"

```
# /etc/apparmor.d/aios-agent-system
profile aios-agent-system {
    # Allow reading most system files
    /etc/** r,
    /var/** rw,
    /proc/** r,
    /sys/** r,

    # Allow writing to config directories
    /etc/aios/** rw,
    /etc/nginx/** rw,

    # Deny dangerous operations
    deny /etc/shadow rw,
    deny /etc/passwd w,
    deny /boot/** w,
    deny /usr/sbin/aios-init w,

    # Network: only through tool registry socket
    network unix stream,
    deny network inet,
    deny network inet6,
}
```

### Step 9.5: Implement Secret Management

**Claude Code prompt**: "Implement secret management — encrypted secrets file, kernel keyring storage at boot, per-agent access control"

```rust
// security/src/secrets.rs

pub struct SecretManager {
    // Secrets are stored in Linux kernel keyring, NOT in memory structs
}

impl SecretManager {
    pub fn load_from_encrypted_file(path: &str, passphrase: &str) -> Result<Self> {
        // 1. Read encrypted file
        // 2. Decrypt with passphrase (argon2 KDF + AES-256-GCM)
        // 3. Parse secrets (key-value pairs)
        // 4. Store each in kernel keyring
        // 5. Delete the encrypted file from disk
        // 6. Return manager (holds no secrets in memory)
    }

    pub fn get_secret(&self, name: &str, agent: &str) -> Result<String> {
        // 1. Check if agent is authorized to access this secret
        // 2. Read from kernel keyring
        // 3. Return value
    }

    pub fn rotate_secret(&self, name: &str, new_value: &str) -> Result<()> {
        // 1. Update kernel keyring
        // 2. Log rotation event to audit
    }
}
```

### Step 9.6: Implement Audit Ledger

**Claude Code prompt**: "Implement the append-only, hash-chained audit ledger — every security-relevant event is recorded with a hash of the previous entry for tamper detection"

```rust
// security/src/audit.rs

pub struct AuditLedger {
    db: Connection,  // SQLite at /var/lib/aios/ledger/audit.db
    last_hash: String,
}

impl AuditLedger {
    pub async fn append(&mut self, entry: AuditEntry) -> Result<()> {
        // 1. Compute hash: SHA-256(previous_hash + entry_data)
        let entry_data = serde_json::to_string(&entry)?;
        let hash = sha256(format!("{}:{}", self.last_hash, entry_data));

        // 2. Insert with hash chain
        self.db.execute(
            "INSERT INTO audit_log (id, timestamp, event_type, agent, action, target, \
             result, details, previous_hash, entry_hash) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![entry.id, entry.timestamp, entry.event_type, entry.agent,
                    entry.action, entry.target, entry.result,
                    entry_data, self.last_hash, hash],
        )?;

        self.last_hash = hash;
        Ok(())
    }

    pub async fn verify_chain(&self) -> Result<bool> {
        // Walk the entire chain, verify each hash matches
        // Returns false if any entry has been tampered with
    }
}
```

### Step 9.7: Implement Security Agent

**Claude Code prompt**: "Implement the Security Agent with intrusion detection loop, vulnerability scanning, and security policy enforcement"

### Step 9.8: Implement Podman Sandboxing for Tasks

**Claude Code prompt**: "Add Podman rootless container support for running untrusted workloads — the Task Agent should be able to spawn sandboxed containers"

---

## Deliverables Checklist

- [ ] Capability system parses and checks permissions
- [ ] Tool calls rejected when agent lacks capability
- [ ] Temporary capability grants work with auto-expiry
- [ ] Agent cgroup resource limits enforced
- [ ] AppArmor profiles created for all agent types
- [ ] Secrets loaded from encrypted file to kernel keyring
- [ ] Secrets not stored on disk in plaintext
- [ ] Audit ledger records all security events
- [ ] Hash chain verification detects tampering
- [ ] Security Agent runs IDS loop
- [ ] Podman sandbox available for untrusted tasks
- [ ] Emergency SSH access still works

---

## Next Phase
→ [Phase 10: Package Manager](./10-PACKAGES.md)
