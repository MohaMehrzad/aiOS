#!/usr/bin/env bash
# ============================================================
# first-boot.sh — aiOS First-Boot Initialization
# ============================================================
# Called by aios-init (PID 1) when /var/lib/aios/.first-boot exists.
#
# This script performs one-time system initialization:
#   1. Generate Ed25519 keypair for system identity
#   2. Initialize SQLite databases (memory, audit, working memory)
#   3. Create directory structure
#   4. Set permissions
#   5. Test network connectivity
#   6. Test API connectivity (if keys available)
#   7. Download models if not present
#   8. Run hardware detection and save results
#   9. Create system agent initial state
#  10. Remove first-boot flag
#  11. Write initialization timestamp
#
# Exit codes:
#   0 — success
#   1 — fatal error (system cannot operate)
# ============================================================
set -euo pipefail

# -----------------------------------------------------------
# Constants
# -----------------------------------------------------------
AIOS_DIR="/var/lib/aios"
AIOS_CONFIG="/etc/aios/config.toml"
AIOS_KEY_DIR="/etc/aios/keys"
FIRST_BOOT_FLAG="${AIOS_DIR}/.first-boot"
INITIALIZED_FLAG="${AIOS_DIR}/initialized"
LOG_FILE="/var/log/aios/first-boot.log"

# -----------------------------------------------------------
# Logging
# -----------------------------------------------------------
mkdir -p /var/log/aios

log() {
    local level="$1"
    shift
    local timestamp
    timestamp="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    local msg="[${timestamp}] [${level}] $*"
    echo "$msg" | tee -a "$LOG_FILE"
}

log_info()  { log "INFO"  "$@"; }
log_warn()  { log "WARN"  "$@"; }
log_error() { log "ERROR" "$@"; }
log_ok()    { log "OK"    "$@"; }

# -----------------------------------------------------------
# Error handler
# -----------------------------------------------------------
on_error() {
    local line_no="$1"
    log_error "First-boot failed at line ${line_no}"
    log_error "Check ${LOG_FILE} for details"
    # Do NOT remove the first-boot flag so it retries next boot
    exit 1
}

trap 'on_error ${LINENO}' ERR

# -----------------------------------------------------------
# Start
# -----------------------------------------------------------
log_info "============================================"
log_info "  aiOS First-Boot Initialization"
log_info "============================================"

# -----------------------------------------------------------
# Step 1: Generate Ed25519 keypair for system identity
# -----------------------------------------------------------
log_info "Step 1/10: Generating system identity keypair..."

mkdir -p "$AIOS_KEY_DIR"
chmod 700 "$AIOS_KEY_DIR"

PRIVATE_KEY="${AIOS_KEY_DIR}/system_ed25519"
PUBLIC_KEY="${AIOS_KEY_DIR}/system_ed25519.pub"

if [ -f "$PRIVATE_KEY" ] && [ -f "$PUBLIC_KEY" ]; then
    log_warn "Keypair already exists, skipping generation"
else
    # Generate Ed25519 keypair without passphrase (system key)
    ssh-keygen -t ed25519 -f "$PRIVATE_KEY" -N "" -C "aios-system@$(cat /etc/hostname 2>/dev/null || echo 'aios')" \
        >> "$LOG_FILE" 2>&1

    chmod 600 "$PRIVATE_KEY"
    chmod 644 "$PUBLIC_KEY"

    log_ok "System identity keypair generated"
    log_info "  Public key: $(cat "$PUBLIC_KEY")"
fi

# Also generate a signing key for the audit ledger
SIGNING_KEY="${AIOS_KEY_DIR}/ledger_ed25519"
if [ ! -f "$SIGNING_KEY" ]; then
    ssh-keygen -t ed25519 -f "$SIGNING_KEY" -N "" -C "aios-ledger@$(cat /etc/hostname 2>/dev/null || echo 'aios')" \
        >> "$LOG_FILE" 2>&1
    chmod 600 "$SIGNING_KEY"
    chmod 644 "${SIGNING_KEY}.pub"
    log_ok "Audit ledger signing key generated"
fi

