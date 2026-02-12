"""
Tests for SystemAgent -- system health monitoring, service management, and metrics.

Covers task dispatch, health checks with threshold logic, service restart flow,
metrics collection, process listing, and the extract_service_name helper.
"""

from __future__ import annotations

import json
import time
from typing import Any
from unittest.mock import AsyncMock, patch

import pytest

from aios_agent.agents.system import (
    CPU_CRIT_THRESHOLD,
    CPU_WARN_THRESHOLD,
    DISK_CRIT_THRESHOLD,
    DISK_WARN_THRESHOLD,
    MEM_CRIT_THRESHOLD,
    MEM_WARN_THRESHOLD,
    SystemAgent,
)
from aios_agent.base import AgentConfig


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def config() -> AgentConfig:
    return AgentConfig(
        max_retries=1,
        retry_delay_s=0.01,
        grpc_timeout_s=2.0,
    )


@pytest.fixture
def agent(config: AgentConfig) -> SystemAgent:
    return SystemAgent(agent_id="system-test-001", config=config)


def _mock_grpc(agent: SystemAgent, responses: dict[str, bytes] | bytes | None = None):
    """Return a patched _grpc_call that returns canned responses.

    If *responses* is a dict, keys are matched against the method argument.
    Otherwise all calls get the same bytes back.
    """
    default = json.dumps({"success": True}).encode()

    async def _side_effect(channel, service, method, data, **kw):
        if isinstance(responses, dict):
            return responses.get(method, default)
        return responses or default

    return patch.object(agent, "_grpc_call", new_callable=AsyncMock, side_effect=_side_effect)


# ---------------------------------------------------------------------------
# Capabilities and agent type
# ---------------------------------------------------------------------------


class TestSystemAgentBasics:
    def test_agent_type(self, agent: SystemAgent):
        assert agent.get_agent_type() == "system"

    def test_capabilities(self, agent: SystemAgent):
        caps = agent.get_capabilities()
        assert "system.health_check" in caps
        assert "system.metrics" in caps
        assert "system.restart_service" in caps


# ---------------------------------------------------------------------------
# Task dispatch
# ---------------------------------------------------------------------------


class TestTaskDispatch:
    @pytest.mark.asyncio
    async def test_health_keyword_dispatches_check_health(self, agent: SystemAgent):
        with patch.object(agent, "_check_health", new_callable=AsyncMock, return_value={"healthy": True}) as mock_h:
            result = await agent.handle_task({"description": "Run a health check"})
        mock_h.assert_awaited_once()
        assert result["healthy"] is True

    @pytest.mark.asyncio
    async def test_restart_keyword_dispatches_restart_service(self, agent: SystemAgent):
        with patch.object(agent, "_restart_service", new_callable=AsyncMock, return_value={"success": True}) as mock_r:
            result = await agent.handle_task({
                "description": "restart nginx",
                "input_json": {"service": "nginx"},
            })
        mock_r.assert_awaited_once_with("nginx")

    @pytest.mark.asyncio
    async def test_metric_keyword_dispatches_get_metrics(self, agent: SystemAgent):
        with patch.object(agent, "_get_metrics", new_callable=AsyncMock, return_value={"success": True}) as mock_m:
            await agent.handle_task({"description": "get cpu metrics"})
        mock_m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_process_keyword_dispatches_list_processes(self, agent: SystemAgent):
        with patch.object(agent, "_list_processes", new_callable=AsyncMock, return_value={"success": True}) as mock_p:
            await agent.handle_task({"description": "list running processes"})
        mock_p.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_unclear_task_uses_ai_fallback(self, agent: SystemAgent):
        """When no keyword matches, the agent calls think() then dispatches."""
        with patch.object(agent, "think", new_callable=AsyncMock, return_value="check_health"), \
             patch.object(agent, "store_decision", new_callable=AsyncMock), \
             patch.object(agent, "_check_health", new_callable=AsyncMock, return_value={"healthy": True}):
            result = await agent.handle_task({"description": "do something unrecognised"})
        assert result["healthy"] is True


# ---------------------------------------------------------------------------
# _check_health tests
# ---------------------------------------------------------------------------


