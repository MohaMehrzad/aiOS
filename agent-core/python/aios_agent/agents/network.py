"""
NetworkAgent — Manages network interfaces, DNS, connectivity, and firewall.

Capabilities:
  - Connectivity monitoring (ping, DNS, port checks)
  - Network interface listing
  - DNS lookups
  - Firewall rule management
  - Network diagnostics
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
            "net.interfaces",
            "net.ping",
            "net.dns",
            "net.port_scan",
            "firewall.rules",
            "firewall.add_rule",
            "firewall.delete_rule",
        ]

    # ------------------------------------------------------------------
    # Task dispatcher
    # ------------------------------------------------------------------

    async def handle_task(self, task: dict[str, Any]) -> dict[str, Any]:
        description = task.get("description", "").lower()
        input_data = task.get("input_json", {}) if isinstance(task.get("input_json"), dict) else {}

        if "connect" in description or "ping" in description or "reachab" in description:
            return await self._check_connectivity(input_data)
        if "dns" in description or "resolv" in description or "nameserver" in description:
            return await self._dns_lookup(input_data)
        if "list" in description and ("interface" in description or "nic" in description):
            return await self._list_interfaces()
        if "interface" in description:
            return await self._list_interfaces()
        if "diagnos" in description or "troubleshoot" in description:
            return await self._diagnose_network(input_data)
        if "firewall" in description or "iptable" in description or "nftable" in description:
            return await self._manage_firewall(input_data)
        if "port" in description and ("scan" in description or "check" in description):
            return await self._port_scan(input_data)

        # AI fallback
        decision = await self.think(
            f"Network task received: '{task.get('description', '')}'. "
            f"Options: check_connectivity, dns_lookup, list_interfaces, "
            f"diagnose, manage_firewall, port_scan. "
            f"Which action matches best? Reply with ONLY the action name.",
            level=IntelligenceLevel.OPERATIONAL,
        )

        action = decision.strip().lower()
        if "connect" in action or "ping" in action:
            return await self._check_connectivity(input_data)
        if "dns" in action:
            return await self._dns_lookup(input_data)
        if "firewall" in action:
            return await self._manage_firewall(input_data)
        if "diagnos" in action:
            return await self._diagnose_network(input_data)
        if "port" in action:
            return await self._port_scan(input_data)
        return await self._check_connectivity(input_data)

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
                "net.ping",
                {"host": target, "count": 3},
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
                "net.dns",
                {"hostname": domain},
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
                    "net.port_scan",
                    {"host": host, "port": port},
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
    # DNS lookup
    # ------------------------------------------------------------------

    async def _dns_lookup(self, params: dict[str, Any]) -> dict[str, Any]:
        """Perform DNS lookups."""
        domain = params.get("domain", "")
        if not domain:
            return {"success": False, "error": "No domain specified"}

        result = await self.call_tool(
            "net.dns",
            {"hostname": domain},
            reason=f"DNS lookup: {domain}",
        )
        return {
            "success": result.get("success", False),
            "domain": domain,
            "addresses": result.get("output", {}).get("addresses", []),
            "error": result.get("error", ""),
        }

    # ------------------------------------------------------------------
    # Interface listing
    # ------------------------------------------------------------------

    async def _list_interfaces(self) -> dict[str, Any]:
        """List all network interfaces and their current status."""
        result = await self.call_tool(
            "net.interfaces", {},
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
    # Port scanning
    # ------------------------------------------------------------------

    async def _port_scan(self, params: dict[str, Any]) -> dict[str, Any]:
        """Scan ports on a target host."""
        host = params.get("host", "localhost")
        ports = params.get("ports", [22, 80, 443, 8080, 9090])

        results: list[dict[str, Any]] = []
        for port in ports:
            result = await self.call_tool(
                "net.port_scan",
                {"host": host, "port": port},
                reason=f"Port scan: {host}:{port}",
            )
            open_status = False
            if result.get("success"):
                open_status = result.get("output", {}).get("open", False)
            results.append({"host": host, "port": port, "open": open_status})

        return {
            "success": True,
            "host": host,
            "ports_scanned": len(ports),
            "results": results,
            "open_ports": [r["port"] for r in results if r["open"]],
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

        # Step 2: Ping external target
        ext_ping = await self.call_tool(
            "net.ping", {"host": target, "count": 3},
            reason=f"Diagnostic: ping external {target}",
        )
        steps_performed.append({"step": "ping_external", "result": ext_ping})
        if not ext_ping.get("success") or not ext_ping.get("output", {}).get("received", 0):
            problems.append(f"External target {target} is unreachable")

        # Step 3: DNS resolution
        dns_check = await self.call_tool(
            "net.dns", {"hostname": "google.com"},
            reason="Diagnostic: DNS resolution test",
        )
        steps_performed.append({"step": "dns_resolve", "result": dns_check})
        if not dns_check.get("success"):
            problems.append("DNS resolution is failing")

        # Ask AI to summarise the diagnosis
        diagnosis_prompt = (
            f"Network diagnostic results:\n"
            f"Active interfaces: {len(active_interfaces)}\n"
            f"Problems found: {problems if problems else 'None'}\n\n"
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
                "firewall.add_rule",
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
                "firewall.delete_rule",
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
            "firewall.rules", {},
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
                self.task_poll_loop(),
                self._connectivity_loop(),
                self._shutdown_event.wait(),
            )
        finally:
            await self.unregister_from_orchestrator()
            await self._close_channels()
            self._running = False


if __name__ == "__main__":
    import os
    agent = NetworkAgent(agent_id=os.getenv("AIOS_AGENT_NAME", "network-agent"))
    asyncio.run(agent.run())
