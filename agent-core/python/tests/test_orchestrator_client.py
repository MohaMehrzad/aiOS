"""
Tests for OrchestratorClient -- goal submission, agent registration,
status retrieval, and retry/connection behaviour.
"""

from __future__ import annotations

import asyncio
import json
import time
from typing import Any
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from aios_agent.orchestrator_client import OrchestratorClient, OrchestratorClientConfig


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def oc_config() -> OrchestratorClientConfig:
    return OrchestratorClientConfig(
        address="localhost:50051",
        timeout_s=5.0,
        max_retries=2,
        retry_delay_s=0.01,
    )


@pytest.fixture
def client(oc_config: OrchestratorClientConfig) -> OrchestratorClient:
    return OrchestratorClient(config=oc_config)


def _mock_channel(response_data: dict[str, Any] | None = None):
    """Return a MagicMock channel whose unary_unary returns an AsyncMock."""
    data = response_data or {"success": True}
    channel = MagicMock()
    call_fn = AsyncMock(return_value=json.dumps(data).encode())
    channel.unary_unary.return_value = call_fn
    channel.close = AsyncMock()
    return channel


# ---------------------------------------------------------------------------
# Config tests
# ---------------------------------------------------------------------------


class TestOrchestratorClientConfig:
    def test_defaults(self):
        cfg = OrchestratorClientConfig()
        assert cfg.address == "localhost:50051"
        assert cfg.timeout_s == 30.0
        assert cfg.max_retries == 3

    def test_custom_values(self, oc_config: OrchestratorClientConfig):
        assert oc_config.timeout_s == 5.0
        assert oc_config.max_retries == 2


# ---------------------------------------------------------------------------
# Connection lifecycle
# ---------------------------------------------------------------------------


class TestConnectionLifecycle:
    def test_connect_creates_channel(self, client: OrchestratorClient):
        with patch("aios_agent.orchestrator_client.grpc.aio.insecure_channel") as mock_ch:
            mock_ch.return_value = MagicMock()
            client.connect()
            assert client._channel is not None
            mock_ch.assert_called_once_with("localhost:50051")

    def test_connect_is_idempotent(self, client: OrchestratorClient):
        with patch("aios_agent.orchestrator_client.grpc.aio.insecure_channel") as mock_ch:
            mock_ch.return_value = MagicMock()
            client.connect()
            client.connect()
            mock_ch.assert_called_once()

    @pytest.mark.asyncio
    async def test_close_clears_channel(self, client: OrchestratorClient):
        ch = MagicMock()
        ch.close = AsyncMock()
        client._channel = ch
        await client.close()
        assert client._channel is None
        ch.close.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_context_manager(self, client: OrchestratorClient):
        with patch("aios_agent.orchestrator_client.grpc.aio.insecure_channel") as mock_ch:
            mock_ch.return_value = MagicMock()
            mock_ch.return_value.close = AsyncMock()
            async with client as c:
                assert c is client
                assert client._channel is not None
        # After __aexit__, channel is closed
        assert client._channel is None

    def test_default_config_from_env(self):
        with patch.dict("os.environ", {"AIOS_ORCHESTRATOR_ADDR": "remote:9090"}):
            c = OrchestratorClient()
            assert c.config.address == "remote:9090"


# ---------------------------------------------------------------------------
# Encode/decode helpers
# ---------------------------------------------------------------------------


class TestEncodeDecode:
    def test_encode(self):
        data = {"key": "value", "num": 42}
        result = OrchestratorClient._encode(data)
        assert isinstance(result, bytes)
        assert json.loads(result) == data

    def test_decode_valid(self):
        raw = json.dumps({"hello": "world"}).encode()
        assert OrchestratorClient._decode(raw) == {"hello": "world"}

    def test_decode_empty(self):
        assert OrchestratorClient._decode(b"") == {}

    def test_decode_invalid(self):
        result = OrchestratorClient._decode(b"\xff\xfe")
        assert "_raw" in result


# ---------------------------------------------------------------------------
# Goal submission
# ---------------------------------------------------------------------------


class TestGoalSubmission:
    @pytest.mark.asyncio
    async def test_submit_goal_returns_id(self, client: OrchestratorClient):
        channel = _mock_channel({"id": "goal-12345"})
        client._channel = channel

        goal_id = await client.submit_goal("Install nginx", priority=7)

        assert goal_id == "goal-12345"
        # Verify the call was made to the correct method
        channel.unary_unary.assert_called_once()
        call_path = channel.unary_unary.call_args[0][0]
        assert "SubmitGoal" in call_path

    @pytest.mark.asyncio
    async def test_submit_goal_includes_metadata(self, client: OrchestratorClient):
        channel = _mock_channel({"id": "goal-m1"})
        client._channel = channel

        await client.submit_goal(
            "Deploy app",
            priority=8,
            source="test",
            tags=["deploy", "production"],
            metadata={"env": "prod"},
        )

        # Verify the request payload
        call_fn = channel.unary_unary.return_value
        request_bytes = call_fn.call_args[0][0]
        payload = json.loads(request_bytes)
        assert payload["description"] == "Deploy app"
        assert payload["priority"] == 8
        assert payload["tags"] == ["deploy", "production"]
        assert "metadata_json" in payload

    @pytest.mark.asyncio
    async def test_submit_goal_default_values(self, client: OrchestratorClient):
        channel = _mock_channel({"id": "goal-d1"})
        client._channel = channel

        await client.submit_goal("Simple goal")

        call_fn = channel.unary_unary.return_value
        payload = json.loads(call_fn.call_args[0][0])
        assert payload["priority"] == 5
        assert payload["source"] == "python-client"
        assert payload["tags"] == []


