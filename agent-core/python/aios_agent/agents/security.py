"""
SecurityAgent — Intrusion detection, vulnerability scanning, and policy enforcement.

Capabilities:
  - Continuous intrusion detection monitoring
  - Vulnerability scanning of packages and configurations
  - Security policy enforcement and compliance checking
  - Audit log monitoring for suspicious activity
  - File integrity checking
"""

from __future__ import annotations

import asyncio
import hashlib
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
    """Agent responsible for system security: IDS, scanning, and policy enforcement."""

    def get_agent_type(self) -> str:
        return "security"

    def get_capabilities(self) -> list[str]:
        return [
            "security.scan_vulnerabilities",
            "security.check_integrity",
            "security.enforce_policy",
            "security.audit_logs",
            "security.intrusion_detection",
            "security.compliance_check",
            "security.threat_analysis",
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
        if "policy" in description or "enforce" in description or "compliance" in description:
            return await self._enforce_policy(input_data)
        if "audit" in description or "log" in description:
            return await self._audit_logs(input_data)
        if "intrusion" in description or "ids" in description or "detect" in description:
            return await self._intrusion_check(input_data)
        if "threat" in description or "analys" in description:
            return await self._threat_analysis(input_data)

        decision = await self.think(
            f"Security task: '{task.get('description', '')}'. "
            f"Options: scan_vulnerabilities, check_integrity, enforce_policy, "
            f"audit_logs, intrusion_check, threat_analysis. "
            f"Which action? Reply with ONLY the action name.",
            level=IntelligenceLevel.REACTIVE,
        )

        action = decision.strip().lower()
        if "vuln" in action or "scan" in action:
            return await self._scan_vulnerabilities(input_data)
        if "integr" in action:
            return await self._check_integrity(input_data)
        if "policy" in action or "enforce" in action:
            return await self._enforce_policy(input_data)
        if "audit" in action:
            return await self._audit_logs(input_data)
        if "threat" in action:
            return await self._threat_analysis(input_data)
        return await self._intrusion_check(input_data)

    # ------------------------------------------------------------------
    # Vulnerability scanning
    # ------------------------------------------------------------------

    async def _scan_vulnerabilities(self, params: dict[str, Any]) -> dict[str, Any]:
        """Scan for known vulnerabilities in installed packages and configs."""
        scope = params.get("scope", "full")  # full | packages | configs | network
        targets = params.get("targets", [])

        findings: list[dict[str, Any]] = []
        scan_tasks: list[tuple[str, Any]] = []

        # Package vulnerability scan
        if scope in ("full", "packages"):
            scan_tasks.append(("packages", self.call_tool(
                "security.scan_packages",
                {"targets": targets},
                reason="Vulnerability scan: installed packages",
            )))

        # Configuration scan
        if scope in ("full", "configs"):
            scan_tasks.append(("configs", self.call_tool(
                "security.scan_configs",
                {"check_permissions": True, "check_defaults": True},
                reason="Vulnerability scan: system configurations",
            )))

        # Network exposure scan
        if scope in ("full", "network"):
            scan_tasks.append(("network", self.call_tool(
                "security.scan_ports",
                {"targets": targets or ["localhost"]},
                reason="Vulnerability scan: open ports",
            )))

        # Execute all scans concurrently
        results_map: dict[str, dict[str, Any]] = {}
        if scan_tasks:
            gathered = await asyncio.gather(
                *[task for _, task in scan_tasks],
                return_exceptions=True,
            )
            for (label, _), result in zip(scan_tasks, gathered):
                if isinstance(result, Exception):
                    results_map[label] = {"success": False, "error": str(result)}
                else:
                    results_map[label] = result

        # Aggregate findings
        for label, res in results_map.items():
            if res.get("success"):
                for vuln in res.get("output", {}).get("vulnerabilities", []):
                    findings.append({
                        "source": label,
                        "id": vuln.get("id", ""),
                        "cve": vuln.get("cve", ""),
                        "severity": vuln.get("severity", "unknown"),
                        "package": vuln.get("package", ""),
                        "description": vuln.get("description", ""),
                        "fix_available": vuln.get("fix_available", False),
                        "fix_version": vuln.get("fix_version", ""),
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
                f"Security scan found {len(critical_findings)} critical/high vulnerabilities:\n"
                + "\n".join(
                    f"- {f.get('cve', 'N/A')}: {f.get('description', '')[:100]} "
                    f"(pkg: {f.get('package', 'N/A')}, fix: {f.get('fix_version', 'N/A')})"
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
            "scan_results": {k: {"success": v.get("success", False)} for k, v in results_map.items()},
        }

    # ------------------------------------------------------------------
    # File integrity checking
    # ------------------------------------------------------------------

    async def _check_integrity(self, params: dict[str, Any]) -> dict[str, Any]:
        """Check integrity of critical system files."""
        paths = params.get("paths", [
            "/etc/passwd", "/etc/shadow", "/etc/group",
            "/etc/sudoers", "/etc/ssh/sshd_config",
            "/boot/vmlinuz", "/boot/initrd",
        ])
        baseline_key = "integrity_baseline"

        # Compute current hashes
        hash_result = await self.call_tool(
            "security.file_hashes",
            {"paths": paths, "algorithm": "sha256"},
            reason="Computing file integrity hashes",
        )

        if not hash_result.get("success"):
            return {"success": False, "error": hash_result.get("error", "Failed to compute hashes")}

        current_hashes: dict[str, str] = hash_result.get("output", {}).get("hashes", {})

        # Load baseline from memory
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
            # First run — store as baseline
            logger.info("No integrity baseline found — creating initial baseline")

        # Update baseline
        await self.store_memory(baseline_key, {
            "hashes": current_hashes,
            "updated_at": int(time.time()),
        })

        if changes:
            # Analyse changes with AI
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
                    c.get("path", "").startswith(("/etc/shadow", "/etc/sudoers", "/boot/"))
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
    # Policy enforcement
    # ------------------------------------------------------------------

    async def _enforce_policy(self, params: dict[str, Any]) -> dict[str, Any]:
        """Check and enforce security policies."""
        policy_name = params.get("policy", "default")
        enforce = params.get("enforce", False)
        dry_run = params.get("dry_run", True)

        # Define policies
        policies: dict[str, list[dict[str, Any]]] = {
            "default": [
                {"check": "no_root_ssh", "tool": "security.check_ssh_config", "params": {"check": "permit_root_login"}, "expected": "no"},
                {"check": "password_min_length", "tool": "security.check_password_policy", "params": {"check": "min_length"}, "expected_min": 8},
                {"check": "firewall_enabled", "tool": "security.check_firewall_status", "params": {}, "expected": "active"},
                {"check": "no_empty_passwords", "tool": "security.check_empty_passwords", "params": {}, "expected": 0},
                {"check": "updates_available", "tool": "security.check_security_updates", "params": {}, "expected_max": 0},
            ],
            "strict": [
                {"check": "no_root_ssh", "tool": "security.check_ssh_config", "params": {"check": "permit_root_login"}, "expected": "no"},
                {"check": "ssh_key_only", "tool": "security.check_ssh_config", "params": {"check": "password_auth"}, "expected": "no"},
                {"check": "password_min_length", "tool": "security.check_password_policy", "params": {"check": "min_length"}, "expected_min": 12},
                {"check": "firewall_enabled", "tool": "security.check_firewall_status", "params": {}, "expected": "active"},
                {"check": "no_empty_passwords", "tool": "security.check_empty_passwords", "params": {}, "expected": 0},
                {"check": "selinux_enforcing", "tool": "security.check_selinux", "params": {}, "expected": "enforcing"},
                {"check": "audit_enabled", "tool": "security.check_audit_status", "params": {}, "expected": "active"},
            ],
        }

        checks = policies.get(policy_name, policies["default"])
        results: list[dict[str, Any]] = []
        violations: list[dict[str, Any]] = []
        remediated: list[str] = []

        for check_def in checks:
            check_name = check_def["check"]
            tool_result = await self.call_tool(
                check_def["tool"],
                check_def.get("params", {}),
                reason=f"Policy enforcement: {check_name}",
            )

            compliant = False
            actual_value: Any = None

            if tool_result.get("success"):
                actual_value = tool_result.get("output", {}).get("value")
                if "expected" in check_def:
                    compliant = str(actual_value).lower() == str(check_def["expected"]).lower()
                elif "expected_min" in check_def:
                    try:
                        compliant = float(actual_value or 0) >= check_def["expected_min"]
                    except (ValueError, TypeError):
                        compliant = False
                elif "expected_max" in check_def:
                    try:
                        compliant = float(actual_value or 0) <= check_def["expected_max"]
                    except (ValueError, TypeError):
                        compliant = False
            else:
                actual_value = f"check_failed: {tool_result.get('error', '')}"

            check_result = {
                "check": check_name,
                "compliant": compliant,
                "actual_value": actual_value,
                "policy": check_def.get("expected", check_def.get("expected_min", check_def.get("expected_max"))),
            }
            results.append(check_result)

            if not compliant:
                violations.append(check_result)
                # Attempt remediation if enforcement is enabled
                if enforce and not dry_run:
                    fix_result = await self.call_tool(
                        f"{check_def['tool']}_fix",
                        check_def.get("params", {}),
                        reason=f"Auto-remediating policy violation: {check_name}",
                    )
                    if fix_result.get("success"):
                        remediated.append(check_name)
                        check_result["remediated"] = True

        compliance_pct = (
            (len(results) - len(violations)) / len(results) * 100 if results else 100.0
        )

        await self.update_metric("security.compliance_percent", compliance_pct)
        await self.store_memory(f"policy_check:{policy_name}", {
            "timestamp": int(time.time()),
            "compliance_percent": compliance_pct,
            "violations": len(violations),
        })

        return {
            "success": True,
            "policy": policy_name,
            "total_checks": len(results),
            "compliant": len(results) - len(violations),
            "violations": len(violations),
            "compliance_percent": round(compliance_pct, 1),
            "results": results,
            "violation_details": violations,
            "remediated": remediated,
            "dry_run": dry_run,
        }

    # ------------------------------------------------------------------
    # Audit log monitoring
    # ------------------------------------------------------------------

    async def _audit_logs(self, params: dict[str, Any]) -> dict[str, Any]:
        """Monitor and analyse audit logs for suspicious activity."""
        timeframe_minutes = params.get("timeframe_minutes", 60)
        log_sources = params.get("sources", ["auth", "syslog", "kernel"])

        all_events: list[dict[str, Any]] = []
        for source in log_sources:
            result = await self.call_tool(
                "security.read_audit_logs",
                {"source": source, "timeframe_minutes": timeframe_minutes},
                reason=f"Reading audit logs: {source} (last {timeframe_minutes}m)",
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
        # Check for unexpected network connections
        conn_result = await self.call_tool(
            "security.check_connections",
            {"check_known_bad": True},
            reason="IDS: checking network connections",
        )
        suspicious_connections: list[dict[str, Any]] = []
        if conn_result.get("success"):
            suspicious_connections = conn_result.get("output", {}).get("suspicious", [])

        # Check for unexpected processes
        proc_result = await self.call_tool(
            "security.check_processes",
            {"check_signatures": True},
            reason="IDS: checking running processes",
        )
        suspicious_processes: list[dict[str, Any]] = []
        if proc_result.get("success"):
            suspicious_processes = proc_result.get("output", {}).get("suspicious", [])

        # Check for rootkits
        rootkit_result = await self.call_tool(
            "security.rootkit_check", {},
            reason="IDS: rootkit scan",
        )
        rootkit_findings: list[dict[str, Any]] = []
        if rootkit_result.get("success"):
            rootkit_findings = rootkit_result.get("output", {}).get("findings", [])

        threat_level = "clean"
        if rootkit_findings:
            threat_level = "critical"
        elif suspicious_connections or suspicious_processes:
            threat_level = "suspicious"

        if threat_level != "clean":
            analysis = await self.think(
                f"IDS check results:\n"
                f"- Suspicious connections: {len(suspicious_connections)}\n"
                f"- Suspicious processes: {len(suspicious_processes)}\n"
                f"- Rootkit findings: {len(rootkit_findings)}\n\n"
                f"Connections: {json.dumps(suspicious_connections[:5], default=str)}\n"
                f"Processes: {json.dumps(suspicious_processes[:5], default=str)}\n\n"
                f"Assess the threat and recommend immediate actions.",
                level=IntelligenceLevel.STRATEGIC,
            )

            await self.push_event(
                "security.intrusion_detected",
                {
                    "threat_level": threat_level,
                    "connections": len(suspicious_connections),
                    "processes": len(suspicious_processes),
                    "rootkits": len(rootkit_findings),
                },
                critical=(threat_level == "critical"),
            )
        else:
            analysis = "No threats detected. System appears clean."

        return {
            "threat_level": threat_level,
            "suspicious_connections": suspicious_connections,
            "suspicious_processes": suspicious_processes,
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
            f"- Total vulnerabilities: {vuln_data.get('total_findings', 0)}\n\n"
            f"Provide:\n1. Overall threat level (low/medium/high/critical)\n"
            f"2. Top 3 risks\n3. Recommended actions",
            level=IntelligenceLevel.STRATEGIC,
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
                "vulnerabilities": vuln_data.get("total_findings", 0),
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
