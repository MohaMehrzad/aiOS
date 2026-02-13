"""
SecurityAgent — Intrusion detection, vulnerability scanning, and audit monitoring.

Capabilities:
  - Intrusion detection (suspicious connections, processes, rootkits)
  - Security scanning (open ports, SUID binaries, file permissions)
  - File integrity checking
  - Audit log monitoring for suspicious activity
  - Threat analysis
"""

from __future__ import annotations

import asyncio
import json
import logging
import time
from typing import Any

from aios_agent.base import BaseAgent, IntelligenceLevel

logger = logging.getLogger("aios.agent.security")

IDS_CHECK_INTERVAL_S = 45.0
AUDIT_CHECK_INTERVAL_S = 120.0
SEVERITY_WEIGHTS = {"critical": 10, "high": 7, "medium": 4, "low": 1, "info": 0}


class SecurityAgent(BaseAgent):
    """Agent responsible for system security: IDS, scanning, and monitoring."""

    def get_agent_type(self) -> str:
        return "security"

    def get_capabilities(self) -> list[str]:
        return [
            "sec.scan",
            "sec.check_perms",
            "sec.file_integrity",
            "sec.scan_rootkits",
            "sec.audit",
            "monitor.logs",
            "net.port_scan",
            "process.list",
        ]

    # ------------------------------------------------------------------
    # Task dispatcher
    # ------------------------------------------------------------------

    async def handle_task(self, task: dict[str, Any]) -> dict[str, Any]:
        description = task.get("description", "").lower()
        input_data = task.get("input_json", {}) if isinstance(task.get("input_json"), dict) else {}

        if "vulnerabilit" in description or "scan" in description or "cve" in description:
            return await self._scan_vulnerabilities(input_data)
        if "integrity" in description or "checksum" in description or "hash" in description:
            return await self._check_integrity(input_data)
        if "audit" in description or "log" in description:
            return await self._audit_logs(input_data)
        if "intrusion" in description or "ids" in description or "detect" in description:
            return await self._intrusion_check(input_data)
        if "threat" in description or "analys" in description:
            return await self._threat_analysis(input_data)
        if "permission" in description or "perm" in description:
            return await self._check_permissions(input_data)

        decision = await self.think(
            f"Security task: '{task.get('description', '')}'. "
            f"Options: scan_vulnerabilities, check_integrity, "
            f"audit_logs, intrusion_check, threat_analysis, check_permissions. "
            f"Which action? Reply with ONLY the action name.",
            level=IntelligenceLevel.OPERATIONAL,
        )

        action = decision.strip().lower()
        if "vuln" in action or "scan" in action:
            return await self._scan_vulnerabilities(input_data)
        if "integr" in action:
            return await self._check_integrity(input_data)
        if "audit" in action:
            return await self._audit_logs(input_data)
        if "threat" in action:
            return await self._threat_analysis(input_data)
        if "perm" in action:
            return await self._check_permissions(input_data)
        return await self._intrusion_check(input_data)

    # ------------------------------------------------------------------
    # Vulnerability scanning
    # ------------------------------------------------------------------

    async def _scan_vulnerabilities(self, params: dict[str, Any]) -> dict[str, Any]:
        """Scan for security issues: open ports, file permissions, SUID binaries."""
        scope = params.get("scope", "full")  # full | ports | permissions
        targets = params.get("targets", [])

        findings: list[dict[str, Any]] = []
        scan_results: dict[str, dict[str, Any]] = {}

        # Security scan (ports, SUID, weak permissions)
        if scope in ("full", "permissions"):
            scan_result = await self.call_tool(
                "sec.scan",
                {},
                reason="Vulnerability scan: security scan",
            )
            scan_results["security_scan"] = scan_result
            if scan_result.get("success"):
                for issue in scan_result.get("output", {}).get("findings", []):
                    findings.append({
                        "source": "security_scan",
                        "severity": issue.get("severity", "medium"),
                        "description": issue.get("description", ""),
                        "detail": issue.get("detail", ""),
                    })

        # Port scan — check common ports
        if scope in ("full", "ports"):
            common_ports = [22, 80, 443, 8080, 8081, 9090, 50051, 50052, 50053, 50054, 50055]
            port_tasks = [
                self.call_tool(
                    "net.port_scan",
                    {"host": "localhost", "port": p},
                    reason=f"Vulnerability scan: port {p}",
                )
                for p in common_ports
            ]
            port_results = await asyncio.gather(*port_tasks, return_exceptions=True)
            for port_num, result in zip(common_ports, port_results):
                if isinstance(result, dict) and result.get("success"):
                    is_open = result.get("output", {}).get("open", False)
                    if is_open:
                        findings.append({
                            "source": "port_scan",
                            "severity": "info",
                            "description": f"Open port: {port_num}/tcp",
                            "detail": "open",
                        })

        # Permission check
        if scope in ("full", "permissions"):
            perm_result = await self.call_tool(
                "sec.check_perms",
                {"path": "/etc"},
                reason="Vulnerability scan: file permissions",
            )
            scan_results["perm_check"] = perm_result
            if perm_result.get("success"):
                for issue in perm_result.get("output", {}).get("issues", []):
                    findings.append({
                        "source": "permissions",
                        "severity": issue.get("severity", "low"),
                        "description": issue.get("description", ""),
                        "detail": issue.get("path", ""),
                    })

        # Sort by severity
        findings.sort(
            key=lambda f: SEVERITY_WEIGHTS.get(f.get("severity", "info"), 0),
            reverse=True,
        )

        # Calculate risk score
        risk_score = sum(
            SEVERITY_WEIGHTS.get(f.get("severity", "info"), 0) for f in findings
        )
        max_possible = len(findings) * 10 if findings else 1
        risk_percent = min(100.0, (risk_score / max_possible) * 100)

        # AI analysis for critical findings
        critical_findings = [f for f in findings if f.get("severity") in ("critical", "high")]
        recommendations: list[str] = []
        if critical_findings:
            analysis = await self.think(
                f"Security scan found {len(critical_findings)} critical/high issues:\n"
                + "\n".join(
                    f"- [{f.get('severity')}] {f.get('description', '')[:100]}"
                    for f in critical_findings[:10]
                )
                + "\n\nProvide prioritised remediation steps (one per line, max 5).",
                level=IntelligenceLevel.TACTICAL,
            )
            recommendations = [
                line.strip().lstrip("- 0123456789.)")
                for line in analysis.strip().split("\n")
                if line.strip()
            ][:5]

        await self.push_event(
            "security.scan_complete",
            {
                "scope": scope,
                "total_findings": len(findings),
                "critical": len([f for f in findings if f["severity"] == "critical"]),
                "high": len([f for f in findings if f["severity"] == "high"]),
                "risk_percent": risk_percent,
            },
            critical=any(f["severity"] == "critical" for f in findings),
        )

        await self.store_memory("last_vuln_scan", {
            "timestamp": int(time.time()),
            "total_findings": len(findings),
            "risk_percent": risk_percent,
        })

        return {
            "success": True,
            "scope": scope,
            "total_findings": len(findings),
            "risk_score": risk_score,
            "risk_percent": round(risk_percent, 1),
            "findings": findings,
            "recommendations": recommendations,
            "scan_results": {k: {"success": v.get("success", False)} for k, v in scan_results.items()},
        }

    # ------------------------------------------------------------------
    # File integrity checking
    # ------------------------------------------------------------------

    async def _check_integrity(self, params: dict[str, Any]) -> dict[str, Any]:
        """Check integrity of critical system files."""
        paths = params.get("paths", [
            "/etc/passwd", "/etc/shadow", "/etc/group",
            "/etc/sudoers", "/etc/ssh/sshd_config",
        ])

        # Compute current hashes
        hash_result = await self.call_tool(
            "sec.file_integrity",
            {"mode": "check", "paths": paths},
            reason="Computing file integrity hashes",
        )

        if not hash_result.get("success"):
            return {"success": False, "error": hash_result.get("error", "Failed to compute hashes")}

        current_hashes: dict[str, str] = hash_result.get("output", {}).get("hashes", {})

        # Load baseline from memory
        baseline_key = "integrity_baseline"
        baseline = await self.recall_memory(baseline_key)
        changes: list[dict[str, str]] = []

        if baseline and isinstance(baseline, dict):
            stored_hashes = baseline.get("hashes", {})
            for path, current_hash in current_hashes.items():
                old_hash = stored_hashes.get(path)
                if old_hash is None:
                    changes.append({"path": path, "type": "new", "hash": current_hash})
                elif old_hash != current_hash:
                    changes.append({
                        "path": path,
                        "type": "modified",
                        "old_hash": old_hash,
                        "new_hash": current_hash,
                    })
            for path in stored_hashes:
                if path not in current_hashes:
                    changes.append({"path": path, "type": "deleted"})
        else:
            logger.info("No integrity baseline found — creating initial baseline")

        # Update baseline
        await self.store_memory(baseline_key, {
            "hashes": current_hashes,
            "updated_at": int(time.time()),
        })

        if changes:
            analysis = await self.think(
                f"File integrity check found {len(changes)} changes:\n"
                + "\n".join(f"- {c['path']}: {c['type']}" for c in changes)
                + "\n\nAre any of these suspicious? Which need investigation? "
                f"Reply with a brief risk assessment.",
                level=IntelligenceLevel.TACTICAL,
            )

            await self.push_event(
                "security.integrity_changes",
                {"changes": changes, "count": len(changes)},
                critical=any(
                    c.get("path", "").startswith(("/etc/shadow", "/etc/sudoers"))
                    for c in changes
                    if c["type"] == "modified"
                ),
            )
        else:
            analysis = "No changes detected. All files match baseline."

        return {
            "success": True,
            "files_checked": len(current_hashes),
            "changes_detected": len(changes),
            "changes": changes,
            "analysis": analysis.strip(),
            "baseline_existed": baseline is not None,
        }

    # ------------------------------------------------------------------
    # Permission checking
    # ------------------------------------------------------------------

    async def _check_permissions(self, params: dict[str, Any]) -> dict[str, Any]:
        """Check file and directory permissions for security issues."""
        result = await self.call_tool(
            "sec.check_perms",
            {"path": "/etc"},
            reason="Security permission check",
        )

        if not result.get("success"):
            return {"success": False, "error": result.get("error", "Permission check failed")}

        issues = result.get("output", {}).get("issues", [])
        return {
            "success": True,
            "issues_found": len(issues),
            "issues": issues,
        }

    # ------------------------------------------------------------------
    # Audit log monitoring
    # ------------------------------------------------------------------

    async def _audit_logs(self, params: dict[str, Any]) -> dict[str, Any]:
        """Monitor and analyse system logs for suspicious activity."""
        timeframe_minutes = params.get("timeframe_minutes", 60)
        log_sources = params.get("sources", ["auth", "syslog", "kernel"])

        all_events: list[dict[str, Any]] = []
        for source in log_sources:
            result = await self.call_tool(
                "monitor.logs",
                {"service": source, "lines": 200},
                reason=f"Reading logs: {source}",
            )
            if result.get("success"):
                events = result.get("output", {}).get("events", [])
                for event in events:
                    event["log_source"] = source
                all_events.extend(events)

        # Classify events
        suspicious: list[dict[str, Any]] = []
        failed_logins: list[dict[str, Any]] = []
        privilege_escalations: list[dict[str, Any]] = []

        for event in all_events:
            etype = event.get("type", "").lower()
            msg = event.get("message", "").lower()

            if "failed" in msg and ("login" in msg or "auth" in msg or "password" in msg):
                failed_logins.append(event)
            if "sudo" in msg or "su " in msg or "privilege" in msg or "root" in etype:
                privilege_escalations.append(event)
            if any(kw in msg for kw in ["denied", "violation", "unauthorized", "intrusion", "blocked"]):
                suspicious.append(event)

        # Threshold-based alerting
        alerts: list[dict[str, Any]] = []
        if len(failed_logins) > 10:
            alerts.append({
                "type": "brute_force_suspect",
                "severity": "high",
                "message": f"{len(failed_logins)} failed login attempts in {timeframe_minutes}m",
                "count": len(failed_logins),
            })
        if len(privilege_escalations) > 5:
            alerts.append({
                "type": "privilege_escalation",
                "severity": "medium",
                "message": f"{len(privilege_escalations)} privilege escalation events",
                "count": len(privilege_escalations),
            })

        # AI analysis if suspicious events found
        analysis = ""
        if suspicious or alerts:
            analysis_text = await self.think(
                f"Audit log analysis:\n"
                f"- Total events: {len(all_events)}\n"
                f"- Failed logins: {len(failed_logins)}\n"
                f"- Privilege escalations: {len(privilege_escalations)}\n"
                f"- Suspicious events: {len(suspicious)}\n"
                f"- Alerts raised: {len(alerts)}\n\n"
                f"Sample suspicious events:\n"
                + "\n".join(f"  - {e.get('message', '')[:150]}" for e in suspicious[:5])
                + "\n\nProvide a security assessment. Is immediate action needed?",
                level=IntelligenceLevel.TACTICAL,
            )
            analysis = analysis_text.strip()

        if alerts:
            await self.push_event(
                "security.audit_alerts",
                {"alerts": alerts, "suspicious_count": len(suspicious)},
                critical=any(a["severity"] in ("critical", "high") for a in alerts),
            )

        return {
            "success": True,
            "timeframe_minutes": timeframe_minutes,
            "total_events": len(all_events),
            "failed_logins": len(failed_logins),
            "privilege_escalations": len(privilege_escalations),
            "suspicious_events": len(suspicious),
            "alerts": alerts,
            "analysis": analysis,
        }

    # ------------------------------------------------------------------
    # Intrusion detection
    # ------------------------------------------------------------------

    async def _intrusion_check(self, params: dict[str, Any]) -> dict[str, Any]:
        """Run an intrusion detection check."""
        # Check for unexpected network connections via port scan
        ids_ports = [22, 80, 443, 3306, 5432, 6379, 8080, 8443, 9090]
        port_tasks = [
            self.call_tool(
                "net.port_scan",
                {"host": "localhost", "port": p},
                reason=f"IDS: check port {p}",
            )
            for p in ids_ports
        ]
        port_results = await asyncio.gather(*port_tasks, return_exceptions=True)
        open_ports: list[dict[str, Any]] = []
        for port_num, result in zip(ids_ports, port_results):
            if isinstance(result, dict) and result.get("success"):
                if result.get("output", {}).get("open", False):
                    open_ports.append({"port": port_num, "proto": "tcp"})

        # Check for unexpected processes
        proc_result = await self.call_tool(
            "process.list",
            {},
            reason="IDS: checking running processes",
        )
        processes: list[dict[str, Any]] = []
        if proc_result.get("success"):
            processes = proc_result.get("output", {}).get("processes", [])

        # Check for rootkits
        rootkit_result = await self.call_tool(
            "sec.scan_rootkits", {},
            reason="IDS: rootkit scan",
        )
        rootkit_findings: list[dict[str, Any]] = []
        if rootkit_result.get("success"):
            rootkit_findings = rootkit_result.get("output", {}).get("findings", [])

        threat_level = "clean"
        if rootkit_findings:
            threat_level = "critical"
        elif len(open_ports) > 20:
            threat_level = "suspicious"

        analysis = "No threats detected. System appears clean."
        if threat_level != "clean":
            analysis_text = await self.think(
                f"IDS check results:\n"
                f"- Open ports: {len(open_ports)}\n"
                f"- Running processes: {len(processes)}\n"
                f"- Rootkit findings: {len(rootkit_findings)}\n\n"
                f"Ports: {json.dumps(open_ports[:10], default=str)}\n"
                f"Rootkits: {json.dumps(rootkit_findings[:5], default=str)}\n\n"
                f"Assess the threat and recommend immediate actions.",
                level=IntelligenceLevel.TACTICAL,
            )
            analysis = analysis_text.strip()

            await self.push_event(
                "security.intrusion_detected",
                {
                    "threat_level": threat_level,
                    "open_ports": len(open_ports),
                    "processes": len(processes),
                    "rootkits": len(rootkit_findings),
                },
                critical=(threat_level == "critical"),
            )

        return {
            "threat_level": threat_level,
            "open_ports": open_ports,
            "process_count": len(processes),
            "rootkit_findings": rootkit_findings,
            "analysis": analysis.strip(),
        }

    # ------------------------------------------------------------------
    # Threat analysis
    # ------------------------------------------------------------------

    async def _threat_analysis(self, params: dict[str, Any]) -> dict[str, Any]:
        """Perform a comprehensive threat analysis combining multiple data sources."""
        # Gather data from multiple sources
        vuln_data = await self._scan_vulnerabilities({"scope": "full"})
        integrity_data = await self._check_integrity({})
        audit_data = await self._audit_logs({"timeframe_minutes": 1440})  # Last 24h

        # Build a comprehensive threat profile
        threat_profile = {
            "vulnerability_risk": vuln_data.get("risk_percent", 0),
            "integrity_changes": integrity_data.get("changes_detected", 0),
            "audit_alerts": len(audit_data.get("alerts", [])),
            "failed_logins_24h": audit_data.get("failed_logins", 0),
            "suspicious_events": audit_data.get("suspicious_events", 0),
        }

        analysis = await self.think(
            f"Comprehensive threat analysis for aiOS:\n"
            f"- Vulnerability risk: {threat_profile['vulnerability_risk']}%\n"
            f"- File integrity changes: {threat_profile['integrity_changes']}\n"
            f"- Active audit alerts: {threat_profile['audit_alerts']}\n"
            f"- Failed logins (24h): {threat_profile['failed_logins_24h']}\n"
            f"- Suspicious events: {threat_profile['suspicious_events']}\n"
            f"- Total findings: {vuln_data.get('total_findings', 0)}\n\n"
            f"Provide:\n1. Overall threat level (low/medium/high/critical)\n"
            f"2. Top 3 risks\n3. Recommended actions",
            level=IntelligenceLevel.TACTICAL,
        )

        await self.store_memory("last_threat_analysis", {
            "timestamp": int(time.time()),
            "profile": threat_profile,
        })

        return {
            "success": True,
            "threat_profile": threat_profile,
            "analysis": analysis.strip(),
            "data_sources": {
                "vulnerability_findings": vuln_data.get("total_findings", 0),
                "integrity_files_checked": integrity_data.get("files_checked", 0),
                "audit_events_reviewed": audit_data.get("total_events", 0),
            },
        }

    # ------------------------------------------------------------------
    # Background loops
    # ------------------------------------------------------------------

    async def _ids_loop(self) -> None:
        """Continuous intrusion detection loop."""
        while not self._shutdown_event.is_set():
            try:
                result = await self._intrusion_check({})
                if result.get("threat_level") != "clean":
                    logger.critical("IDS ALERT: threat_level=%s", result["threat_level"])
            except Exception as exc:
                logger.error("IDS loop error: %s", exc)
            try:
                await asyncio.wait_for(
                    self._shutdown_event.wait(),
                    timeout=IDS_CHECK_INTERVAL_S,
                )
            except asyncio.TimeoutError:
                pass

    async def _audit_loop(self) -> None:
        """Periodic audit log review loop."""
        while not self._shutdown_event.is_set():
            try:
                await self._audit_logs({"timeframe_minutes": int(AUDIT_CHECK_INTERVAL_S / 60) + 1})
            except Exception as exc:
                logger.error("Audit loop error: %s", exc)
            try:
                await asyncio.wait_for(
                    self._shutdown_event.wait(),
                    timeout=AUDIT_CHECK_INTERVAL_S,
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
                self._ids_loop(),
                self._audit_loop(),
                self._shutdown_event.wait(),
            )
        finally:
            await self.unregister_from_orchestrator()
            await self._close_channels()
            self._running = False


if __name__ == "__main__":
    import os
    agent = SecurityAgent(agent_id=os.getenv("AIOS_AGENT_NAME", "security-agent"))
    asyncio.run(agent.run())
