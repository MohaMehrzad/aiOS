"""
Tests for SecurityAgent -- vulnerability scanning, integrity checking,
threat classification, audit log analysis, and intrusion detection.
"""

from __future__ import annotations

import json
from typing import Any
from unittest.mock import AsyncMock, patch

import pytest

from aios_agent.agents.security import SEVERITY_WEIGHTS, SecurityAgent
from aios_agent.base import AgentConfig


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def config() -> AgentConfig:
    return AgentConfig(max_retries=1, retry_delay_s=0.01, grpc_timeout_s=2.0)


@pytest.fixture
def agent(config: AgentConfig) -> SecurityAgent:
    return SecurityAgent(agent_id="security-test-001", config=config)


# ---------------------------------------------------------------------------
# Basics
# ---------------------------------------------------------------------------


class TestSecurityAgentBasics:
    def test_agent_type(self, agent: SecurityAgent):
        assert agent.get_agent_type() == "security"

    def test_capabilities(self, agent: SecurityAgent):
        caps = agent.get_capabilities()
        assert "security.scan_vulnerabilities" in caps
        assert "security.check_integrity" in caps
        assert "security.threat_analysis" in caps
        assert "security.audit_logs" in caps


# ---------------------------------------------------------------------------
# Task dispatch
# ---------------------------------------------------------------------------


