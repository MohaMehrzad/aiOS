"""
Tests for PackageAgent -- package search, install, remove, CVE checking, and updates.
"""

from __future__ import annotations

import json
from typing import Any
from unittest.mock import AsyncMock, patch

import pytest

from aios_agent.agents.package import PackageAgent
from aios_agent.base import AgentConfig


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def config() -> AgentConfig:
    return AgentConfig(max_retries=1, retry_delay_s=0.01, grpc_timeout_s=2.0)


@pytest.fixture
def agent(config: AgentConfig) -> PackageAgent:
    return PackageAgent(agent_id="package-test-001", config=config)


# ---------------------------------------------------------------------------
# Basics
# ---------------------------------------------------------------------------


class TestPackageAgentBasics:
    def test_agent_type(self, agent: PackageAgent):
        assert agent.get_agent_type() == "package"

    def test_capabilities(self, agent: PackageAgent):
        caps = agent.get_capabilities()
        assert "package.install" in caps
        assert "package.remove" in caps
        assert "package.search" in caps
        assert "package.check_vulnerabilities" in caps


# ---------------------------------------------------------------------------
# Task dispatch
# ---------------------------------------------------------------------------


class TestPackageTaskDispatch:
    @pytest.mark.asyncio
    async def test_install_keyword(self, agent: PackageAgent):
        with patch.object(agent, "_install_package", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({
                "description": "install nginx",
                "input_json": {"packages": ["nginx"]},
            })
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_remove_keyword(self, agent: PackageAgent):
        with patch.object(agent, "_remove_package", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({
                "description": "remove old package",
                "input_json": {"packages": ["old-pkg"]},
            })
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_update_keyword(self, agent: PackageAgent):
        with patch.object(agent, "_update_all", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "update all packages"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_vulnerability_keyword(self, agent: PackageAgent):
        with patch.object(agent, "_check_vulnerabilities", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "check for CVE vulnerabilities"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_search_keyword(self, agent: PackageAgent):
        with patch.object(agent, "_search_packages", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({
                "description": "search for web servers",
                "input_json": {"query": "web server"},
            })
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_list_keyword(self, agent: PackageAgent):
        with patch.object(agent, "_list_installed", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "list installed packages"})
        m.assert_awaited_once()


# ---------------------------------------------------------------------------
# Package search
# ---------------------------------------------------------------------------


class TestPackageSearch:
    @pytest.mark.asyncio
    async def test_search_returns_results(self, agent: PackageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "packages": [
                    {"name": "nginx", "version": "1.24", "description": "web server"},
                    {"name": "apache2", "version": "2.4", "description": "web server"},
                ]
            }}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._search_packages("web server")

        assert result["success"] is True
        assert result["result_count"] == 2
        assert result["query"] == "web server"

    @pytest.mark.asyncio
    async def test_search_empty_query(self, agent: PackageAgent):
        result = await agent._search_packages("")
        assert result["success"] is False
        assert "No search query" in result["error"]

    @pytest.mark.asyncio
    async def test_search_tool_failure(self, agent: PackageAgent):
        async def _fail(name, input_json=None, *, reason="", task_id=None):
            return {"success": False, "error": "repo unavailable"}

        with patch.object(agent, "call_tool", side_effect=_fail):
            result = await agent._search_packages("nginx")

        assert result["success"] is False


# ---------------------------------------------------------------------------
# Package installation
# ---------------------------------------------------------------------------


class TestInstallPackage:
    @pytest.mark.asyncio
    async def test_successful_install(self, agent: PackageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "package.resolve_dependencies":
                return {"success": True, "output": {"dependencies": ["libssl"]}}
            if name == "package.cve_check":
                return {"success": True, "output": {"vulnerabilities": []}}
            if name == "package.install":
                return {"success": True, "execution_id": "ex-1"}
            if name == "package.verify":
                return {"success": True, "output": {"version": "1.24.0"}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "store_memory", new_callable=AsyncMock), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._install_package(["nginx"], {})

        assert result["success"] is True
        assert result["installed"] == 1
        assert result["results"][0]["installed_version"] == "1.24.0"
        assert result["results"][0]["dependencies"] == ["libssl"]

    @pytest.mark.asyncio
    async def test_install_empty_packages(self, agent: PackageAgent):
        result = await agent._install_package([], {})
        assert result["success"] is False
        assert "No packages specified" in result["error"]

    @pytest.mark.asyncio
    async def test_install_skipped_due_to_cve(self, agent: PackageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "package.resolve_dependencies":
                return {"success": True, "output": {"dependencies": []}}
            if name == "package.cve_check":
                return {"success": True, "output": {
                    "vulnerabilities": [
                        {"cve": "CVE-2024-999", "severity": "critical",
                         "description": "remote code execution"}
                    ]
                }}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="SKIP, too dangerous"), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._install_package(["vuln-pkg"], {})

        assert result["success"] is False
        assert result["results"][0]["success"] is False
        assert "CVE" in result["results"][0]["error"]

    @pytest.mark.asyncio
    async def test_install_force_ignores_cve(self, agent: PackageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "package.resolve_dependencies":
                return {"success": True, "output": {"dependencies": []}}
            if name == "package.cve_check":
                return {"success": True, "output": {
                    "vulnerabilities": [
                        {"cve": "CVE-2024-999", "severity": "critical",
                         "description": "bad"}
                    ]
                }}
            if name == "package.install":
                return {"success": True, "execution_id": "ex-1"}
            if name == "package.verify":
                return {"success": True, "output": {"version": "1.0"}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "store_memory", new_callable=AsyncMock), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._install_package(["vuln-pkg"], {"force": True})

        assert result["success"] is True
        assert result["installed"] == 1

    @pytest.mark.asyncio
    async def test_install_failure(self, agent: PackageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "package.resolve_dependencies":
                return {"success": True, "output": {"dependencies": []}}
            if name == "package.cve_check":
                return {"success": True, "output": {"vulnerabilities": []}}
            if name == "package.install":
                return {"success": False, "error": "disk full"}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._install_package(["pkg"], {})

        assert result["success"] is False
        assert result["results"][0]["error"] == "disk full"


# ---------------------------------------------------------------------------
# Package removal
# ---------------------------------------------------------------------------


class TestRemovePackage:
    @pytest.mark.asyncio
    async def test_successful_removal(self, agent: PackageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "package.reverse_dependencies":
                return {"success": True, "output": {"dependents": []}}
            if name == "package.remove":
                return {"success": True, "execution_id": "ex-r1"}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._remove_package(["old-pkg"], {})

        assert result["success"] is True
        assert result["removed"] == 1

    @pytest.mark.asyncio
    async def test_removal_blocked_by_dependents(self, agent: PackageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "package.reverse_dependencies":
                return {"success": True, "output": {"dependents": ["important-app"]}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="KEEP, would break important-app"):
            result = await agent._remove_package(["critical-lib"], {})

        assert result["success"] is False
        assert "dependents" in result["results"][0]["error"]

    @pytest.mark.asyncio
    async def test_removal_empty_list(self, agent: PackageAgent):
        result = await agent._remove_package([], {})
        assert result["success"] is False


# ---------------------------------------------------------------------------
# CVE checking
# ---------------------------------------------------------------------------


class TestCVECheck:
    @pytest.mark.asyncio
    async def test_cve_check_with_findings(self, agent: PackageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "package.cve_check":
                return {"success": True, "output": {
                    "vulnerabilities": [
                        {"cve": "CVE-2024-001", "severity": "critical",
                         "package": "openssl", "fix_available": True,
                         "description": "overflow"},
                        {"cve": "CVE-2024-002", "severity": "low",
                         "package": "zlib", "fix_available": False,
                         "description": "minor"},
                    ]
                }}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="1. Update openssl\n2. Monitor zlib"), \
             patch.object(agent, "update_metric", new_callable=AsyncMock):
            result = await agent._check_vulnerabilities({"packages": ["openssl", "zlib"]})

        assert result["success"] is True
        assert result["total_vulnerabilities"] == 2
        assert result["by_severity"]["critical"] == 1
        assert result["by_severity"]["low"] == 1
        assert result["fixable"] == 1
        assert len(result["recommendations"]) > 0

    @pytest.mark.asyncio
    async def test_cve_check_no_packages(self, agent: PackageAgent):
        """When no packages specified, it lists installed packages first."""
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "package.list_installed":
                return {"success": True, "output": {
                    "packages": [{"name": "curl"}, {"name": "wget"}]
                }}
            if name == "package.cve_check":
                return {"success": True, "output": {"vulnerabilities": []}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock):
            result = await agent._check_vulnerabilities({})

        assert result["success"] is True
        assert result["packages_checked"] == 2


# ---------------------------------------------------------------------------
# System-wide update
# ---------------------------------------------------------------------------


class TestUpdateAll:
    @pytest.mark.asyncio
    async def test_update_when_up_to_date(self, agent: PackageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "package.refresh_index":
                return {"success": True, "output": {}}
            if name == "package.list_updates":
                return {"success": True, "output": {"updates": []}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._update_all({})

        assert result["success"] is True
        assert "up to date" in result["message"]

    @pytest.mark.asyncio
    async def test_dry_run_lists_updates(self, agent: PackageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "package.refresh_index":
                return {"success": True, "output": {}}
            if name == "package.list_updates":
                return {"success": True, "output": {
                    "updates": [
                        {"package": "curl", "current": "7.0", "available": "7.1"},
                    ]
                }}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._update_all({"dry_run": True})

        assert result["dry_run"] is True
        assert result["updates_available"] == 1

    @pytest.mark.asyncio
    async def test_actual_update(self, agent: PackageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "package.refresh_index":
                return {"success": True, "output": {}}
            if name == "package.list_updates":
                return {"success": True, "output": {
                    "updates": [{"package": "curl", "current": "7.0", "available": "7.1"}]
                }}
            if name == "package.cve_check":
                return {"success": True, "output": {"vulnerabilities": []}}
            if name == "package.update_all":
                return {"success": True, "output": {"updated_count": 1}, "execution_id": "e1"}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "push_event", new_callable=AsyncMock), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._update_all({})

        assert result["success"] is True
        assert result["updated_count"] == 1


# ---------------------------------------------------------------------------
# _extract_package_names helper
# ---------------------------------------------------------------------------


class TestExtractPackageNames:
    def test_extract_after_install(self):
        result = PackageAgent._extract_package_names("install nginx and curl", "install")
        assert "nginx" in result
        assert "curl" in result

    def test_extract_after_remove(self):
        result = PackageAgent._extract_package_names("remove old-package", "remove")
        assert "old-package" in result

    def test_skips_filler_words(self):
        result = PackageAgent._extract_package_names("install the package nginx", "install")
        assert "nginx" in result
        assert "the" not in result
        assert "package" not in result

    def test_empty_description(self):
        result = PackageAgent._extract_package_names("", "install")
        assert result == []

    def test_no_action_match(self):
        result = PackageAgent._extract_package_names("do something else", "install")
        assert result == []
