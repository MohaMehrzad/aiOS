"""
NetworkAgent — Manages network interfaces, DNS, connectivity, and DHCP.

Capabilities:
  - Interface configuration (IP, netmask, gateway)
  - Connectivity monitoring (ping, traceroute, port checks)
  - DNS management (resolvers, records, cache flushing)
  - DHCP lease management
"""

from __future__ import annotations

import asyncio
import logging
import time
from typing import Any

from aios_agent.base import BaseAgent, IntelligenceLevel

logger = logging.getLogger("aios.agent.network")

CONNECTIVITY_CHECK_INTERVAL_S = 60.0
DEFAULT_PING_TARGETS = ["8.8.8.8", "1.1.1.1", "9.9.9.9"]
DEFAULT_DNS_TEST_DOMAINS = ["google.com", "cloudflare.com"]


class NetworkAgent(BaseAgent):
    """Agent responsible for network configuration and monitoring."""

    def get_agent_type(self) -> str:
        return "network"

    def get_capabilities(self) -> list[str]:
        return [
            "network.configure_interface",
            "network.check_connectivity",
            "network.manage_dns",
            "network.manage_dhcp",
            "network.list_interfaces",
            "network.diagnose",
            "network.firewall",
        ]

    # ------------------------------------------------------------------
    # Task dispatcher
    # ------------------------------------------------------------------

    async def handle_task(self, task: dict[str, Any]) -> dict[str, Any]:
        description = task.get("description", "").lower()
        input_data = task.get("input_json", {}) if isinstance(task.get("input_json"), dict) else {}

        if "interface" in description or "configure" in description or "ip" in description:
            return await self._configure_interface(input_data)
        if "connect" in description or "ping" in description or "reachab" in description:
            return await self._check_connectivity(input_data)
        if "dns" in description or "resolv" in description or "nameserver" in description:
            return await self._manage_dns(input_data)
        if "dhcp" in description or "lease" in description:
            return await self._manage_dhcp(input_data)
        if "list" in description and ("interface" in description or "nic" in description):
            return await self._list_interfaces()
        if "diagnos" in description or "troubleshoot" in description:
            return await self._diagnose_network(input_data)
        if "firewall" in description or "iptable" in description or "nftable" in description:
            return await self._manage_firewall(input_data)

        # AI fallback
        decision = await self.think(
            f"Network task received: '{task.get('description', '')}'. "
            f"Options: configure_interface, check_connectivity, manage_dns, manage_dhcp, "
            f"list_interfaces, diagnose, manage_firewall. "
            f"Which action matches best? Reply with ONLY the action name.",
            level=IntelligenceLevel.REACTIVE,
        )

        action = decision.strip().lower()
        if "interface" in action and "list" not in action:
            return await self._configure_interface(input_data)
        if "connect" in action or "ping" in action:
            return await self._check_connectivity(input_data)
        if "dns" in action:
            return await self._manage_dns(input_data)
        if "dhcp" in action:
            return await self._manage_dhcp(input_data)
        if "firewall" in action:
            return await self._manage_firewall(input_data)
        if "diagnos" in action:
            return await self._diagnose_network(input_data)
        return await self._check_connectivity(input_data)

    # ------------------------------------------------------------------
    # Interface configuration
    # ------------------------------------------------------------------

    async def _configure_interface(self, params: dict[str, Any]) -> dict[str, Any]:
        """Configure a network interface with IP address settings."""
        interface = params.get("interface", "")
        ip_address = params.get("ip_address", "")
        netmask = params.get("netmask", "255.255.255.0")
        gateway = params.get("gateway", "")
        action = params.get("action", "configure")  # configure | up | down

        if not interface:
            interfaces_result = await self.call_tool(
                "network.list_interfaces", {},
                reason="No interface specified — listing available interfaces",
            )
            available = interfaces_result.get("output", {}).get("interfaces", [])
            return {
                "success": False,
                "error": "No interface specified",
                "available_interfaces": [iface.get("name", "") for iface in available],
            }

        if action in ("up", "down"):
            result = await self.call_tool(
                "network.interface_control",
                {"interface": interface, "action": action},
                reason=f"Bringing interface {interface} {action}",
            )
            await self.push_event(
                "network.interface_changed",
                {"interface": interface, "action": action, "success": result.get("success", False)},
            )
            return {
                "success": result.get("success", False),
                "interface": interface,
                "action": action,
                "error": result.get("error", ""),
            }

        if not ip_address:
            return {"success": False, "error": "No IP address provided for configuration"}

        # Safety check before modifying interface
        safety = await self.think(
            f"I am about to configure interface '{interface}' with IP {ip_address}/{netmask}, "
            f"gateway {gateway}. Is this a valid and safe configuration? "
            f"Check for RFC compliance and potential conflicts. Answer YES or NO with reason.",
            level=IntelligenceLevel.OPERATIONAL,
        )

        if "no" in safety.lower()[:10]:
            return {
                "success": False,
                "error": f"Configuration rejected by safety check: {safety.strip()}",
                "interface": interface,
            }

        result = await self.call_tool(
            "network.configure_interface",
            {
                "interface": interface,
                "ip_address": ip_address,
                "netmask": netmask,
                "gateway": gateway,
            },
            reason=f"Configuring {interface}: {ip_address}/{netmask} gw {gateway}",
        )

        if result.get("success"):
            await self.store_memory(f"interface_config:{interface}", {
                "ip_address": ip_address,
                "netmask": netmask,
                "gateway": gateway,
                "configured_at": int(time.time()),
            })
            await self.push_event(
                "network.interface_configured",
                {"interface": interface, "ip": ip_address, "gateway": gateway},
            )

        return {
            "success": result.get("success", False),
            "interface": interface,
            "ip_address": ip_address,
            "netmask": netmask,
            "gateway": gateway,
            "error": result.get("error", ""),
            "execution_id": result.get("execution_id", ""),
        }

    # ------------------------------------------------------------------
    # Connectivity checking
    # ------------------------------------------------------------------

    async def _check_connectivity(self, params: dict[str, Any]) -> dict[str, Any]:
        """Check network connectivity to multiple targets."""
        targets = params.get("targets", DEFAULT_PING_TARGETS)
        dns_domains = params.get("dns_domains", DEFAULT_DNS_TEST_DOMAINS)
        port_checks = params.get("port_checks", [])  # [{"host": "x", "port": 80}]

        results: dict[str, Any] = {
            "ping": {},
            "dns": {},
            "port": {},
        }
        overall_healthy = True

        # Ping tests
        ping_tasks = []
        for target in targets:
            ping_tasks.append(self.call_tool(
                "network.ping",
                {"target": target, "count": 3, "timeout_s": 5},
                reason=f"Connectivity check: ping {target}",
            ))

        ping_results = await asyncio.gather(*ping_tasks, return_exceptions=True)
        for target, result in zip(targets, ping_results):
            if isinstance(result, Exception):
                results["ping"][target] = {"reachable": False, "error": str(result)}
                overall_healthy = False
            elif not result.get("success"):
                results["ping"][target] = {"reachable": False, "error": result.get("error", "")}
                overall_healthy = False
            else:
                output = result.get("output", {})
                results["ping"][target] = {
                    "reachable": True,
                    "rtt_ms": output.get("avg_rtt_ms", 0.0),
                    "packet_loss": output.get("packet_loss_pct", 0.0),
                }
                if output.get("packet_loss_pct", 0.0) > 50:
                    overall_healthy = False

        # DNS resolution tests
        dns_tasks = []
        for domain in dns_domains:
            dns_tasks.append(self.call_tool(
                "network.dns_resolve",
                {"domain": domain},
                reason=f"Connectivity check: DNS resolve {domain}",
            ))

        dns_results = await asyncio.gather(*dns_tasks, return_exceptions=True)
        for domain, result in zip(dns_domains, dns_results):
            if isinstance(result, Exception):
                results["dns"][domain] = {"resolved": False, "error": str(result)}
                overall_healthy = False
            elif not result.get("success"):
                results["dns"][domain] = {"resolved": False, "error": result.get("error", "")}
                overall_healthy = False
            else:
                output = result.get("output", {})
                results["dns"][domain] = {
                    "resolved": True,
                    "addresses": output.get("addresses", []),
                    "ttl": output.get("ttl", 0),
                }

        # Port checks
        for check in port_checks:
            host = check.get("host", "")
            port = check.get("port", 0)
            if host and port:
                port_result = await self.call_tool(
                    "network.port_check",
                    {"host": host, "port": port, "timeout_s": 5},
                    reason=f"Connectivity check: port {host}:{port}",
                )
                if isinstance(port_result, dict) and port_result.get("success"):
                    port_open = port_result.get("output", {}).get("open", False)
                    results["port"][f"{host}:{port}"] = {"open": port_open}
                else:
                    results["port"][f"{host}:{port}"] = {"open": False}
                    overall_healthy = False

        await self.update_metric("network.connectivity_healthy", 1.0 if overall_healthy else 0.0)

        if not overall_healthy:
            await self.push_event(
                "network.connectivity_issue",
                {"results": results},
                critical=True,
            )

        return {
            "healthy": overall_healthy,
            "results": results,
            "timestamp": int(time.time()),
        }

    # ------------------------------------------------------------------
    # DNS management
    # ------------------------------------------------------------------

    async def _manage_dns(self, params: dict[str, Any]) -> dict[str, Any]:
        """Manage DNS configuration: set resolvers, flush cache, add records."""
        action = params.get("action", "status")
        resolvers = params.get("resolvers", [])
        domain = params.get("domain", "")
        record_type = params.get("record_type", "A")
        record_value = params.get("record_value", "")

        if action == "set_resolvers" and resolvers:
            result = await self.call_tool(
                "network.dns_set_resolvers",
                {"resolvers": resolvers},
                reason=f"Setting DNS resolvers to {resolvers}",
            )
            if result.get("success"):
                await self.store_memory("dns_resolvers", {
                    "resolvers": resolvers,
                    "set_at": int(time.time()),
                })
            return {
                "success": result.get("success", False),
                "action": "set_resolvers",
                "resolvers": resolvers,
                "error": result.get("error", ""),
            }

        if action == "flush_cache":
            result = await self.call_tool(
                "network.dns_flush_cache", {},
                reason="Flushing DNS cache",
            )
            return {
                "success": result.get("success", False),
                "action": "flush_cache",
                "error": result.get("error", ""),
            }

        if action == "add_record" and domain and record_value:
            result = await self.call_tool(
                "network.dns_add_record",
                {"domain": domain, "type": record_type, "value": record_value},
                reason=f"Adding DNS record: {domain} {record_type} {record_value}",
            )
            return {
                "success": result.get("success", False),
                "action": "add_record",
                "domain": domain,
                "record_type": record_type,
                "record_value": record_value,
                "error": result.get("error", ""),
            }

        if action == "resolve" and domain:
            result = await self.call_tool(
                "network.dns_resolve",
                {"domain": domain, "type": record_type},
                reason=f"Resolving {domain} ({record_type})",
            )
            return {
                "success": result.get("success", False),
                "action": "resolve",
                "domain": domain,
                "addresses": result.get("output", {}).get("addresses", []),
                "error": result.get("error", ""),
            }

        # Default: get current DNS status
        result = await self.call_tool(
            "network.dns_status", {},
            reason="Querying DNS status",
        )
        return {
            "success": result.get("success", False),
            "action": "status",
            "dns_config": result.get("output", {}),
            "error": result.get("error", ""),
        }

    # ------------------------------------------------------------------
    # DHCP management
    # ------------------------------------------------------------------

    async def _manage_dhcp(self, params: dict[str, Any]) -> dict[str, Any]:
        """Manage DHCP: request lease, release, renew, show leases."""
        action = params.get("action", "status")
        interface = params.get("interface", "")

        if action == "request" and interface:
            result = await self.call_tool(
                "network.dhcp_request",
                {"interface": interface},
                reason=f"Requesting DHCP lease on {interface}",
            )
            if result.get("success"):
                lease_info = result.get("output", {})
                await self.store_memory(f"dhcp_lease:{interface}", {
                    "ip": lease_info.get("ip_address", ""),
                    "lease_time": lease_info.get("lease_time_s", 0),
                    "obtained_at": int(time.time()),
                })
            return {
                "success": result.get("success", False),
                "action": "request",
                "interface": interface,
                "lease": result.get("output", {}),
                "error": result.get("error", ""),
            }

        if action == "release" and interface:
            result = await self.call_tool(
                "network.dhcp_release",
                {"interface": interface},
                reason=f"Releasing DHCP lease on {interface}",
            )
            return {
                "success": result.get("success", False),
                "action": "release",
                "interface": interface,
                "error": result.get("error", ""),
            }

        if action == "renew" and interface:
            result = await self.call_tool(
                "network.dhcp_renew",
                {"interface": interface},
                reason=f"Renewing DHCP lease on {interface}",
            )
            return {
                "success": result.get("success", False),
                "action": "renew",
                "interface": interface,
                "lease": result.get("output", {}),
                "error": result.get("error", ""),
            }

        # Default: show DHCP leases
        result = await self.call_tool(
            "network.dhcp_status", {},
            reason="Listing DHCP leases",
        )
        return {
            "success": result.get("success", False),
            "action": "status",
            "leases": result.get("output", {}).get("leases", []),
            "error": result.get("error", ""),
        }

    # ------------------------------------------------------------------
    # Interface listing
    # ------------------------------------------------------------------

    async def _list_interfaces(self) -> dict[str, Any]:
        """List all network interfaces and their current status."""
        result = await self.call_tool(
            "network.list_interfaces", {},
            reason="Listing network interfaces",
        )

        if not result.get("success"):
            return {"success": False, "error": result.get("error", "Failed to list interfaces")}

        interfaces = result.get("output", {}).get("interfaces", [])
        return {
            "success": True,
            "interface_count": len(interfaces),
            "interfaces": interfaces,
        }

    # ------------------------------------------------------------------
    # Network diagnosis
    # ------------------------------------------------------------------

    async def _diagnose_network(self, params: dict[str, Any]) -> dict[str, Any]:
        """Run a comprehensive network diagnostic."""
        target = params.get("target", "8.8.8.8")
        problems: list[str] = []
        steps_performed: list[dict[str, Any]] = []

        # Step 1: Check interfaces
        iface_result = await self._list_interfaces()
        steps_performed.append({"step": "list_interfaces", "result": iface_result})
        active_interfaces = [
            i for i in iface_result.get("interfaces", [])
            if i.get("status") in ("up", "active")
        ]
        if not active_interfaces:
            problems.append("No active network interfaces found")

        # Step 2: Ping gateway
        gw_config = await self.recall_memory("interface_config:eth0")
        gateway = params.get("gateway", "")
        if not gateway and isinstance(gw_config, dict):
            gateway = gw_config.get("gateway", "")
        if gateway:
            gw_ping = await self.call_tool(
                "network.ping", {"target": gateway, "count": 3, "timeout_s": 5},
                reason=f"Diagnostic: ping gateway {gateway}",
            )
            steps_performed.append({"step": "ping_gateway", "result": gw_ping})
            if not gw_ping.get("success") or not gw_ping.get("output", {}).get("received", 0):
                problems.append(f"Gateway {gateway} is unreachable")

        # Step 3: Ping external target
        ext_ping = await self.call_tool(
            "network.ping", {"target": target, "count": 3, "timeout_s": 5},
            reason=f"Diagnostic: ping external {target}",
        )
        steps_performed.append({"step": "ping_external", "result": ext_ping})
        if not ext_ping.get("success") or not ext_ping.get("output", {}).get("received", 0):
            problems.append(f"External target {target} is unreachable")

        # Step 4: DNS resolution
        dns_check = await self.call_tool(
            "network.dns_resolve", {"domain": "google.com"},
            reason="Diagnostic: DNS resolution test",
        )
        steps_performed.append({"step": "dns_resolve", "result": dns_check})
        if not dns_check.get("success"):
            problems.append("DNS resolution is failing")

        # Step 5: Traceroute
        trace_result = await self.call_tool(
            "network.traceroute", {"target": target, "max_hops": 15},
            reason=f"Diagnostic: traceroute to {target}",
        )
        steps_performed.append({"step": "traceroute", "result": trace_result})

        # Ask AI to summarise the diagnosis
        diagnosis_prompt = (
            f"Network diagnostic results:\n"
            f"Active interfaces: {len(active_interfaces)}\n"
            f"Problems found: {problems if problems else 'None'}\n"
            f"Traceroute hops: {trace_result.get('output', {}).get('hops', [])}\n\n"
            f"Provide a brief diagnosis and recommended fix (2-3 sentences)."
        )
        analysis = await self.think(diagnosis_prompt, level=IntelligenceLevel.OPERATIONAL)

        return {
            "healthy": len(problems) == 0,
            "problems": problems,
            "diagnosis": analysis.strip(),
            "steps_performed": len(steps_performed),
            "active_interfaces": len(active_interfaces),
        }

    # ------------------------------------------------------------------
    # Firewall management
    # ------------------------------------------------------------------

    async def _manage_firewall(self, params: dict[str, Any]) -> dict[str, Any]:
        """Manage firewall rules."""
        action = params.get("action", "status")
        rule = params.get("rule", {})

        if action == "add_rule" and rule:
            # Safety check on firewall rules
            safety = await self.think(
                f"A firewall rule is being added: {rule}. "
                f"Is this safe? Could it lock us out of the system? Answer YES or NO.",
                level=IntelligenceLevel.OPERATIONAL,
            )
            if "no" in safety.lower()[:10]:
                return {
                    "success": False,
                    "error": f"Firewall rule rejected by safety check: {safety.strip()}",
                }

            result = await self.call_tool(
                "network.firewall_add_rule",
                {"rule": rule},
                reason=f"Adding firewall rule: {rule}",
            )
            return {
                "success": result.get("success", False),
                "action": "add_rule",
                "rule": rule,
                "error": result.get("error", ""),
            }

        if action == "remove_rule" and rule:
            result = await self.call_tool(
                "network.firewall_remove_rule",
                {"rule": rule},
                reason=f"Removing firewall rule: {rule}",
            )
            return {
                "success": result.get("success", False),
                "action": "remove_rule",
                "error": result.get("error", ""),
            }

        # Default: show current rules
        result = await self.call_tool(
            "network.firewall_status", {},
            reason="Listing firewall rules",
        )
        return {
            "success": result.get("success", False),
            "action": "status",
            "rules": result.get("output", {}).get("rules", []),
            "error": result.get("error", ""),
        }

    # ------------------------------------------------------------------
    # Background connectivity loop
    # ------------------------------------------------------------------

    async def _connectivity_loop(self) -> None:
        """Periodically check connectivity in the background."""
        while not self._shutdown_event.is_set():
            try:
                result = await self._check_connectivity({})
                if not result.get("healthy"):
                    logger.warning("Connectivity issue detected — running diagnosis")
                    diag = await self._diagnose_network({})
                    logger.warning("Diagnosis: %s", diag.get("diagnosis", "unknown"))
            except Exception as exc:
                logger.error("Connectivity loop error: %s", exc)

            try:
                await asyncio.wait_for(
                    self._shutdown_event.wait(),
                    timeout=CONNECTIVITY_CHECK_INTERVAL_S,
                )
            except asyncio.TimeoutError:
                pass

    async def run(self) -> None:
        self._running = True
        try:
            await self.register_with_orchestrator()
            await asyncio.gather(
                self.heartbeat_loop(),
                self._connectivity_loop(),
                self._shutdown_event.wait(),
            )
        finally:
            await self.unregister_from_orchestrator()
            await self._close_channels()
            self._running = False
