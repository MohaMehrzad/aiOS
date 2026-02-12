# Complete Configuration Reference

## /etc/aios/config.toml — Full Schema

This is the master configuration file for aiOS. Every field is documented with its type, default, and purpose.

```toml
# ============================================================
# aiOS System Configuration
# ============================================================
# Location: /etc/aios/config.toml
# Loaded by: aios-init at boot
# Hot-reload: Some sections support reload via SIGHUP to orchestrator
# ============================================================

# --- System ---
[system]
hostname = "aios"                    # System hostname
log_level = "info"                   # debug | info | warn | error
log_file = "/var/log/aios/system.log"
autonomy_level = "full"              # full | supervised | manual
#   full:       AI operates completely autonomously
#   supervised: AI operates but logs decisions for human review
#   manual:     AI only acts on explicit human goals

# --- Boot ---
[boot]
init_timeout_seconds = 300           # Max time for full boot sequence
debug_shell = false                  # If true, spawn /bin/sh on serial after boot
clean_shutdown_flag = "/var/lib/aios/clean_shutdown"  # Presence = last shutdown was clean

# --- AI Models ---
[models]
runtime = "llama-cpp"                # Only supported runtime for now
model_dir = "/var/lib/aios/models"   # Where GGUF files are stored
llama_server_binary = "/usr/bin/llama-server"

[models.operational]
file = "tinyllama-1.1b-chat.Q4_K_M.gguf"
always_loaded = true                 # Must load at boot, boot fails if this fails
context_length = 2048
threads = 2                          # CPU threads (ignored if GPU)
gpu_layers = -1                      # -1 = all layers on GPU, 0 = CPU only
max_tokens = 512                     # Default max generation tokens
temperature = 0.1                    # Low temp for consistent system decisions

[models.tactical]
file = "mistral-7b-instruct.Q4_K_M.gguf"
always_loaded = false
load_on_demand = true
context_length = 4096
threads = 4
gpu_layers = -1
max_tokens = 2048
temperature = 0.3
unload_after_idle_minutes = 5        # Free memory when not in use

[models.tactical_alt]
file = "phi-3-mini-3.8b.Q4_K_M.gguf"
always_loaded = false
load_on_demand = true
context_length = 4096
threads = 4
gpu_layers = -1
max_tokens = 2048
temperature = 0.3
unload_after_idle_minutes = 5

# --- External API ---
[api]
enabled = true                       # Set false for fully offline operation

[api.claude]
enabled = true
base_url = "https://api.anthropic.com"
model = "claude-sonnet-4-5-20250929"
max_tokens = 4096
temperature = 0.3
monthly_budget_usd = 100.0
rate_limit_rpm = 50                  # Requests per minute
timeout_seconds = 30

[api.openai]
enabled = true
base_url = "https://api.openai.com"
model = "gpt-4o"
max_tokens = 4096
temperature = 0.3
monthly_budget_usd = 50.0
rate_limit_rpm = 50
timeout_seconds = 30
fallback_only = true                 # Only use when Claude is unavailable

[api.cache]
enabled = true
max_entries = 1000
default_ttl_hours = 1
documentation_ttl_hours = 24
never_cache_security = true          # Never cache security-related queries

# --- Memory ---
[memory]
operational_max_entries = 10000
working_db = "/var/lib/aios/memory/working.db"
longterm_db = "/var/lib/aios/memory/longterm.db"
vector_db_dir = "/var/lib/aios/vectors"

working_retention_days = 30          # Migrate to long-term after this
vacuum_interval_days = 7             # SQLite vacuum schedule
context_max_tokens = 4000            # Max tokens for context assembly

# --- Security ---
[security]
capability_mode = "strict"           # strict | permissive
#   strict:     Agents can ONLY use explicitly granted capabilities
#   permissive: Agents can use any capability (development only!)
audit_all_tool_calls = true          # Log every tool call to audit ledger
audit_db = "/var/lib/aios/ledger/audit.db"
sandbox_agents = true                # Enable cgroup + AppArmor sandboxing
sandbox_untrusted_tasks = true       # Run untrusted workloads in Podman
auto_patch = true                    # Automatically apply security patches
secrets_file = "/etc/aios/secrets.enc"

# --- Networking ---
[networking]
management_port = 9090
management_tls = true                # mTLS for management console
management_subnet = "0.0.0.0/0"      # Restrict to specific subnet in production
dhcp_timeout_seconds = 30
dns_servers = ["1.1.1.1", "8.8.8.8"]
dns_over_tls = true
firewall_default_policy = "deny"     # deny | allow
allow_outbound_https = true

# --- Agents ---
[agents]
config_dir = "/etc/aios/agents"      # Per-agent TOML configs
prompts_dir = "/etc/aios/agents/prompts"  # System prompt text files
max_instances_per_type = 3           # Max concurrent instances of one agent type
heartbeat_timeout_seconds = 15       # Mark agent unhealthy after this
max_restart_attempts = 5
restart_window_seconds = 300

# --- Monitoring ---
[monitoring]
health_check_interval_seconds = 30
metric_collection_interval_seconds = 10
anomaly_detection_enabled = true
log_rotation_max_size_mb = 100
log_rotation_keep_files = 10

# --- Package Manager ---
[packages]
backend = "apk"                      # "apk" (Alpine) or "custom"
repositories = ["https://dl-cdn.alpinelinux.org/alpine/v3.19/main"]
auto_update = true
auto_update_schedule = "daily"       # daily | weekly | never
vulnerability_check = true
vulnerability_check_schedule = "daily"
```

---

## /etc/aios/secrets.env — Format (Before Encryption)

This file is created during installation and immediately encrypted to `secrets.enc`.

```bash
# API Keys
CLAUDE_API_KEY=sk-ant-api03-xxxxxxxxxxxxxxxxxxxxx
OPENAI_API_KEY=sk-proj-xxxxxxxxxxxxxxxxxxxxx

# System Identity (generated at first boot)
SYSTEM_ED25519_PRIVATE_KEY=<base64-encoded-private-key>
SYSTEM_ED25519_PUBLIC_KEY=<base64-encoded-public-key>

# Management Console
MANAGEMENT_TOKEN=<random-256-bit-hex>

# Internal TLS CA (generated at first boot)
CA_PRIVATE_KEY=<base64-encoded-pem>
```

### Encryption/Decryption

```bash
# Encrypt (during install or key rotation)
openssl enc -aes-256-cbc -salt -pbkdf2 -iter 100000 \
    -in /etc/aios/secrets.env -out /etc/aios/secrets.enc

# Decrypt (at boot, into kernel keyring, then delete)
openssl enc -d -aes-256-cbc -salt -pbkdf2 -iter 100000 \
    -in /etc/aios/secrets.enc | aios-keyring-load
# aios-keyring-load reads stdin, stores each KEY=VALUE in kernel keyring
# Then: shred -u /etc/aios/secrets.enc
```

---

## Configuration Loading Order

```
1. aios-init reads /etc/aios/config.toml
2. aios-init decrypts secrets.enc → loads into kernel keyring
3. aios-init starts aios-runtime with [models] config
4. aios-init starts aios-memory with [memory] config
5. aios-init starts aios-tools with [security] config
6. aios-init starts aios-orchestrator with full config
7. Orchestrator reads /etc/aios/agents/*.toml
8. Orchestrator spawns agents per their configs
```
