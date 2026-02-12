"""
Shared pytest fixtures for the aiOS agent test suite.

Provides mocked gRPC channels, tool clients, orchestrator clients,
and reusable agent configurations for all test modules.
"""

from __future__ import annotations

import json
from typing import Any
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from aios_agent.base import AgentConfig


# ---------------------------------------------------------------------------
# Note: pytest-asyncio with asyncio_mode="auto" handles event loops
# automatically. No custom event_loop fixture needed.
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# gRPC mocks
# ---------------------------------------------------------------------------


@pytest.fixture
def mock_grpc_channel():
    """Return a mock gRPC async channel with a unary_unary callable."""
    channel = MagicMock()
    call_fn = AsyncMock(return_value=b'{"success": true}')
    channel.unary_unary.return_value = call_fn
    channel.close = AsyncMock()
    return channel


@pytest.fixture
def mock_tool_client(mock_grpc_channel):
    """Return a mock tool execution client that returns success results."""

    async def _tool_response(name: str, **kwargs: Any) -> dict[str, Any]:
        return {
            "success": True,
            "output": {},
            "tool": name,
            "execution_id": "exec-001",
            "duration_ms": 42,
        }

    mock = AsyncMock(side_effect=_tool_response)
    return mock


@pytest.fixture
def mock_orchestrator_client():
    """Return a mock OrchestratorClient with all async methods mocked."""
    client = MagicMock()
    client.submit_goal = AsyncMock(return_value="goal-abc123")
    client.get_goal_status = AsyncMock(return_value={
        "goal": {"status": "completed"},
        "tasks": [],
        "current_phase": "done",
        "progress_percent": 100.0,
    })
    client.cancel_goal = AsyncMock(return_value=True)
    client.list_goals = AsyncMock(return_value=([], 0))
    client.register_agent = AsyncMock(return_value=True)
    client.unregister_agent = AsyncMock(return_value=True)
    client.heartbeat = AsyncMock(return_value=True)
    client.list_agents = AsyncMock(return_value=[])
    client.get_system_status = AsyncMock(return_value={
        "active_goals": 0,
        "pending_tasks": 0,
        "active_agents": 1,
        "loaded_models": [],
        "cpu_percent": 10.0,
        "memory_used_mb": 512.0,
        "memory_total_mb": 16384.0,
        "autonomy_level": "supervised",
        "uptime_seconds": 3600,
    })
    client.wait_for_goal = AsyncMock(return_value={
        "goal": {"status": "completed"},
        "tasks": [],
        "current_phase": "done",
        "progress_percent": 100.0,
    })
    client.connect = MagicMock()
    client.close = AsyncMock()
    # Support async context manager
    client.__aenter__ = AsyncMock(return_value=client)
    client.__aexit__ = AsyncMock(return_value=None)
    return client


# ---------------------------------------------------------------------------
# Agent configuration
# ---------------------------------------------------------------------------


@pytest.fixture
def temp_agent_config() -> AgentConfig:
    """Return a test-friendly AgentConfig with localhost addresses."""
    return AgentConfig(
        orchestrator_addr="localhost:50051",
        tools_addr="localhost:50052",
        memory_addr="localhost:50053",
        runtime_addr="localhost:50054",
        heartbeat_interval_s=1.0,
        max_retries=1,
        retry_delay_s=0.01,
        grpc_timeout_s=5.0,
        log_level="DEBUG",
    )


# ---------------------------------------------------------------------------
# Helper to build a successful gRPC response
# ---------------------------------------------------------------------------


def make_grpc_response(data: dict[str, Any]) -> bytes:
    """Encode a dict as JSON bytes, mimicking a gRPC response payload."""
    return json.dumps(data, default=str).encode("utf-8")


# ---------------------------------------------------------------------------
# Patch helper for BaseAgent._grpc_call
# ---------------------------------------------------------------------------


@pytest.fixture
def patch_grpc_call():
    """Context-manager fixture that patches BaseAgent._grpc_call.

    Usage in tests::

        async def test_something(patch_grpc_call):
            with patch_grpc_call(return_data={"success": True}) as mock_call:
                ...
    """

    class _Patcher:
        def __call__(self, return_data: dict[str, Any] | None = None):
            data = return_data or {"success": True}
            return patch(
                "aios_agent.base.BaseAgent._grpc_call",
                new_callable=AsyncMock,
                return_value=json.dumps(data, default=str).encode("utf-8"),
            )

    return _Patcher()
