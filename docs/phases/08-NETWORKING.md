# Phase 8: AI-Controlled Networking

## Goal
The Network Agent fully manages all networking — interface configuration, DNS, firewall rules, VPN, and connectivity monitoring. No static config files.

## Prerequisites
- Phase 6 complete (tool registry with basic net.* tools)
- Read [architecture/NETWORKING.md](../architecture/NETWORKING.md)

---

## Step-by-Step

### Step 8.1: Implement Full Network Tools

**Claude Code prompt**: "Implement the complete set of network and firewall tools — net.configure, net.connections, firewall.rules, firewall.allow, firewall.deny, firewall.remove, dns.configure"

Additional tools beyond Phase 6 basics:
```
net.configure       — Set IP address, netmask, gateway, DNS for an interface
net.route_add       — Add routing table entry
net.route_del       — Remove routing table entry
net.route_list      — List routing table
net.connections     — List all active TCP/UDP connections
net.bandwidth       — Get current bandwidth usage per interface
firewall.rules      — List all nftables rules
firewall.allow      — Add allow rule
firewall.deny       — Add deny rule
firewall.remove     — Remove a specific rule
dns.configure       — Configure unbound DNS resolver
dns.add_record      — Add local DNS record
dns.blocklist       — Add domain to blocklist
```

### Step 8.2: Install and Configure unbound

**Claude Code prompt**: "Add unbound DNS resolver to the rootfs, create a default configuration, and add tools for the Network Agent to manage it"

Include in rootfs build:
- Static unbound binary
- Default config pointing to 1.1.1.1 and 8.8.8.8 with DNS-over-TLS
- Network Agent can update config and reload

### Step 8.3: Implement nftables Management

**Claude Code prompt**: "Implement nftables firewall management — create the default ruleset at boot, provide tools for dynamic rule addition/removal"

The default ruleset from [architecture/NETWORKING.md](../architecture/NETWORKING.md) is applied at boot by the Network Agent. The agent then modifies rules dynamically.

### Step 8.4: Implement Network Agent Logic

**Claude Code prompt**: "Implement the full Network Agent — boot-time network detection and configuration, dynamic firewall management, connectivity monitoring, and offline detection"

```python
# agent-core/python/aios_agent/agents/network.py

class NetworkAgent(BaseAgent):
    @property
    def system_prompt(self):
        return """You are the Network Agent for aiOS. You manage all networking:
        interfaces, IP configuration, DNS, firewall, routing, and VPN.
        At boot, detect and configure all interfaces.
        Continuously monitor connectivity and adapt to changes.
        Always maintain the firewall in a secure default-deny state.
        Log all network changes with reasons."""

    async def on_boot(self):
        """Called at system boot to initialize networking."""
        # 1. Detect interfaces
        interfaces = await self.call_tool("net.interfaces")

        # 2. Configure each interface
        for iface in interfaces.data["interfaces"]:
            if iface["name"] == "lo":
                continue
            await self.configure_interface(iface)

        # 3. Set up DNS
        await self.setup_dns()

        # 4. Apply default firewall
        await self.apply_default_firewall()

        # 5. Verify connectivity
        await self.verify_connectivity()

    async def configure_interface(self, iface: dict):
        """Auto-detect and configure a network interface."""
        # Try DHCP first
        result = await self.call_tool("net.configure",
            interface=iface["name"], method="dhcp", timeout=10)

        if not result.success:
            # DHCP failed — use AI to determine static config
            config = await self.think(
                f"DHCP failed for interface {iface['name']}. "
                f"Interface info: {iface}. "
                f"Suggest a static IP configuration.",
                level="tactical"
            )
            # Parse and apply suggested config

    async def monitoring_loop(self):
        """Continuous network monitoring."""
        while True:
            # Check internet connectivity
            ping = await self.call_tool("net.ping", host="1.1.1.1", timeout=5)
            if not ping.success:
                await self.handle_connectivity_loss()

            # Check DNS resolution
            dns = await self.call_tool("net.dns_lookup", domain="api.anthropic.com")
            if not dns.success:
                await self.handle_dns_failure()

            # Monitor bandwidth for anomalies
            bw = await self.call_tool("net.bandwidth")
            if self.is_anomalous(bw.data):
                await self.investigate_traffic_anomaly(bw.data)

            await asyncio.sleep(30)
```

### Step 8.5: Implement WireGuard VPN Support

**Claude Code prompt**: "Add WireGuard VPN tools and configuration — the Network Agent should be able to set up and manage WireGuard tunnels"

### Step 8.6: Integration Test

**Claude Code prompt**: "Test: boot aiOS in QEMU with user-mode networking, verify Network Agent configures the interface, sets up DNS, applies firewall rules, and can make outbound HTTP requests"

---

## Deliverables Checklist

- [ ] All network tools implemented (net.configure, firewall.*, dns.*)
- [ ] unbound DNS resolver installed and configured
- [ ] nftables default ruleset applied at boot
- [ ] Network Agent auto-configures interfaces at boot
- [ ] Firewall in default-deny state with required rules
- [ ] DNS resolution works
- [ ] Connectivity monitoring loop running
- [ ] Offline detection works
- [ ] WireGuard VPN tools available
- [ ] Integration test passes in QEMU

---

## Next Phase
→ [Phase 9: Security](./09-SECURITY.md) (can be parallel with Phase 8)