class TestSecurityTaskDispatch:
    @pytest.mark.asyncio
    async def test_vulnerability_keyword(self, agent: SecurityAgent):
        with patch.object(agent, "_scan_vulnerabilities", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "scan for vulnerabilities"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_integrity_keyword(self, agent: SecurityAgent):
        with patch.object(agent, "_check_integrity", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "check file integrity"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_audit_keyword(self, agent: SecurityAgent):
        with patch.object(agent, "_audit_logs", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "review audit logs"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_intrusion_keyword(self, agent: SecurityAgent):
        with patch.object(agent, "_intrusion_check", new_callable=AsyncMock,
                          return_value={"threat_level": "clean"}) as m:
            await agent.handle_task({"description": "run intrusion detection"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_threat_keyword(self, agent: SecurityAgent):
        with patch.object(agent, "_threat_analysis", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "threat analysis report"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_policy_keyword(self, agent: SecurityAgent):
        with patch.object(agent, "_enforce_policy", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "enforce security policy"})
        m.assert_awaited_once()


# ---------------------------------------------------------------------------
# Vulnerability scanning
# ---------------------------------------------------------------------------


class TestVulnScan:
    @pytest.mark.asyncio
    async def test_scan_aggregates_findings(self, agent: SecurityAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "security.scan_packages":
                return {"success": True, "output": {
                    "vulnerabilities": [
                        {"id": "v1", "cve": "CVE-2024-001", "severity": "high",
                         "package": "openssl", "description": "buffer overflow",
                         "fix_available": True, "fix_version": "3.0.1"},
                    ]
                }}
            if name == "security.scan_configs":
                return {"success": True, "output": {
                    "vulnerabilities": [
                        {"id": "v2", "cve": "", "severity": "medium",
                         "package": "", "description": "weak SSH config",
                         "fix_available": True, "fix_version": ""},
                    ]
                }}
            if name == "security.scan_ports":
                return {"success": True, "output": {"vulnerabilities": []}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value="1. Update openssl"), \
             patch.object(agent, "push_event", new_callable=AsyncMock), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._scan_vulnerabilities({"scope": "full"})

        assert result["success"] is True
        assert result["total_findings"] == 2
        # High severity should sort first
        assert result["findings"][0]["severity"] == "high"
        assert result["risk_score"] > 0
        assert len(result["recommendations"]) > 0

    @pytest.mark.asyncio
    async def test_scan_no_findings(self, agent: SecurityAgent):
        async def _clean(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {"vulnerabilities": []}}

        with patch.object(agent, "call_tool", side_effect=_clean), \
             patch.object(agent, "push_event", new_callable=AsyncMock), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._scan_vulnerabilities({"scope": "full"})

        assert result["total_findings"] == 0
        assert result["risk_percent"] == 0.0
        assert result["recommendations"] == []

    @pytest.mark.asyncio
    async def test_scan_risk_score_calculation(self, agent: SecurityAgent):
        async def _vulns(name, input_json=None, *, reason="", task_id=None):
            if name == "security.scan_packages":
                return {"success": True, "output": {
                    "vulnerabilities": [
                        {"id": "v1", "severity": "critical", "cve": "CVE-C",
                         "package": "p", "description": "d", "fix_available": False, "fix_version": ""},
                        {"id": "v2", "severity": "low", "cve": "CVE-L",
                         "package": "p", "description": "d", "fix_available": False, "fix_version": ""},
                    ]
                }}
            return {"success": True, "output": {"vulnerabilities": []}}

        with patch.object(agent, "call_tool", side_effect=_vulns), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value="fix stuff"), \
             patch.object(agent, "push_event", new_callable=AsyncMock), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._scan_vulnerabilities({"scope": "packages"})

        expected_score = SEVERITY_WEIGHTS["critical"] + SEVERITY_WEIGHTS["low"]
        assert result["risk_score"] == expected_score

    @pytest.mark.asyncio
    async def test_scan_packages_only(self, agent: SecurityAgent):
        tool_names_called = []

        async def _track(name, input_json=None, *, reason="", task_id=None):
            tool_names_called.append(name)
            return {"success": True, "output": {"vulnerabilities": []}}

        with patch.object(agent, "call_tool", side_effect=_track), \
             patch.object(agent, "push_event", new_callable=AsyncMock), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            await agent._scan_vulnerabilities({"scope": "packages"})

        assert "security.scan_packages" in tool_names_called
        assert "security.scan_configs" not in tool_names_called


# ---------------------------------------------------------------------------
# Integrity checking
# ---------------------------------------------------------------------------


class TestIntegrityCheck:
    @pytest.mark.asyncio
    async def test_first_run_creates_baseline(self, agent: SecurityAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "hashes": {"/etc/passwd": "abc123", "/etc/shadow": "def456"}
            }}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "recall_memory", new_callable=AsyncMock, return_value=None), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._check_integrity({})

        assert result["success"] is True
        assert result["files_checked"] == 2
        assert result["changes_detected"] == 0
        assert result["baseline_existed"] is False

    @pytest.mark.asyncio
    async def test_detects_modified_file(self, agent: SecurityAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "hashes": {"/etc/passwd": "new_hash", "/etc/shadow": "def456"}
            }}

        baseline = {"hashes": {"/etc/passwd": "old_hash", "/etc/shadow": "def456"}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "recall_memory", new_callable=AsyncMock, return_value=baseline), \
             patch.object(agent, "store_memory", new_callable=AsyncMock), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value="Suspicious change"), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._check_integrity({})

        assert result["changes_detected"] == 1
        assert result["changes"][0]["path"] == "/etc/passwd"
        assert result["changes"][0]["type"] == "modified"
        assert result["baseline_existed"] is True

    @pytest.mark.asyncio
    async def test_detects_new_file(self, agent: SecurityAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "hashes": {"/etc/passwd": "abc", "/etc/new_file": "new"}
            }}

        baseline = {"hashes": {"/etc/passwd": "abc"}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "recall_memory", new_callable=AsyncMock, return_value=baseline), \
             patch.object(agent, "store_memory", new_callable=AsyncMock), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value="New file found"), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._check_integrity({})

        new_changes = [c for c in result["changes"] if c["type"] == "new"]
        assert len(new_changes) == 1
        assert new_changes[0]["path"] == "/etc/new_file"

    @pytest.mark.asyncio
    async def test_detects_deleted_file(self, agent: SecurityAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "hashes": {"/etc/passwd": "abc"}
            }}

        baseline = {"hashes": {"/etc/passwd": "abc", "/etc/gone": "xyz"}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "recall_memory", new_callable=AsyncMock, return_value=baseline), \
             patch.object(agent, "store_memory", new_callable=AsyncMock), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value="File deleted"), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._check_integrity({})

        deleted_changes = [c for c in result["changes"] if c["type"] == "deleted"]
        assert len(deleted_changes) == 1
        assert deleted_changes[0]["path"] == "/etc/gone"

    @pytest.mark.asyncio
    async def test_hash_tool_failure(self, agent: SecurityAgent):
        async def _fail(name, input_json=None, *, reason="", task_id=None):
            return {"success": False, "error": "permission denied"}

        with patch.object(agent, "call_tool", side_effect=_fail):
            result = await agent._check_integrity({})

        assert result["success"] is False


# ---------------------------------------------------------------------------
# Audit log analysis
# ---------------------------------------------------------------------------


