# Networking Architecture

## Overview

The AI controls the entire network stack. No static configuration files that humans edit — the Network Agent dynamically manages interfaces, routing, DNS, firewall, and connectivity based on goals and system state.

---

## Network Stack

```
┌─────────────────────────────────────────────┐
│              NETWORK AGENT                   │
│   Decision making, policy, troubleshooting   │
├─────────────────────────────────────────────┤
│            NETWORK MANAGER                    │
│   Interface config, DHCP client, routing      │
│   Implementation: networkd (lightweight)      │
├─────────────────────────────────────────────┤
│              FIREWALL                         │
│   nftables (kernel firewall)                  │
│   Managed by Network Agent via tools          │
├─────────────────────────────────────────────┤
│               DNS                             │
│   Local resolver (unbound)                    │
│   Caching + forwarding to upstream            │
├─────────────────────────────────────────────┤
│         LINUX KERNEL NETWORKING               │
│   TCP/IP, routing, netfilter, bridge, veth    │
└─────────────────────────────────────────────┘
```

---

## Interface Management

### Boot-Time Detection
```python
# Network Agent: first boot sequence
async def configure_network():
    # 1. Detect all interfaces
    interfaces = await tool("net.interfaces")

    # 2. For each interface, determine type and configure
    for iface in interfaces:
        if iface.is_loopback:
            continue  # lo is always up

        if iface.has_link:
            # Physical interface with cable/link detected
            if await dhcp_available(iface):
                await tool("net.configure", iface=iface.name, method="dhcp")
            else:
                # Use local model to decide: check common subnets, assign static
                config = await local_model.suggest_network_config(iface)
                await tool("net.configure", iface=iface.name, **config)

    # 3. Verify connectivity
    can_reach_internet = await tool("net.ping", host="1.1.1.1")
    can_resolve_dns = await tool("net.dns_lookup", domain="api.anthropic.com")

    if not can_reach_internet:
        # Escalate to Claude for troubleshooting
        await escalate("No internet connectivity after network configuration")
```

### Dynamic Reconfiguration
The Network Agent monitors link status and reconfigures on changes:
- Cable unplugged → detect, log, try alternate interface
- DHCP lease expired → renew or switch to static
- New interface appears → configure automatically

---

## DNS Configuration

### Local Resolver: `unbound`
Runs on localhost:53, all system DNS goes through it.

```
Application → unbound (localhost:53) → upstream DNS servers
                 │
                 └── Local cache (reduces external queries)
                 └── Query logging (AI can analyze DNS patterns)
                 └── Block malicious domains (updated by Security Agent)
```

### Configuration
```yaml
# Managed by Network Agent, written to /etc/unbound/unbound.conf
server:
  interface: 127.0.0.1
  access-control: 127.0.0.0/8 allow
  cache-max-ttl: 86400
  cache-min-ttl: 300

  # Security: block known malicious domains
  # This file is updated by Security Agent
  include: /etc/unbound/blocklist.conf

forward-zone:
  name: "."
  forward-addr: 1.1.1.1         # Cloudflare (primary)
  forward-addr: 8.8.8.8         # Google (fallback)
  forward-tls-upstream: yes     # DNS over TLS
```

---

## Firewall (nftables)

### Architecture
```
Network Agent → firewall.* tools → nftables kernel API
```