class TestCheckHealth:
    @pytest.mark.asyncio
    async def test_healthy_system(self, agent: SystemAgent):
        tool_responses = []

        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "system.metrics":
                return {"success": True, "output": {
                    "cpu_percent": 30.0,
                    "memory_percent": 40.0,
                    "disk_percent": 50.0,
                }}
            if name == "system.service_status":
                return {"success": True, "output": {
                    "services": [
                        {"name": "sshd", "status": "running"},
                        {"name": "nginx", "status": "running"},
                    ]
                }}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock), \
             patch.object(agent, "push_event", new_callable=AsyncMock), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._check_health({})

        assert result["healthy"] is True
        assert result["severity"] == "healthy"
        assert result["issues"] == []
        assert result["failed_services"] == []

    @pytest.mark.asyncio
    async def test_cpu_warning(self, agent: SystemAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "system.metrics":
                return {"success": True, "output": {
                    "cpu_percent": 88.0,
                    "memory_percent": 40.0,
                    "disk_percent": 50.0,
                }}
            return {"success": True, "output": {"services": []}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock), \
             patch.object(agent, "push_event", new_callable=AsyncMock), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._check_health({})

        assert result["healthy"] is False
        assert result["severity"] == "warning"
        assert any(i["resource"] == "cpu" for i in result["issues"])

    @pytest.mark.asyncio
    async def test_critical_triggers_ai_analysis(self, agent: SystemAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "system.metrics":
                return {"success": True, "output": {
                    "cpu_percent": 97.0,
                    "memory_percent": 96.0,
                    "disk_percent": 50.0,
                }}
            return {"success": True, "output": {"services": []}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock), \
             patch.object(agent, "push_event", new_callable=AsyncMock), \
             patch.object(agent, "store_memory", new_callable=AsyncMock), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="1. Kill zombie procs\n2. Clear cache\n3. Alert ops"):
            result = await agent._check_health({})

        assert result["severity"] == "critical"
        assert len(result["recommended_actions"]) == 3
        agent.think.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_failed_services_detected(self, agent: SystemAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "system.metrics":
                return {"success": True, "output": {
                    "cpu_percent": 10.0,
                    "memory_percent": 20.0,
                    "disk_percent": 30.0,
                }}
            if name == "system.service_status":
                return {"success": True, "output": {
                    "services": [
                        {"name": "mysql", "status": "failed"},
                        {"name": "sshd", "status": "running"},
                    ]
                }}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock), \
             patch.object(agent, "push_event", new_callable=AsyncMock), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._check_health({})

        assert "mysql" in result["failed_services"]
        assert result["severity"] == "warning"

    @pytest.mark.asyncio
    async def test_metrics_failure_returns_error(self, agent: SystemAgent):
        async def _fail_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": False, "error": "tool unavailable"}

        with patch.object(agent, "call_tool", side_effect=_fail_tool):
            result = await agent._check_health({})

        assert result["healthy"] is False
        assert result["status"] == "error"


# ---------------------------------------------------------------------------
# _restart_service tests
# ---------------------------------------------------------------------------


class TestRestartService:
    @pytest.mark.asyncio
    async def test_restart_no_service_name(self, agent: SystemAgent):
        result = await agent._restart_service("")
        assert result["success"] is False

    @pytest.mark.asyncio
    async def test_restart_unknown_service(self, agent: SystemAgent):
        result = await agent._restart_service("unknown")
        assert result["success"] is False

    @pytest.mark.asyncio
    async def test_successful_restart(self, agent: SystemAgent):
        call_sequence = []

        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            call_sequence.append(name)
            if name == "system.service_status":
                if len(call_sequence) <= 2:
                    return {"success": True, "output": {"status": "failed"}}
                return {"success": True, "output": {"status": "running"}}
            if name == "system.service_restart":
                return {"success": True, "execution_id": "exec-r1"}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "push_event", new_callable=AsyncMock), \
             patch.object(agent, "store_memory", new_callable=AsyncMock), \
             patch("asyncio.sleep", new_callable=AsyncMock):
            result = await agent._restart_service("nginx")

        assert result["success"] is True
        assert result["new_status"] == "running"

    @pytest.mark.asyncio
    async def test_restart_skipped_by_safety_check(self, agent: SystemAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {"status": "running"}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="NO, this is a critical service"):
            result = await agent._restart_service("sshd")

        assert result["success"] is False
        assert result["action"] == "restart_skipped"


# ---------------------------------------------------------------------------
# _get_metrics tests
# ---------------------------------------------------------------------------


class TestGetMetrics:
    @pytest.mark.asyncio
    async def test_successful_metrics_collection(self, agent: SystemAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "cpu_percent": 45.0,
                "memory_percent": 60.0,
                "disk_percent": 70.0,
            }}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock):
            result = await agent._get_metrics({})

        assert result["success"] is True
        assert result["metrics"]["cpu_percent"] == 45.0
        assert agent.update_metric.await_count == 3

    @pytest.mark.asyncio
    async def test_metrics_failure(self, agent: SystemAgent):
        async def _fail(name, input_json=None, *, reason="", task_id=None):
            return {"success": False, "error": "down"}

        with patch.object(agent, "call_tool", side_effect=_fail):
            result = await agent._get_metrics({})

        assert result["success"] is False


# ---------------------------------------------------------------------------
# _list_processes tests
# ---------------------------------------------------------------------------


class TestListProcesses:
    @pytest.mark.asyncio
    async def test_list_processes_success(self, agent: SystemAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "processes": [
                    {"pid": 1, "name": "init", "cpu": 0.1},
                    {"pid": 100, "name": "python", "cpu": 50.0},
                ]
            }}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._list_processes({"sort_by": "cpu"})

        assert result["success"] is True
        assert result["process_count"] == 2
        assert result["sort_by"] == "cpu"


# ---------------------------------------------------------------------------
# _extract_service_name helper tests
# ---------------------------------------------------------------------------


class TestExtractServiceName:
    def test_extracts_name_after_restart(self):
        assert SystemAgent._extract_service_name("restart nginx") == "nginx"

    def test_extracts_name_after_service(self):
        assert SystemAgent._extract_service_name("service sshd") == "sshd"

    def test_returns_unknown_when_no_match(self):
        assert SystemAgent._extract_service_name("hello world") == "unknown"

    def test_skips_keyword_as_candidate(self):
        assert SystemAgent._extract_service_name("restart service mysql") == "mysql"

    def test_strips_punctuation(self):
        assert SystemAgent._extract_service_name("restart nginx.") == "nginx"