class TestAuditLogs:
    @pytest.mark.asyncio
    async def test_audit_classifies_events(self, agent: SecurityAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "security.read_audit_logs":
                source = (input_json or {}).get("source", "")
                if source == "auth":
                    return {"success": True, "output": {
                        "events": [
                            {"type": "auth", "message": "Failed login attempt for root"},
                            {"type": "auth", "message": "Failed login attempt for admin"},
                            {"type": "auth", "message": "Successful login for user1"},
                        ] * 5  # 15 events total to exceed threshold
                    }}
                return {"success": True, "output": {"events": []}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="Brute force attack suspected"), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._audit_logs({"sources": ["auth"]})

        assert result["success"] is True
        assert result["failed_logins"] > 10
        assert len(result["alerts"]) > 0
        # Brute force alert should be generated
        brute_alerts = [a for a in result["alerts"] if a["type"] == "brute_force_suspect"]
        assert len(brute_alerts) == 1
        assert brute_alerts[0]["severity"] == "high"

    @pytest.mark.asyncio
    async def test_clean_audit_no_alerts(self, agent: SecurityAgent):
        async def _clean(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "events": [{"type": "info", "message": "System started normally"}]
            }}

        with patch.object(agent, "call_tool", side_effect=_clean):
            result = await agent._audit_logs({"sources": ["syslog"]})

        assert result["alerts"] == []
        assert result["failed_logins"] == 0

    @pytest.mark.asyncio
    async def test_privilege_escalation_detection(self, agent: SecurityAgent):
        events = [{"type": "priv", "message": "sudo command executed by user"}] * 8

        async def _priv(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {"events": events}}

        with patch.object(agent, "call_tool", side_effect=_priv), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value="Suspicious"), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._audit_logs({"sources": ["auth"]})

        assert result["privilege_escalations"] >= 6
        priv_alerts = [a for a in result["alerts"] if a["type"] == "privilege_escalation"]
        assert len(priv_alerts) == 1


# ---------------------------------------------------------------------------
# Intrusion detection
# ---------------------------------------------------------------------------


class TestIntrusionCheck:
    @pytest.mark.asyncio
    async def test_clean_system(self, agent: SecurityAgent):
        async def _clean(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {"suspicious": [], "findings": []}}

        with patch.object(agent, "call_tool", side_effect=_clean):
            result = await agent._intrusion_check({})

        assert result["threat_level"] == "clean"
        assert result["analysis"] == "No threats detected. System appears clean."

    @pytest.mark.asyncio
    async def test_suspicious_connections(self, agent: SecurityAgent):
        async def _suspicious(name, input_json=None, *, reason="", task_id=None):
            if name == "security.check_connections":
                return {"success": True, "output": {
                    "suspicious": [{"remote": "evil.com", "port": 4444}]
                }}
            return {"success": True, "output": {"suspicious": [], "findings": []}}

        with patch.object(agent, "call_tool", side_effect=_suspicious), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="Suspicious outbound connection"), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._intrusion_check({})

        assert result["threat_level"] == "suspicious"
        assert len(result["suspicious_connections"]) == 1

    @pytest.mark.asyncio
    async def test_rootkit_detection_critical(self, agent: SecurityAgent):
        async def _rootkit(name, input_json=None, *, reason="", task_id=None):
            if name == "security.rootkit_check":
                return {"success": True, "output": {
                    "findings": [{"name": "bad_rootkit", "type": "kernel"}]
                }}
            return {"success": True, "output": {"suspicious": []}}

        with patch.object(agent, "call_tool", side_effect=_rootkit), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value="Critical threat"), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._intrusion_check({})

        assert result["threat_level"] == "critical"
        assert len(result["rootkit_findings"]) == 1


# ---------------------------------------------------------------------------
# Threat analysis (integration of multiple sub-methods)
# ---------------------------------------------------------------------------


class TestThreatAnalysis:
    @pytest.mark.asyncio
    async def test_threat_analysis_combines_sources(self, agent: SecurityAgent):
        with patch.object(agent, "_scan_vulnerabilities", new_callable=AsyncMock,
                          return_value={"risk_percent": 35.0, "total_findings": 5}), \
             patch.object(agent, "_check_integrity", new_callable=AsyncMock,
                          return_value={"changes_detected": 2, "files_checked": 10}), \
             patch.object(agent, "_audit_logs", new_callable=AsyncMock,
                          return_value={"alerts": [{"type": "brute"}], "failed_logins": 15,
                                        "suspicious_events": 3, "total_events": 100}), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="Overall threat: medium\n1. Fix CVEs\n2. Investigate"), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._threat_analysis({})

        assert result["success"] is True
        assert result["threat_profile"]["vulnerability_risk"] == 35.0
        assert result["threat_profile"]["integrity_changes"] == 2
        assert result["threat_profile"]["failed_logins_24h"] == 15
        assert result["data_sources"]["vulnerabilities"] == 5
        assert result["data_sources"]["audit_events_reviewed"] == 100


# ---------------------------------------------------------------------------
# SEVERITY_WEIGHTS tests
# ---------------------------------------------------------------------------


class TestSeverityWeights:
    def test_critical_highest(self):
        assert SEVERITY_WEIGHTS["critical"] > SEVERITY_WEIGHTS["high"]

    def test_info_zero(self):
        assert SEVERITY_WEIGHTS["info"] == 0

    def test_ordering(self):
        assert SEVERITY_WEIGHTS["critical"] > SEVERITY_WEIGHTS["high"] > SEVERITY_WEIGHTS["medium"] > SEVERITY_WEIGHTS["low"]