### Default Ruleset
```nft
table inet aios_firewall {
    chain input {
        type filter hook input priority 0; policy drop;

        # Always allow loopback
        iif "lo" accept

        # Allow established connections
        ct state established,related accept

        # Management console (restricted subnet)
        tcp dport 9090 ip saddr $MANAGEMENT_SUBNET accept
        tcp dport 22 ip saddr $MANAGEMENT_SUBNET accept

        # ICMP (ping)
        icmp type echo-request accept
        icmpv6 type echo-request accept

        # Everything else: DROP (logged)
        log prefix "aios-fw-drop: " drop
    }

    chain output {
        type filter hook output priority 0; policy drop;

        # Loopback
        oif "lo" accept

        # Established
        ct state established,related accept

        # DNS
        tcp dport 53 accept
        udp dport 53 accept

        # HTTPS (API calls, package downloads)
        tcp dport 443 accept

        # NTP
        udp dport 123 accept

        # Everything else: DROP (logged)
        log prefix "aios-fw-out-drop: " drop
    }

    chain forward {
        type filter hook forward priority 0; policy drop;
        # Container traffic rules added dynamically
    }
}
```

### Dynamic Rules
The Network Agent adds/removes rules as needed:
```python
# Example: Agent needs to expose nginx on port 80
await tool("firewall.allow", {
    "chain": "input",
    "protocol": "tcp",
    "port": 80,
    "source": "any",
    "comment": "nginx web server for goal-456",
    "expires": None  # Permanent until removed
})
```

---

## Container Networking

For Podman containers (sandboxed workloads):

### Network Modes
| Mode | Use Case |
|---|---|
| `none` | No network (default for untrusted tasks) |
| `host-loopback` | Can reach localhost services only |
| `bridged` | Full network via bridge interface |
| `custom` | Specific ports/destinations only |

### Implementation
```
Host network namespace
├── eth0 (physical, managed by Network Agent)
├── br-aios (bridge for containers)
│   ├── veth-task-001 → Container 1
│   ├── veth-task-002 → Container 2
│   └── veth-task-003 → Container 3
└── lo (loopback)
```

---

## VPN Support

### WireGuard (Preferred)
Built into the kernel, minimal overhead.

```python
# Network Agent: set up WireGuard tunnel
await tool("net.wireguard_setup", {
    "interface": "wg0",
    "private_key": await keyring.get("wireguard_private"),
    "listen_port": 51820,
    "peers": [
        {
            "public_key": "peer_pubkey_here",
            "endpoint": "remote.server.com:51820",
            "allowed_ips": "10.0.0.0/24",
            "keepalive": 25
        }
    ]
})
```

### Use Cases
- Secure management access from remote locations
- Inter-node communication for multi-node aiOS deployments
- Accessing private resources through corporate VPN

---

## Network Monitoring

The Monitor Agent tracks network metrics:

### Metrics Collected
| Metric | Frequency | Storage |
|---|---|---|
| Interface bytes in/out | 5s | Operational memory |
| Active connections count | 10s | Operational memory |
| DNS query latency | Per query | Working memory |
| Firewall drop count | 30s | Working memory |
| API endpoint latency | Per call | Working memory |
| Bandwidth utilization % | 5s | Operational memory |

### Anomaly Detection
The local model watches for:
- Unusual outbound traffic volume (possible exfiltration)
- Many connection attempts to unknown IPs
- DNS queries for suspicious domains
- Sudden spike in firewall drops (possible scan/attack)
- API latency degradation

---

## Service Discovery

Internal services register with the orchestrator:

```python
# When a service starts, it registers
await orchestrator.register_service(
    name="nginx",
    address="127.0.0.1",
    port=80,
    health_check="/health",
    managed_by="aios-agent-system"
)

# Other agents can discover services
nginx = await orchestrator.find_service("nginx")
# Returns: {name: "nginx", address: "127.0.0.1", port: 80, status: "healthy"}
```

---

## Offline Mode

When internet connectivity is lost:

```
1. Network Agent detects loss of connectivity
2. Notifies orchestrator: "Internet offline"
3. Orchestrator switches to offline mode:
   - All API calls queued (not dropped)
   - Local models handle all decisions
   - Goals requiring API are paused
   - System continues operating with local intelligence
4. Network Agent continuously attempts reconnection
5. On reconnection:
   - Queued API calls are processed
   - Paused goals resume
   - Orchestrator reports offline duration and actions taken
```
