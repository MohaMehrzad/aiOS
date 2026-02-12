# Management Console Architecture

## Overview

The Management Console is the human-facing interface to aiOS. It's a lightweight HTTP API + static web dashboard that allows humans to observe, direct, and override the AI. It is NOT required for normal operation — aiOS runs autonomously without it.

---

## Architecture

```
┌─────────────────────────────────────────┐
│          WEB BROWSER / curl              │
│          (Human operator)                │
└──────────────┬──────────────────────────┘
               │ HTTPS (port 9090)
┌──────────────▼──────────────────────────┐
│       MANAGEMENT CONSOLE SERVICE         │
│                                          │
│  ┌────────────┐  ┌───────────────────┐  │
│  │ Static     │  │ REST API          │  │
│  │ Dashboard  │  │ /api/*            │  │
│  │ (HTML/JS)  │  │                   │  │
│  └────────────┘  └───────┬───────────┘  │
│                          │ gRPC          │
│                  ┌───────▼───────────┐  │
│                  │ Orchestrator      │  │
│                  │ Client            │  │
│                  └───────────────────┘  │
└─────────────────────────────────────────┘
```

---

## REST API Endpoints

### System Status
```
GET /api/status
Response: {
    "uptime_seconds": 86400,
    "autonomy_level": "full",
    "agents": {
        "system":   {"status": "healthy", "tasks_completed": 142},
        "network":  {"status": "healthy", "tasks_completed": 87},
        "security": {"status": "healthy", "tasks_completed": 56},
        "monitor":  {"status": "healthy", "tasks_completed": 312},
        "package":  {"status": "healthy", "tasks_completed": 23},
        "storage":  {"status": "healthy", "tasks_completed": 18},
        "task":     {"status": "idle",    "tasks_completed": 45},
        "dev":      {"status": "idle",    "tasks_completed": 12}
    },
    "resources": {
        "cpu_percent": 12.5,
        "memory_used_gb": 8.2,
        "memory_total_gb": 32.0,
        "disk_used_gb": 45.0,
        "disk_total_gb": 512.0,
        "gpu_utilization_percent": 35.0
    },
    "models_loaded": ["tinyllama-1.1b", "mistral-7b"],
    "api_budget": {
        "monthly_limit_usd": 100.0,
        "spent_usd": 23.45,
        "remaining_usd": 76.55
    }
}
```

### Goal Management
```
POST /api/goals
Body: {"description": "Install and configure PostgreSQL 16"}
Response: {"goal_id": "goal-abc123", "status": "pending"}

GET /api/goals
Response: {"goals": [
    {"id": "goal-abc123", "description": "...", "status": "active", "progress": 0.6},
    {"id": "goal-def456", "description": "...", "status": "completed"}
]}

GET /api/goals/:id
Response: {full goal detail with task breakdown, timeline, decisions made}

DELETE /api/goals/:id
Response: {"status": "cancelled"}
```

### Audit Log
```
GET /api/audit?limit=50&agent=system&level=high
Response: {"entries": [{audit entries}]}
```

### API Budget
```
GET /api/budget
Response: {daily and monthly cost breakdown per provider}
```

### Emergency Controls
```
POST /api/emergency/stop
Response: {"status": "all agents halted"}
# Immediately stops all agent activity. System enters manual mode.

POST /api/emergency/resume
Response: {"status": "autonomy resumed"}
# Re-enables autonomous operation.

POST /api/autonomy
Body: {"level": "supervised"}  # full | supervised | manual
Response: {"previous": "full", "current": "supervised"}
```

---

## Authentication

- **mTLS**: Client certificate required (generated during install)
- **Fallback**: Bearer token authentication for API access
- **Management subnet restriction**: Firewall limits access to configured subnet
- Token stored in `/etc/aios/management-token` (read-only by console service)

---

## Dashboard (Static HTML)

Minimal single-page dashboard built with vanilla HTML/JS (no framework, no build step):

```
┌─────────────────────────────────────────────────────┐
│  aiOS Dashboard                    [Emergency Stop]  │
├─────────────────────────────────────────────────────┤
│                                                      │
│  System Health          Resources                    │
│  ● All agents healthy   CPU: ████░░░░ 35%           │
│  ● Uptime: 14d 6h       RAM: ██████░░ 75%           │
│  ● Autonomy: Full       Disk: ███░░░░░ 38%          │
│                          GPU: ██░░░░░░ 25%           │
│                                                      │
│  Active Goals                                        │
│  ┌────────────────────────────────────────────────┐ │
│  │ goal-abc: Install PostgreSQL     [████████░░]  │ │
│  │ goal-def: Optimize disk usage    [██░░░░░░░░]  │ │
│  └────────────────────────────────────────────────┘ │
│                                                      │
│  Submit New Goal                                     │
│  [____________________________________________] [Go] │
│                                                      │
│  Recent Activity (last 10 actions)                   │
│  12:34 — System Agent: wrote /etc/nginx/nginx.conf   │
│  12:33 — Package Agent: installed nginx 1.24.0       │
│  12:30 — Network Agent: opened port 80               │
│                                                      │
│  API Budget: $23.45 / $100.00 this month             │
└─────────────────────────────────────────────────────┘
```

### Implementation
- Single `index.html` file (~500 lines)
- Vanilla JavaScript, `fetch()` for API calls
- Auto-refresh every 5 seconds
- No dependencies, no build step
- Served by the management console Rust service (embedded static files)

---

## Implementation

The management console is a lightweight HTTP server embedded in the orchestrator (not a separate service):

```rust
// In agent-core/src/management.rs
use axum::{Router, routing::get, routing::post, Json};

pub fn management_router() -> Router {
    Router::new()
        .route("/", get(serve_dashboard))
        .route("/api/status", get(get_status))
        .route("/api/goals", get(list_goals).post(create_goal))
        .route("/api/goals/:id", get(get_goal).delete(cancel_goal))
        .route("/api/audit", get(get_audit))
        .route("/api/budget", get(get_budget))
        .route("/api/emergency/stop", post(emergency_stop))
        .route("/api/emergency/resume", post(emergency_resume))
        .route("/api/autonomy", post(set_autonomy))
        .layer(/* mTLS + auth middleware */)
}
```

Add `axum` to workspace dependencies:
```toml
axum = "0.7"
tower-http = { version = "0.5", features = ["fs", "cors"] }
```