# ---------------------------------------------------------------------------
# Goal status and management
# ---------------------------------------------------------------------------


class TestGoalManagement:
    @pytest.mark.asyncio
    async def test_get_goal_status(self, client: OrchestratorClient):
        channel = _mock_channel({
            "goal": {"id": "g1", "status": "active"},
            "tasks": [{"id": "t1"}],
            "current_phase": "executing",
            "progress_percent": 50.0,
        })
        client._channel = channel

        status = await client.get_goal_status("g1")

        assert status["goal"]["status"] == "active"
        assert len(status["tasks"]) == 1
        assert status["current_phase"] == "executing"
        assert status["progress_percent"] == 50.0

    @pytest.mark.asyncio
    async def test_cancel_goal_success(self, client: OrchestratorClient):
        channel = _mock_channel({"success": True})
        client._channel = channel

        result = await client.cancel_goal("g1")
        assert result is True

    @pytest.mark.asyncio
    async def test_cancel_goal_failure(self, client: OrchestratorClient):
        channel = _mock_channel({"success": False, "message": "already completed"})
        client._channel = channel

        result = await client.cancel_goal("g1")
        assert result is False

    @pytest.mark.asyncio
    async def test_list_goals(self, client: OrchestratorClient):
        channel = _mock_channel({
            "goals": [{"id": "g1"}, {"id": "g2"}],
            "total": 2,
        })
        client._channel = channel

        goals, total = await client.list_goals(status_filter="active", limit=10)
        assert total == 2
        assert len(goals) == 2


# ---------------------------------------------------------------------------
# Agent registration
# ---------------------------------------------------------------------------


class TestAgentRegistration:
    @pytest.mark.asyncio
    async def test_register_agent(self, client: OrchestratorClient):
        channel = _mock_channel({"success": True})
        client._channel = channel

        result = await client.register_agent(
            agent_id="agent-1",
            agent_type="system",
            capabilities=["system.health", "system.metrics"],
            tool_namespaces=["system"],
        )

        assert result is True
        call_fn = channel.unary_unary.return_value
        payload = json.loads(call_fn.call_args[0][0])
        assert payload["agent_id"] == "agent-1"
        assert payload["agent_type"] == "system"
        assert "system.health" in payload["capabilities"]

    @pytest.mark.asyncio
    async def test_unregister_agent(self, client: OrchestratorClient):
        channel = _mock_channel({"success": True})
        client._channel = channel

        result = await client.unregister_agent("agent-1")
        assert result is True

    @pytest.mark.asyncio
    async def test_heartbeat(self, client: OrchestratorClient):
        channel = _mock_channel({"success": True})
        client._channel = channel

        result = await client.heartbeat(
            agent_id="agent-1",
            status="busy",
            current_task_id="t1",
            cpu_usage=50.0,
            memory_usage_mb=1024.0,
        )

        assert result is True
        call_fn = channel.unary_unary.return_value
        payload = json.loads(call_fn.call_args[0][0])
        assert payload["status"] == "busy"
        assert payload["current_task_id"] == "t1"

    @pytest.mark.asyncio
    async def test_list_agents(self, client: OrchestratorClient):
        channel = _mock_channel({
            "agents": [
                {"agent_id": "a1", "agent_type": "system"},
                {"agent_id": "a2", "agent_type": "network"},
            ]
        })
        client._channel = channel

        agents = await client.list_agents()
        assert len(agents) == 2


# ---------------------------------------------------------------------------
# System status
# ---------------------------------------------------------------------------


class TestSystemStatus:
    @pytest.mark.asyncio
    async def test_get_system_status(self, client: OrchestratorClient):
        channel = _mock_channel({
            "active_goals": 3,
            "pending_tasks": 5,
            "active_agents": 4,
            "loaded_models": ["gpt-4", "llama-3"],
            "cpu_percent": 35.0,
            "memory_used_mb": 8192.0,
            "memory_total_mb": 32768.0,
            "autonomy_level": "supervised",
            "uptime_seconds": 7200,
        })
        client._channel = channel

        status = await client.get_system_status()

        assert status["active_goals"] == 3
        assert status["pending_tasks"] == 5
        assert status["active_agents"] == 4
        assert "gpt-4" in status["loaded_models"]
        assert status["autonomy_level"] == "supervised"

    @pytest.mark.asyncio
    async def test_get_system_status_defaults(self, client: OrchestratorClient):
        channel = _mock_channel({})
        client._channel = channel

        status = await client.get_system_status()
        assert status["active_goals"] == 0
        assert status["autonomy_level"] == "unknown"


