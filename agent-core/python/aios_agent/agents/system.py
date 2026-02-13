"""
SystemAgent — Monitors system health, manages services, and collects metrics.

Capabilities:
  - CPU / RAM / disk monitoring via the ``monitor.cpu`` tool
  - Service health checking via ``service.status``
  - Restarting failed services via ``service.restart``
  - Continuous health-checking background loop
"""

from __future__ import annotations

import asyncio
import logging
import time
from typing import Any

from aios_agent.base import BaseAgent, IntelligenceLevel

logger = logging.getLogger("aios.agent.system")

# Threshold defaults
CPU_WARN_THRESHOLD = 85.0
CPU_CRIT_THRESHOLD = 95.0
MEM_WARN_THRESHOLD = 80.0
MEM_CRIT_THRESHOLD = 95.0
DISK_WARN_THRESHOLD = 85.0
DISK_CRIT_THRESHOLD = 95.0

HEALTH_CHECK_INTERVAL_S = 30.0


class SystemAgent(BaseAgent):
    """Agent responsible for overall system health and service management."""

    def get_agent_type(self) -> str:
        return "system"

    def get_capabilities(self) -> list[str]:
        return [
            "system.health_check",
            "monitor.cpu",
            "system.restart_service",
            "service.status",
            "process.list",
            "system.uptime",
        ]

    # ------------------------------------------------------------------
    # Task dispatcher
    # ------------------------------------------------------------------

    async def handle_task(self, task: dict[str, Any]) -> dict[str, Any]:
        description = task.get("description", "").lower()
        input_data = task.get("input_json", {}) if isinstance(task.get("input_json"), dict) else {}

        if "health" in description or "check" in description:
            return await self._check_health(input_data)
        if "restart" in description or "service" in description and "restart" in description:
            service_name = input_data.get("service", self._extract_service_name(description))
            return await self._restart_service(service_name)
        if "metric" in description or "cpu" in description or "ram" in description or "memory" in description:
            return await self._get_metrics(input_data)
        if "process" in description or "top" in description:
            return await self._list_processes(input_data)

        # If the task description is unclear, use AI to decide
        decision = await self.think(
            f"I received a system task: '{task.get('description', '')}'. "
            f"Available actions: check_health, restart_service, get_metrics, list_processes. "
            f"Which action best matches? Reply with ONLY the action name.",
            level=IntelligenceLevel.REACTIVE,
        )

        await self.store_decision(
            context=task.get("description", ""),
            options=["check_health", "restart_service", "get_metrics", "list_processes"],
            chosen=decision.strip(),
            reasoning="AI-dispatched based on task description",
            intelligence_level="reactive",
        )

        action = decision.strip().lower()
        if "restart" in action:
            service_name = input_data.get("service", "unknown")
            return await self._restart_service(service_name)
        if "metric" in action:
            return await self._get_metrics(input_data)
        if "process" in action:
            return await self._list_processes(input_data)
        return await self._check_health(input_data)

    # ------------------------------------------------------------------
    # Actions
    # ------------------------------------------------------------------

    async def _check_health(self, params: dict[str, Any]) -> dict[str, Any]:
        """Run a comprehensive health check on the system."""
        # Call individual monitor tools (each returns its own metric)
        cpu_result, mem_result, disk_result = await asyncio.gather(
            self.call_tool("monitor.cpu", {}, reason="Health check: CPU"),
            self.call_tool("monitor.memory", {}, reason="Health check: Memory"),
            self.call_tool("monitor.disk", {"path": "/"}, reason="Health check: Disk"),
            return_exceptions=True,
        )

        cpu_pct = 0.0
        mem_pct = 0.0
        disk_pct = 0.0

        if isinstance(cpu_result, dict) and cpu_result.get("success"):
            cpu_pct = cpu_result.get("output", {}).get("cpu_percent", 0.0)
        if isinstance(mem_result, dict) and mem_result.get("success"):
            mem_pct = mem_result.get("output", {}).get("used_percent", 0.0)
        if isinstance(disk_result, dict) and disk_result.get("success"):
            disk_pct = disk_result.get("output", {}).get("used_percent", 0.0)

        issues: list[dict[str, Any]] = []
        severity = "healthy"

        if cpu_pct >= CPU_CRIT_THRESHOLD:
            issues.append({"resource": "cpu", "value": cpu_pct, "severity": "critical"})
            severity = "critical"
        elif cpu_pct >= CPU_WARN_THRESHOLD:
            issues.append({"resource": "cpu", "value": cpu_pct, "severity": "warning"})
            if severity != "critical":
                severity = "warning"

        if mem_pct >= MEM_CRIT_THRESHOLD:
            issues.append({"resource": "memory", "value": mem_pct, "severity": "critical"})
            severity = "critical"
        elif mem_pct >= MEM_WARN_THRESHOLD:
            issues.append({"resource": "memory", "value": mem_pct, "severity": "warning"})
            if severity != "critical":
                severity = "warning"

        if disk_pct >= DISK_CRIT_THRESHOLD:
            issues.append({"resource": "disk", "value": disk_pct, "severity": "critical"})
            severity = "critical"
        elif disk_pct >= DISK_WARN_THRESHOLD:
            issues.append({"resource": "disk", "value": disk_pct, "severity": "warning"})
            if severity != "critical":
                severity = "warning"

        # Persist metrics in memory for trend analysis
        await self.update_metric("system.cpu_percent", cpu_pct)
        await self.update_metric("system.memory_percent", mem_pct)
        await self.update_metric("system.disk_percent", disk_pct)

        # Check services
        services_result = await self.call_tool(
            "service.status",
            {"all": True},
            reason="Health check — service enumeration",
        )
        failed_services: list[str] = []
        if services_result.get("success"):
            for svc in services_result.get("output", {}).get("services", []):
                if svc.get("status") in ("failed", "dead", "inactive"):
                    failed_services.append(svc.get("name", "unknown"))

        if failed_services:
            issues.append({
                "resource": "services",
                "value": failed_services,
                "severity": "warning",
            })
            if severity == "healthy":
                severity = "warning"

        # If critical, use AI to decide if we should auto-remediate
        recommended_actions: list[str] = []
        if severity == "critical":
            analysis = await self.think(
                f"System health is CRITICAL. Issues: {issues}. "
                f"Current metrics: CPU={cpu_pct}%, MEM={mem_pct}%, DISK={disk_pct}%. "
                f"Failed services: {failed_services}. "
                f"What immediate actions should I take? List up to 3 actions, one per line.",
                level=IntelligenceLevel.TACTICAL,
            )
            recommended_actions = [
                line.strip().lstrip("- ").lstrip("0123456789.)")
                for line in analysis.strip().split("\n")
                if line.strip()
            ][:3]

        # Push health event
        await self.push_event(
            "system.health",
            {
                "severity": severity,
                "cpu": cpu_pct,
                "memory": mem_pct,
                "disk": disk_pct,
                "issues_count": len(issues),
                "failed_services": failed_services,
            },
            critical=(severity == "critical"),
        )

        await self.store_memory("last_health_check", {
            "timestamp": int(time.time()),
            "severity": severity,
            "issues": issues,
        })

        return {
            "healthy": severity == "healthy",
            "severity": severity,
            "metrics": {
                "cpu_percent": cpu_pct,
                "memory_percent": mem_pct,
                "disk_percent": disk_pct,
            },
            "issues": issues,
            "failed_services": failed_services,
            "recommended_actions": recommended_actions,
        }

    async def _restart_service(self, service_name: str) -> dict[str, Any]:
        """Restart a system service."""
        if not service_name or service_name == "unknown":
            return {"success": False, "error": "No service name provided"}

        # Check current status first
        status_result = await self.call_tool(
            "service.status",
            {"service": service_name},
            reason=f"Pre-restart status check for {service_name}",
        )

        previous_status = "unknown"
        if status_result.get("success"):
            previous_status = status_result.get("output", {}).get("status", "unknown")

        # Determine if restart is safe
        if previous_status == "running":
            safety_check = await self.think(
                f"Service '{service_name}' is currently running (status: {previous_status}). "
                f"Should I restart it? Consider: is it a critical service? "
                f"What are the risks? Answer YES or NO with a brief reason.",
                level=IntelligenceLevel.OPERATIONAL,
            )
            if "no" in safety_check.lower()[:10]:
                return {
                    "success": False,
                    "service": service_name,
                    "action": "restart_skipped",
                    "reason": safety_check.strip(),
                    "previous_status": previous_status,
                }

        # Execute restart
        restart_result = await self.call_tool(
            "service.restart",
            {"service": service_name},
            reason=f"Restarting service {service_name} (was: {previous_status})",
        )

        if not restart_result.get("success"):
            await self.push_event(
                "service.restart_failed",
                {"service": service_name, "error": restart_result.get("error", "")},
                critical=True,
            )
            return {
                "success": False,
                "service": service_name,
                "error": restart_result.get("error", "Restart failed"),
                "previous_status": previous_status,
            }

        # Verify the service came back up
        await asyncio.sleep(2)
        verify_result = await self.call_tool(
            "service.status",
            {"service": service_name},
            reason=f"Post-restart verification for {service_name}",
        )
        new_status = "unknown"
        if verify_result.get("success"):
            new_status = verify_result.get("output", {}).get("status", "unknown")

        await self.push_event(
            "service.restarted",
            {
                "service": service_name,
                "previous_status": previous_status,
                "new_status": new_status,
            },
        )

        await self.store_memory(f"service_restart:{service_name}", {
            "timestamp": int(time.time()),
            "previous_status": previous_status,
            "new_status": new_status,
        })

        return {
            "success": new_status in ("running", "active"),
            "service": service_name,
            "previous_status": previous_status,
            "new_status": new_status,
            "execution_id": restart_result.get("execution_id", ""),
        }

    async def _get_metrics(self, params: dict[str, Any]) -> dict[str, Any]:
        """Collect and return current system metrics."""
        cpu_result, mem_result, disk_result = await asyncio.gather(
            self.call_tool("monitor.cpu", {}, reason="Metrics: CPU"),
            self.call_tool("monitor.memory", {}, reason="Metrics: Memory"),
            self.call_tool("monitor.disk", {"path": "/"}, reason="Metrics: Disk"),
            return_exceptions=True,
        )

        metrics: dict[str, Any] = {}

        if isinstance(cpu_result, dict) and cpu_result.get("success"):
            cpu_output = cpu_result.get("output", {})
            metrics["cpu_percent"] = cpu_output.get("cpu_percent", 0.0)
            await self.update_metric("system.cpu_percent", metrics["cpu_percent"])

        if isinstance(mem_result, dict) and mem_result.get("success"):
            mem_output = mem_result.get("output", {})
            metrics["memory_percent"] = mem_output.get("used_percent", 0.0)
            metrics["memory_used_mb"] = mem_output.get("used_mb", 0)
            metrics["memory_total_mb"] = mem_output.get("total_mb", 0)
            await self.update_metric("system.memory_percent", metrics["memory_percent"])

        if isinstance(disk_result, dict) and disk_result.get("success"):
            disk_output = disk_result.get("output", {})
            metrics["disk_percent"] = disk_output.get("used_percent", 0.0)
            metrics["disk_used_gb"] = disk_output.get("used_gb", 0)
            metrics["disk_total_gb"] = disk_output.get("total_gb", 0)
            await self.update_metric("system.disk_percent", metrics["disk_percent"])

        return {
            "success": True,
            "metrics": metrics,
            "timestamp": int(time.time()),
        }

    async def _list_processes(self, params: dict[str, Any]) -> dict[str, Any]:
        """List running processes sorted by resource usage."""
        sort_by = params.get("sort_by", "cpu")
        limit = params.get("limit", 20)

        result = await self.call_tool(
            "process.list",
            {"sort_by": sort_by, "limit": limit},
            reason="Process list request",
        )

        if not result.get("success"):
            return {"success": False, "error": result.get("error", "Failed to list processes")}

        processes = result.get("output", {}).get("processes", [])
        return {
            "success": True,
            "process_count": len(processes),
            "processes": processes,
            "sort_by": sort_by,
        }

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------

    @staticmethod
    def _extract_service_name(description: str) -> str:
        """Best-effort extraction of a service name from a natural-language description."""
        keywords = ["restart", "service", "start", "stop", "enable", "disable"]
        words = description.split()
        for i, word in enumerate(words):
            if word.lower() in keywords and i + 1 < len(words):
                candidate = words[i + 1].strip(".,;:'\"")
                if candidate and candidate.lower() not in keywords:
                    return candidate
        return "unknown"

    # ------------------------------------------------------------------
    # Background health loop
    # ------------------------------------------------------------------

    async def _health_check_loop(self) -> None:
        """Periodically run health checks in the background."""
        while not self._shutdown_event.is_set():
            try:
                health = await self._check_health({})
                if health.get("severity") == "critical":
                    logger.critical("System health CRITICAL: %s", health.get("issues"))
                    # Attempt auto-remediation for failed services
                    for svc in health.get("failed_services", []):
                        logger.warning("Auto-restarting failed service: %s", svc)
                        await self._restart_service(svc)
                elif health.get("severity") == "warning":
                    logger.warning("System health WARNING: %s", health.get("issues"))
            except Exception as exc:
                logger.error("Health check loop error: %s", exc)

            try:
                await asyncio.wait_for(
                    self._shutdown_event.wait(),
                    timeout=HEALTH_CHECK_INTERVAL_S,
                )
            except asyncio.TimeoutError:
                pass

    async def run(self) -> None:
        """Run the system agent with its background health-check loop."""
        self._running = True
        try:
            registered = await self.register_with_orchestrator()
            if not registered:
                logger.warning("Running without orchestrator registration")

            await asyncio.gather(
                self.heartbeat_loop(),
                self.task_poll_loop(),
                self._health_check_loop(),
                self._shutdown_event.wait(),
            )
        finally:
            await self.unregister_from_orchestrator()
            await self._close_channels()
            self._running = False
            logger.info("SystemAgent %s shut down", self.agent_id)


if __name__ == "__main__":
    import os
    agent = SystemAgent(agent_id=os.getenv("AIOS_AGENT_NAME", "system-agent"))
    asyncio.run(agent.run())