# -----------------------------------------------------------
# Step 2: Initialize SQLite databases
# -----------------------------------------------------------
log_info "Step 2/10: Initializing databases..."

MEMORY_DB="${AIOS_DIR}/memory/memory.db"
AUDIT_DB="${AIOS_DIR}/ledger/audit.db"
WORKING_DB="${AIOS_DIR}/memory/working.db"

mkdir -p "${AIOS_DIR}/memory"
mkdir -p "${AIOS_DIR}/ledger"

# Initialize long-term memory database
if [ ! -f "$MEMORY_DB" ]; then
    sqlite3 "$MEMORY_DB" << 'SQL'
CREATE TABLE IF NOT EXISTS memories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content TEXT NOT NULL,
    embedding BLOB,
    memory_type TEXT NOT NULL DEFAULT 'episodic',
    importance REAL NOT NULL DEFAULT 0.5,
    access_count INTEGER NOT NULL DEFAULT 0,
    last_accessed TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    metadata TEXT
);

CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories(importance DESC);
CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at DESC);

CREATE TABLE IF NOT EXISTS memory_associations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    target_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    relation_type TEXT NOT NULL,
    strength REAL NOT NULL DEFAULT 0.5,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_assoc_source ON memory_associations(source_id);
CREATE INDEX IF NOT EXISTS idx_assoc_target ON memory_associations(target_id);

CREATE TABLE IF NOT EXISTS memory_metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO memory_metadata (key, value) VALUES ('schema_version', '1');
INSERT INTO memory_metadata (key, value) VALUES ('created_at', datetime('now'));
SQL
    log_ok "Long-term memory database initialized: ${MEMORY_DB}"
fi

# Initialize audit ledger database
if [ ! -f "$AUDIT_DB" ]; then
    sqlite3 "$AUDIT_DB" << 'SQL'
CREATE TABLE IF NOT EXISTS audit_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
    event_type TEXT NOT NULL,
    agent TEXT,
    action TEXT NOT NULL,
    target TEXT,
    result TEXT,
    details TEXT,
    hash TEXT,
    previous_hash TEXT
);

CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_events(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_audit_type ON audit_events(event_type);
CREATE INDEX IF NOT EXISTS idx_audit_agent ON audit_events(agent);

CREATE TABLE IF NOT EXISTS audit_metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO audit_metadata (key, value) VALUES ('schema_version', '1');
INSERT INTO audit_metadata (key, value) VALUES ('created_at', datetime('now'));
INSERT INTO audit_metadata (key, value) VALUES ('chain_initialized', 'true');
SQL
    log_ok "Audit ledger database initialized: ${AUDIT_DB}"
fi

# Initialize working memory database
if [ ! -f "$WORKING_DB" ]; then
    sqlite3 "$WORKING_DB" << 'SQL'
CREATE TABLE IF NOT EXISTS working_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    agent TEXT NOT NULL,
    content TEXT NOT NULL,
    item_type TEXT NOT NULL DEFAULT 'thought',
    priority REAL NOT NULL DEFAULT 0.5,
    ttl_seconds INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_working_agent ON working_items(agent);
CREATE INDEX IF NOT EXISTS idx_working_priority ON working_items(priority DESC);
CREATE INDEX IF NOT EXISTS idx_working_expires ON working_items(expires_at);

CREATE TABLE IF NOT EXISTS active_goals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    goal TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    priority REAL NOT NULL DEFAULT 0.5,
    assigned_agent TEXT,
    parent_goal_id INTEGER REFERENCES active_goals(id),
    progress REAL NOT NULL DEFAULT 0.0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_goals_status ON active_goals(status);
CREATE INDEX IF NOT EXISTS idx_goals_agent ON active_goals(assigned_agent);

CREATE TABLE IF NOT EXISTS context_stack (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    agent TEXT NOT NULL,
    context_data TEXT NOT NULL,
    depth INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_context_agent ON context_stack(agent);
SQL
    log_ok "Working memory database initialized: ${WORKING_DB}"
fi

# -----------------------------------------------------------
# Step 3: Create directory structure
# -----------------------------------------------------------
log_info "Step 3/10: Creating directory structure..."

DIRECTORIES=(
    "${AIOS_DIR}/memory"
    "${AIOS_DIR}/models"
    "${AIOS_DIR}/tasks"
    "${AIOS_DIR}/scratch"
    "${AIOS_DIR}/cache"
    "${AIOS_DIR}/vectors"
    "${AIOS_DIR}/ledger"
)

for dir in "${DIRECTORIES[@]}"; do
    mkdir -p "$dir"
    log_info "  Created: ${dir}"
done

log_ok "Directory structure created"

# -----------------------------------------------------------
# Step 4: Set proper permissions
# -----------------------------------------------------------
log_info "Step 4/10: Setting permissions..."

# aiOS data directories — owned by root, world-readable structure
chmod 755 "$AIOS_DIR"
chmod 750 "${AIOS_DIR}/memory"
chmod 750 "${AIOS_DIR}/models"
chmod 750 "${AIOS_DIR}/tasks"
chmod 700 "${AIOS_DIR}/scratch"
chmod 750 "${AIOS_DIR}/cache"
chmod 750 "${AIOS_DIR}/vectors"
chmod 750 "${AIOS_DIR}/ledger"

# Database files — owner read/write only
find "${AIOS_DIR}/memory" -name "*.db" -exec chmod 600 {} \; 2>/dev/null || true
find "${AIOS_DIR}/ledger" -name "*.db" -exec chmod 600 {} \; 2>/dev/null || true

# Key directory — strict permissions
chmod 700 "$AIOS_KEY_DIR"
find "$AIOS_KEY_DIR" -name "*_ed25519" -exec chmod 600 {} \; 2>/dev/null || true
find "$AIOS_KEY_DIR" -name "*.pub" -exec chmod 644 {} \; 2>/dev/null || true

# Config files
chmod 600 "$AIOS_CONFIG" 2>/dev/null || true

# Log directory
chmod 755 /var/log/aios

log_ok "Permissions set"

# -----------------------------------------------------------
# Step 5: Test network connectivity
# -----------------------------------------------------------
log_info "Step 5/10: Testing network connectivity..."

NETWORK_OK=false

# Try multiple endpoints
CONNECTIVITY_TARGETS=(
    "1.1.1.1"
    "8.8.8.8"
    "9.9.9.9"
)

for target in "${CONNECTIVITY_TARGETS[@]}"; do
    if ping -c 1 -W 3 "$target" >> "$LOG_FILE" 2>&1; then
        NETWORK_OK=true
        log_ok "Network connectivity confirmed (reached ${target})"
        break
    fi
done

if [ "$NETWORK_OK" = false ]; then
    # Try DNS-based check as fallback
    if command -v wget >/dev/null 2>&1; then
        if wget -q --spider --timeout=5 "https://api.anthropic.com" 2>/dev/null; then
            NETWORK_OK=true
            log_ok "Network connectivity confirmed via HTTPS"
        fi
    elif command -v curl >/dev/null 2>&1; then
        if curl -s --max-time 5 --head "https://api.anthropic.com" >/dev/null 2>&1; then
            NETWORK_OK=true
            log_ok "Network connectivity confirmed via HTTPS"
        fi
    fi
fi

if [ "$NETWORK_OK" = false ]; then
    log_warn "No network connectivity detected. Cloud AI features will be unavailable."
    log_warn "The system will operate in local-only mode."
fi

# -----------------------------------------------------------
# Step 6: Test API connectivity (if keys exist)
# -----------------------------------------------------------
log_info "Step 6/10: Testing API connectivity..."

API_TESTED=false

# Check if decrypted secrets are available in environment or secrets file
CLAUDE_API_KEY="${CLAUDE_API_KEY:-}"
OPENAI_API_KEY="${OPENAI_API_KEY:-}"

# Try to read from secrets.env if it exists (pre-encryption)
if [ -f "/etc/aios/secrets.env" ]; then
    source_val="$(grep '^CLAUDE_API_KEY=' /etc/aios/secrets.env 2>/dev/null | cut -d= -f2- || true)"
    if [ -n "$source_val" ]; then
        CLAUDE_API_KEY="$source_val"
    fi
    source_val="$(grep '^OPENAI_API_KEY=' /etc/aios/secrets.env 2>/dev/null | cut -d= -f2- || true)"
    if [ -n "$source_val" ]; then
        OPENAI_API_KEY="$source_val"
    fi
fi

if [ "$NETWORK_OK" = true ]; then
    # Test Claude API
    if [ -n "$CLAUDE_API_KEY" ]; then
        log_info "  Testing Claude API connectivity..."
        HTTP_CODE=""
        if command -v curl >/dev/null 2>&1; then
            HTTP_CODE="$(curl -s -o /dev/null -w '%{http_code}' \
                --max-time 10 \
                -H "x-api-key: ${CLAUDE_API_KEY}" \
                -H "anthropic-version: 2023-06-01" \
                "https://api.anthropic.com/v1/messages" 2>/dev/null || echo "000")"
        elif command -v wget >/dev/null 2>&1; then
            if wget -q --spider --timeout=10 \
                --header="x-api-key: ${CLAUDE_API_KEY}" \
                --header="anthropic-version: 2023-06-01" \
                "https://api.anthropic.com/v1/messages" 2>/dev/null; then
                HTTP_CODE="200"
            else
                HTTP_CODE="000"
            fi
        fi

        # Any response (even 400/401) means the API is reachable
        if [ -n "$HTTP_CODE" ] && [ "$HTTP_CODE" != "000" ]; then
            log_ok "  Claude API reachable (HTTP ${HTTP_CODE})"
            API_TESTED=true
        else
            log_warn "  Claude API unreachable"
        fi
    else
        log_info "  No Claude API key configured, skipping"
    fi

    # Test OpenAI API
    if [ -n "$OPENAI_API_KEY" ]; then
        log_info "  Testing OpenAI API connectivity..."
        HTTP_CODE=""
        if command -v curl >/dev/null 2>&1; then
            HTTP_CODE="$(curl -s -o /dev/null -w '%{http_code}' \
                --max-time 10 \
                -H "Authorization: Bearer ${OPENAI_API_KEY}" \
                "https://api.openai.com/v1/models" 2>/dev/null || echo "000")"
        fi

        if [ -n "$HTTP_CODE" ] && [ "$HTTP_CODE" != "000" ]; then
            log_ok "  OpenAI API reachable (HTTP ${HTTP_CODE})"
            API_TESTED=true
        else
            log_warn "  OpenAI API unreachable"
        fi
    else
        log_info "  No OpenAI API key configured, skipping"
    fi
else
    log_info "  Skipping API tests (no network)"
fi

if [ "$API_TESTED" = false ] && [ -z "$CLAUDE_API_KEY" ] && [ -z "$OPENAI_API_KEY" ]; then
    log_info "  No API keys configured. System will use local models only."
fi

# -----------------------------------------------------------
# Step 7: Download models if not present
# -----------------------------------------------------------
log_info "Step 7/10: Checking AI models..."

MODEL_DIR="${AIOS_DIR}/models"
MODELS_PRESENT=false

# Check if any GGUF models exist
if ls "${MODEL_DIR}"/*.gguf >/dev/null 2>&1; then
    MODEL_COUNT="$(ls -1 "${MODEL_DIR}"/*.gguf 2>/dev/null | wc -l)"
    log_ok "Found ${MODEL_COUNT} model(s) in ${MODEL_DIR}"
    for model_file in "${MODEL_DIR}"/*.gguf; do
        log_info "  $(basename "$model_file")"
    done
    MODELS_PRESENT=true
fi

if [ "$MODELS_PRESENT" = false ] && [ "$NETWORK_OK" = true ]; then
    log_info "No models found. Attempting to download..."
    if [ -x "/usr/lib/aios/download-models.sh" ]; then
        /usr/lib/aios/download-models.sh >> "$LOG_FILE" 2>&1 || log_warn "Model download failed (will retry later)"
    elif [ -x "/opt/aios/scripts/download-models.sh" ]; then
        /opt/aios/scripts/download-models.sh >> "$LOG_FILE" 2>&1 || log_warn "Model download failed (will retry later)"
    else
        log_warn "No model download script found. Models must be installed manually."
        log_info "Place GGUF model files in ${MODEL_DIR}/"
    fi
elif [ "$MODELS_PRESENT" = false ]; then
    log_warn "No models found and no network available."
    log_warn "Place GGUF model files in ${MODEL_DIR}/ to enable local inference."
fi

# -----------------------------------------------------------
# Step 8: Run hardware detection and save results
# -----------------------------------------------------------
log_info "Step 8/10: Detecting hardware..."

HARDWARE_JSON="${AIOS_DIR}/hardware.json"

# Gather hardware information
CPU_MODEL="$(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2 | sed 's/^ //' || echo 'unknown')"
CPU_COUNT="$(nproc 2>/dev/null || grep -c '^processor' /proc/cpuinfo 2>/dev/null || echo '1')"
RAM_KB="$(grep '^MemTotal' /proc/meminfo 2>/dev/null | awk '{print $2}' || echo '0')"
RAM_MB="$((RAM_KB / 1024))"

# Detect GPU
GPU_DETECTED=false
GPU_NAME="none"
GPU_VRAM_MB=0

if [ -d /proc/driver/nvidia ] || lspci 2>/dev/null | grep -qi nvidia; then
    GPU_DETECTED=true
    GPU_NAME="$(lspci 2>/dev/null | grep -i 'vga.*nvidia' | head -1 | sed 's/.*: //' || echo 'NVIDIA (unknown)')"
    if command -v nvidia-smi >/dev/null 2>&1; then
        GPU_VRAM_MB="$(nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits 2>/dev/null | head -1 || echo '0')"
    fi
elif lspci 2>/dev/null | grep -qi 'amd.*radeon\|amd.*vga'; then
    GPU_DETECTED=true
    GPU_NAME="$(lspci 2>/dev/null | grep -i 'vga.*amd' | head -1 | sed 's/.*: //' || echo 'AMD (unknown)')"
fi

# Detect storage
STORAGE_DEVICES="[]"
if command -v lsblk >/dev/null 2>&1; then
    STORAGE_DEVICES="$(lsblk -Jd -o NAME,SIZE,TYPE,MODEL 2>/dev/null | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    disks = [d for d in data.get('blockdevices', []) if d.get('type') == 'disk']
    print(json.dumps(disks))
except:
    print('[]')
" 2>/dev/null || echo '[]')"
fi

# Detect network interfaces
NET_INTERFACES="[]"
if [ -d /sys/class/net ]; then
    interfaces=""
    for iface_dir in /sys/class/net/*; do
        iface="$(basename "$iface_dir")"
        if [ "$iface" = "lo" ]; then
            continue
        fi
        mac="$(cat "${iface_dir}/address" 2>/dev/null || echo 'unknown')"
        state="$(cat "${iface_dir}/operstate" 2>/dev/null || echo 'unknown')"
        if [ -n "$interfaces" ]; then
            interfaces="${interfaces},"
        fi
        interfaces="${interfaces}{\"name\":\"${iface}\",\"mac\":\"${mac}\",\"state\":\"${state}\"}"
    done
    NET_INTERFACES="[${interfaces}]"
fi

# Write hardware.json
cat > "$HARDWARE_JSON" << HWJSON
{
    "detected_at": "$(date -u '+%Y-%m-%dT%H:%M:%SZ')",
    "cpu": {
        "model": "${CPU_MODEL}",
        "cores": ${CPU_COUNT},
        "architecture": "$(uname -m 2>/dev/null || echo 'unknown')"
    },
    "memory": {
        "total_mb": ${RAM_MB},
        "total_kb": ${RAM_KB}
    },
    "gpu": {
        "detected": ${GPU_DETECTED},
        "name": "${GPU_NAME}",
        "vram_mb": ${GPU_VRAM_MB}
    },
    "storage": ${STORAGE_DEVICES},
    "network": ${NET_INTERFACES},
    "kernel": "$(uname -r 2>/dev/null || echo 'unknown')",
    "boot_mode": "$([ -d /sys/firmware/efi ] && echo 'uefi' || echo 'bios')"
}
HWJSON

chmod 644 "$HARDWARE_JSON"
log_ok "Hardware detection complete: ${CPU_COUNT} CPUs, ${RAM_MB} MB RAM, GPU: ${GPU_DETECTED}"

# -----------------------------------------------------------
# Step 9: Create system agent initial state
# -----------------------------------------------------------
log_info "Step 9/10: Creating system agent initial state..."

AGENT_STATE_DIR="${AIOS_DIR}/agents"
mkdir -p "$AGENT_STATE_DIR"

cat > "${AGENT_STATE_DIR}/system-agent.json" << AGENTJSON
{
    "agent": "SystemAgent",
    "status": "initialized",
    "created_at": "$(date -u '+%Y-%m-%dT%H:%M:%SZ')",
    "boot_count": 1,
    "last_boot": "$(date -u '+%Y-%m-%dT%H:%M:%SZ')",
    "capabilities_verified": true,
    "hardware_profile": "${HARDWARE_JSON}",
    "operational_mode": "autonomous",
    "intelligence_level": "local",
    "goals": [
        {
            "id": 1,
            "goal": "Monitor system health and maintain stability",
            "type": "standing",
            "priority": 1.0,
            "status": "active"
        },
        {
            "id": 2,
            "goal": "Optimize resource utilization",
            "type": "standing",
            "priority": 0.7,
            "status": "active"
        }
    ],
    "context": {
        "first_boot": true,
        "network_available": ${NETWORK_OK},
        "api_keys_configured": $([ -n "$CLAUDE_API_KEY" ] || [ -n "$OPENAI_API_KEY" ] && echo true || echo false),
        "models_available": ${MODELS_PRESENT}
    }
}
AGENTJSON

chmod 644 "${AGENT_STATE_DIR}/system-agent.json"
log_ok "System agent initial state created"

# Record the first boot in the audit ledger
if [ -f "$AUDIT_DB" ]; then
    sqlite3 "$AUDIT_DB" << SQL
INSERT INTO audit_events (event_type, agent, action, target, result, details)
VALUES (
    'system',
    'installer',
    'first_boot_init',
    'system',
    'success',
    '{"cpu_count": ${CPU_COUNT}, "ram_mb": ${RAM_MB}, "gpu": ${GPU_DETECTED}, "network": ${NETWORK_OK}}'
);
SQL
    log_ok "First boot recorded in audit ledger"
fi

# -----------------------------------------------------------
# Step 10: Finalize — remove flag, write timestamp
# -----------------------------------------------------------
log_info "Step 10/10: Finalizing..."

# Remove first-boot flag
rm -f "$FIRST_BOOT_FLAG"
log_ok "Removed first-boot flag"

# Create initialized marker with timestamp and details
cat > "$INITIALIZED_FLAG" << INIT
{
    "initialized_at": "$(date -u '+%Y-%m-%dT%H:%M:%SZ')",
    "version": "0.1.0",
    "hostname": "$(cat /etc/hostname 2>/dev/null || echo 'aios')",
    "network_at_init": ${NETWORK_OK},
    "api_configured": $([ -n "$CLAUDE_API_KEY" ] || [ -n "$OPENAI_API_KEY" ] && echo true || echo false),
    "models_present": ${MODELS_PRESENT},
    "hardware": {
        "cpus": ${CPU_COUNT},
        "ram_mb": ${RAM_MB},
        "gpu": ${GPU_DETECTED}
    }
}
INIT

chmod 644 "$INITIALIZED_FLAG"

# -----------------------------------------------------------
# Complete
# -----------------------------------------------------------
log_info "============================================"
log_info "  First boot complete. System autonomous."
log_info "============================================"
log_info ""
log_info "  Hostname:     $(cat /etc/hostname 2>/dev/null || echo 'aios')"
log_info "  Hardware:     ${CPU_COUNT} CPUs, ${RAM_MB} MB RAM"
log_info "  GPU:          ${GPU_NAME}"
log_info "  Network:      ${NETWORK_OK}"
log_info "  Models:       ${MODELS_PRESENT}"
log_info "  Init log:     ${LOG_FILE}"
log_info ""
log_info "First boot complete. System autonomous."

exit 0