# ---------------------------------------------------------------------------
# Wait for goal
# ---------------------------------------------------------------------------


class TestWaitForGoal:
    @pytest.mark.asyncio
    async def test_wait_returns_on_completed(self, client: OrchestratorClient):
        channel = _mock_channel({
            "goal": {"status": "completed"},
            "tasks": [],
            "current_phase": "done",
            "progress_percent": 100.0,
        })
        client._channel = channel

        result = await client.wait_for_goal("g1", poll_interval_s=0.01, timeout_s=1.0)
        assert result["goal"]["status"] == "completed"

    @pytest.mark.asyncio
    async def test_wait_returns_on_failed(self, client: OrchestratorClient):
        channel = _mock_channel({
            "goal": {"status": "failed"},
            "tasks": [],
            "current_phase": "failed",
            "progress_percent": 0.0,
        })
        client._channel = channel

        result = await client.wait_for_goal("g1", poll_interval_s=0.01, timeout_s=1.0)
        assert result["goal"]["status"] == "failed"

    @pytest.mark.asyncio
    async def test_wait_timeout_raises(self, client: OrchestratorClient):
        channel = _mock_channel({
            "goal": {"status": "active"},
            "tasks": [],
            "current_phase": "executing",
            "progress_percent": 50.0,
        })
        client._channel = channel

        with pytest.raises(TimeoutError):
            await client.wait_for_goal("g1", poll_interval_s=0.01, timeout_s=0.05)

    @pytest.mark.asyncio
    async def test_wait_polls_until_terminal(self, client: OrchestratorClient):
        call_count = 0

        async def _changing_status(request_bytes, timeout=None):
            nonlocal call_count
            call_count += 1
            if call_count < 3:
                return json.dumps({
                    "goal": {"status": "active"},
                    "tasks": [],
                    "current_phase": "executing",
                    "progress_percent": 50.0,
                }).encode()
            return json.dumps({
                "goal": {"status": "completed"},
                "tasks": [],
                "current_phase": "done",
                "progress_percent": 100.0,
            }).encode()

        channel = MagicMock()
        channel.unary_unary.return_value = _changing_status
        client._channel = channel

        result = await client.wait_for_goal("g1", poll_interval_s=0.01, timeout_s=5.0)
        assert result["goal"]["status"] == "completed"
        assert call_count >= 3


# ---------------------------------------------------------------------------
# Retry behaviour
# ---------------------------------------------------------------------------


class TestRetryBehaviour:
    @pytest.mark.asyncio
    async def test_retry_on_unavailable(self, client: OrchestratorClient):
        import grpc

        call_count = 0

        async def _flaky(request_bytes, timeout=None):
            nonlocal call_count
            call_count += 1
            if call_count < 2:
                raise grpc.aio.AioRpcError(
                    grpc.StatusCode.UNAVAILABLE,
                    initial_metadata=None,
                    trailing_metadata=None,
                    details="service unavailable",
                    debug_error_string=None,
                )
            return json.dumps({"success": True}).encode()

        channel = MagicMock()
        channel.unary_unary.return_value = _flaky
        client._channel = channel
        client.config.retry_delay_s = 0.01

        result = await client._call("SomeMethod", {})
        assert result["success"] is True
        assert call_count == 2

    @pytest.mark.asyncio
    async def test_max_retries_exceeded(self, client: OrchestratorClient):
        import grpc

        async def _always_fail(request_bytes, timeout=None):
            raise grpc.aio.AioRpcError(
                grpc.StatusCode.UNAVAILABLE,
                initial_metadata=None,
                trailing_metadata=None,
                details="down",
                debug_error_string=None,
            )

        channel = MagicMock()
        channel.unary_unary.return_value = _always_fail
        client._channel = channel
        client.config.retry_delay_s = 0.01
        client.config.max_retries = 2

        with pytest.raises(RuntimeError, match="failed after 2 retries"):
            await client._call("BadMethod", {})

    @pytest.mark.asyncio
    async def test_non_retryable_error_raises_immediately(self, client: OrchestratorClient):
        import grpc

        async def _auth_error(request_bytes, timeout=None):
            raise grpc.aio.AioRpcError(
                grpc.StatusCode.PERMISSION_DENIED,
                initial_metadata=None,
                trailing_metadata=None,
                details="access denied",
                debug_error_string=None,
            )

        channel = MagicMock()
        channel.unary_unary.return_value = _auth_error
        client._channel = channel

        with pytest.raises(grpc.aio.AioRpcError):
            await client._call("SecureMethod", {})

    @pytest.mark.asyncio
    async def test_auto_connect_on_call(self, client: OrchestratorClient):
        """_call should auto-connect if no channel exists."""
        assert client._channel is None
        with patch("aios_agent.orchestrator_client.grpc.aio.insecure_channel") as mock_ch:
            ch = MagicMock()
            call_fn = AsyncMock(return_value=json.dumps({"ok": True}).encode())
            ch.unary_unary.return_value = call_fn
            mock_ch.return_value = ch
            result = await client._call("Test", {})
        assert result["ok"] is True
