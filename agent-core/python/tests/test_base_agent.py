"""
Tests for BaseAgent — the abstract base class of all aiOS agents.

Covers construction, capabilities, think(), call_tool(), memory operations,
lifecycle (start/stop/heartbeat), and the execute_task wrapper.
"""

from __future__ import annotations

import asyncio
import json
import time
from typing import Any
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from aios_agent.base import AgentConfig, BaseAgent, IntelligenceLevel


# ---------------------------------------------------------------------------
# Concrete subclass for testing the ABC
# ---------------------------------------------------------------------------


class TestableAgent(BaseAgent):
    """Minimal concrete agent used to test BaseAgent behaviour."""

    def __init__(self, *args: Any, **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        self._last_task: dict[str, Any] | None = None

    async def handle_task(self, task: dict[str, Any]) -> dict[str, Any]:
        self._last_task = task
        return {"handled": True, "description": task.get("description", "")}

    def get_capabilities(self) -> list[str]:
        return ["test.cap1", "test.cap2"]

    def get_agent_type(self) -> str:
        return "testable"


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def config() -> AgentConfig:
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


@pytest.fixture
def agent(config: AgentConfig) -> TestableAgent:
    return TestableAgent(agent_id="test-agent-001", config=config)


# ---------------------------------------------------------------------------
# Initialisation tests
# ---------------------------------------------------------------------------


class TestAgentInit:
    def test_agent_id_assigned(self, agent: TestableAgent):
        assert agent.agent_id == "test-agent-001"

    def test_agent_id_auto_generated(self, config: AgentConfig):
        a = TestableAgent(config=config)
        assert a.agent_id.startswith("testable-")
        assert len(a.agent_id) > len("testable-")

    def test_config_is_stored(self, agent: TestableAgent, config: AgentConfig):
        assert agent.config is config
        assert agent.config.tools_addr == "localhost:50052"

    def test_default_config_from_env(self):
        with patch.dict("os.environ", {"AIOS_ORCHESTRATOR_ADDR": "remote:9999"}):
            a = TestableAgent()
            assert a.config.orchestrator_addr == "remote:9999"

    def test_initial_counters(self, agent: TestableAgent):
        assert agent._tasks_completed == 0
        assert agent._tasks_failed == 0
        assert agent._current_task_id is None
        assert agent._running is False

    def test_grpc_channels_initially_none(self, agent: TestableAgent):
        assert agent._orchestrator_channel is None
        assert agent._tools_channel is None
        assert agent._memory_channel is None
        assert agent._runtime_channel is None


# ---------------------------------------------------------------------------
# Capability tests
# ---------------------------------------------------------------------------


class TestCapabilities:
    def test_get_capabilities_returns_list(self, agent: TestableAgent):
        caps = agent.get_capabilities()
        assert isinstance(caps, list)
        assert "test.cap1" in caps
        assert "test.cap2" in caps

    def test_get_agent_type(self, agent: TestableAgent):
        assert agent.get_agent_type() == "testable"


# ---------------------------------------------------------------------------
# think() tests
# ---------------------------------------------------------------------------


class TestThink:
    @pytest.mark.asyncio
    async def test_think_sends_correct_payload(self, agent: TestableAgent):
        response_data = {"text": "AI response", "tokens_used": 42, "model_used": "test-model"}
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=json.dumps(response_data).encode()):
            result = await agent.think("Hello", level=IntelligenceLevel.OPERATIONAL)

        assert result == "AI response"
        call_args = agent._grpc_call.call_args
        # Verify channel, service, method
        assert call_args[0][1] == "aios.runtime.AIRuntime"
        assert call_args[0][2] == "Infer"

    @pytest.mark.asyncio
    async def test_think_dispatches_reactive_level(self, agent: TestableAgent):
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=json.dumps({"text": "ok"}).encode()):
            await agent.think("fast question", level=IntelligenceLevel.REACTIVE)

        payload = json.loads(agent._grpc_call.call_args[0][3])
        assert payload["intelligence_level"] == "reactive"

    @pytest.mark.asyncio
    async def test_think_dispatches_strategic_level(self, agent: TestableAgent):
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=json.dumps({"text": "plan"}).encode()):
            await agent.think("complex plan", level=IntelligenceLevel.STRATEGIC)

        payload = json.loads(agent._grpc_call.call_args[0][3])
        assert payload["intelligence_level"] == "strategic"

    @pytest.mark.asyncio
    async def test_think_accepts_string_level(self, agent: TestableAgent):
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=json.dumps({"text": "tac"}).encode()):
            result = await agent.think("something", level="tactical")
        assert result == "tac"

    @pytest.mark.asyncio
    async def test_think_includes_system_prompt(self, agent: TestableAgent):
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=json.dumps({"text": "x"}).encode()):
            await agent.think("q", system_prompt="custom prompt")
        payload = json.loads(agent._grpc_call.call_args[0][3])
        assert payload["system_prompt"] == "custom prompt"

    @pytest.mark.asyncio
    async def test_think_default_system_prompt_includes_agent_type(self, agent: TestableAgent):
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=json.dumps({"text": "x"}).encode()):
            await agent.think("q")
        payload = json.loads(agent._grpc_call.call_args[0][3])
        assert "testable" in payload["system_prompt"]
        assert agent.agent_id in payload["system_prompt"]


# ---------------------------------------------------------------------------
# call_tool() tests
# ---------------------------------------------------------------------------


class TestCallTool:
    @pytest.mark.asyncio
    async def test_call_tool_builds_correct_request(self, agent: TestableAgent):
        tool_response = {
            "success": True,
            "output_json": json.dumps({"result": 42}),
            "execution_id": "exec-123",
            "duration_ms": 100,
        }
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=json.dumps(tool_response).encode()):
            result = await agent.call_tool("my.tool", {"key": "val"}, reason="test reason")

        assert result["success"] is True
        assert result["output"] == {"result": 42}
        assert result["tool"] == "my.tool"
        assert result["execution_id"] == "exec-123"

        # Verify the service/method
        call_args = agent._grpc_call.call_args
        assert call_args[0][1] == "aios.tools.ToolRegistry"
        assert call_args[0][2] == "Execute"

    @pytest.mark.asyncio
    async def test_call_tool_handles_failure(self, agent: TestableAgent):
        tool_response = {"success": False, "error": "tool exploded", "execution_id": "exec-f"}
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=json.dumps(tool_response).encode()):
            result = await agent.call_tool("broken.tool")

        assert result["success"] is False
        assert "exploded" in result["error"]
        assert result["tool"] == "broken.tool"

    @pytest.mark.asyncio
    async def test_call_tool_with_task_id(self, agent: TestableAgent):
        agent._current_task_id = "task-999"
        tool_response = {"success": True, "output_json": "{}", "execution_id": "e1"}
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=json.dumps(tool_response).encode()):
            await agent.call_tool("t", task_id="override-task")

        payload = json.loads(agent._grpc_call.call_args[0][3])
        assert payload["task_id"] == "override-task"

    @pytest.mark.asyncio
    async def test_call_tool_defaults_to_current_task_id(self, agent: TestableAgent):
        agent._current_task_id = "task-current"
        tool_response = {"success": True, "output_json": "{}", "execution_id": "e"}
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=json.dumps(tool_response).encode()):
            await agent.call_tool("t")

        payload = json.loads(agent._grpc_call.call_args[0][3])
        assert payload["task_id"] == "task-current"

    @pytest.mark.asyncio
    async def test_call_tool_dict_output(self, agent: TestableAgent):
        tool_response = {
            "success": True,
            "output_json": {"direct_dict": True},
            "execution_id": "e",
        }
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=json.dumps(tool_response).encode()):
            result = await agent.call_tool("t")

        assert result["output"] == {"direct_dict": True}


# ---------------------------------------------------------------------------
# Memory operation tests
# ---------------------------------------------------------------------------


class TestMemoryOperations:
    @pytest.mark.asyncio
    async def test_store_memory(self, agent: TestableAgent):
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=b'{}'):
            await agent.store_memory("mykey", {"foo": "bar"})

        call_args = agent._grpc_call.call_args
        assert call_args[0][1] == "aios.memory.MemoryService"
        assert call_args[0][2] == "StoreAgentState"
        payload = json.loads(call_args[0][3])
        assert payload["agent_name"] == "test-agent-001"

    @pytest.mark.asyncio
    async def test_recall_memory_returns_value(self, agent: TestableAgent):
        state_json = json.dumps({"key": "mykey", "value": "hello"})
        response = json.dumps({"state_json": state_json}).encode()
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=response):
            result = await agent.recall_memory("mykey")

        assert result == "hello"

    @pytest.mark.asyncio
    async def test_recall_memory_returns_none_for_wrong_key(self, agent: TestableAgent):
        state_json = json.dumps({"key": "otherkey", "value": "hello"})
        response = json.dumps({"state_json": state_json}).encode()
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=response):
            result = await agent.recall_memory("mykey")

        assert result is None

    @pytest.mark.asyncio
    async def test_push_event(self, agent: TestableAgent):
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=b'{}'):
            await agent.push_event("test.event", {"detail": 1}, critical=True)

        call_args = agent._grpc_call.call_args
        assert call_args[0][2] == "PushEvent"
        payload = json.loads(call_args[0][3])
        assert payload["category"] == "test.event"
        assert payload["source"] == "test-agent-001"
        assert payload["critical"] is True

    @pytest.mark.asyncio
    async def test_update_metric(self, agent: TestableAgent):
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=b'{}'):
            await agent.update_metric("cpu.load", 75.5)

        payload = json.loads(agent._grpc_call.call_args[0][3])
        assert payload["key"] == "cpu.load"
        assert payload["value"] == 75.5

    @pytest.mark.asyncio
    async def test_get_metric(self, agent: TestableAgent):
        response = json.dumps({"value": 42.0}).encode()
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=response):
            result = await agent.get_metric("some.metric")

        assert result == 42.0

    @pytest.mark.asyncio
    async def test_store_pattern(self, agent: TestableAgent):
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=b'{}'):
            pattern_id = await agent.store_pattern("high_cpu", "restart service", 0.95)

        assert isinstance(pattern_id, str)
        assert len(pattern_id) == 12

    @pytest.mark.asyncio
    async def test_find_pattern_found(self, agent: TestableAgent):
        response = json.dumps({
            "found": True,
            "pattern": {"trigger": "high_cpu", "action": "restart"}
        }).encode()
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=response):
            result = await agent.find_pattern("high_cpu")

        assert result == {"trigger": "high_cpu", "action": "restart"}

    @pytest.mark.asyncio
    async def test_find_pattern_not_found(self, agent: TestableAgent):
        response = json.dumps({"found": False}).encode()
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=response):
            result = await agent.find_pattern("unknown_trigger")

        assert result is None


# ---------------------------------------------------------------------------
# Lifecycle tests
# ---------------------------------------------------------------------------


class TestLifecycle:
    @pytest.mark.asyncio
    async def test_register_with_orchestrator(self, agent: TestableAgent):
        response = json.dumps({"success": True}).encode()
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=response):
            result = await agent.register_with_orchestrator()

        assert result is True
        payload = json.loads(agent._grpc_call.call_args[0][3])
        assert payload["agent_id"] == "test-agent-001"
        assert payload["agent_type"] == "testable"
        assert "test.cap1" in payload["capabilities"]

    @pytest.mark.asyncio
    async def test_register_failure_returns_false(self, agent: TestableAgent):
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          side_effect=Exception("connection refused")):
            result = await agent.register_with_orchestrator()

        assert result is False

    @pytest.mark.asyncio
    async def test_unregister_from_orchestrator(self, agent: TestableAgent):
        response = json.dumps({"success": True}).encode()
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=response):
            result = await agent.unregister_from_orchestrator()

        assert result is True

    @pytest.mark.asyncio
    async def test_heartbeat_sends_correct_payload(self, agent: TestableAgent):
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=b'{}'):
            await agent._send_heartbeat()

        payload = json.loads(agent._grpc_call.call_args[0][3])
        assert payload["agent_id"] == "test-agent-001"
        assert payload["status"] == "idle"

    @pytest.mark.asyncio
    async def test_heartbeat_busy_when_task_active(self, agent: TestableAgent):
        agent._current_task_id = "task-active"
        with patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=b'{}'):
            await agent._send_heartbeat()

        payload = json.loads(agent._grpc_call.call_args[0][3])
        assert payload["status"] == "busy"
        assert payload["current_task_id"] == "task-active"

    def test_shutdown_sets_event(self, agent: TestableAgent):
        assert not agent._shutdown_event.is_set()
        agent.shutdown()
        assert agent._shutdown_event.is_set()

    def test_uptime_seconds(self, agent: TestableAgent):
        assert agent.uptime_seconds >= 0

    def test_get_status_stopped(self, agent: TestableAgent):
        status = agent.get_status()
        assert status["agent_id"] == "test-agent-001"
        assert status["agent_type"] == "testable"
        assert status["status"] == "stopped"
        assert status["tasks_completed"] == 0

    def test_get_status_busy(self, agent: TestableAgent):
        agent._running = True
        agent._current_task_id = "t1"
        status = agent.get_status()
        assert status["status"] == "busy"

    def test_get_status_running(self, agent: TestableAgent):
        agent._running = True
        status = agent.get_status()
        assert status["status"] == "running"

    @pytest.mark.asyncio
    async def test_close_channels(self, agent: TestableAgent):
        mock_ch = AsyncMock()
        agent._orchestrator_channel = mock_ch
        agent._tools_channel = mock_ch
        agent._memory_channel = mock_ch
        agent._runtime_channel = mock_ch
        await agent._close_channels()
        assert mock_ch.close.call_count == 4


# ---------------------------------------------------------------------------
# execute_task() wrapper tests
# ---------------------------------------------------------------------------


class TestExecuteTask:
    @pytest.mark.asyncio
    async def test_execute_task_success(self, agent: TestableAgent):
        task = {"id": "task-42", "description": "do something"}
        result = await agent.execute_task(task)

        assert result["task_id"] == "task-42"
        assert result["success"] is True
        assert result["output_json"]["handled"] is True
        assert result["duration_ms"] >= 0
        assert agent._tasks_completed == 1
        assert agent._current_task_id is None  # cleaned up

    @pytest.mark.asyncio
    async def test_execute_task_auto_generates_id(self, agent: TestableAgent):
        task = {"description": "no id"}
        result = await agent.execute_task(task)
        assert result["task_id"]  # non-empty
        assert result["success"] is True

    @pytest.mark.asyncio
    async def test_execute_task_handles_exception(self, agent: TestableAgent):
        async def exploding_handler(task):
            raise ValueError("boom")

        agent.handle_task = exploding_handler
        result = await agent.execute_task({"id": "t1", "description": "fail"})

        assert result["success"] is False
        assert "boom" in result["error"]
        assert agent._tasks_failed == 1
        assert agent._current_task_id is None

    @pytest.mark.asyncio
    async def test_execute_task_deserializes_input_json_string(self, agent: TestableAgent):
        task = {
            "id": "t2",
            "description": "with input",
            "input_json": '{"key": "value"}',
        }
        await agent.execute_task(task)
        assert agent._last_task["input_json"] == {"key": "value"}

    @pytest.mark.asyncio
    async def test_execute_task_deserializes_input_json_bytes(self, agent: TestableAgent):
        task = {
            "id": "t3",
            "description": "with bytes",
            "input_json": b'{"key": "value"}',
        }
        await agent.execute_task(task)
        assert agent._last_task["input_json"] == {"key": "value"}

    @pytest.mark.asyncio
    async def test_execute_task_handles_bad_input_json(self, agent: TestableAgent):
        task = {
            "id": "t4",
            "description": "bad json",
            "input_json": "not{json",
        }
        await agent.execute_task(task)
        assert agent._last_task["input_json"] == {}


# ---------------------------------------------------------------------------
# Proto JSON helpers tests
# ---------------------------------------------------------------------------


class TestProtoHelpers:
    def test_encode_proto_json(self):
        data = {"key": "value", "num": 42}
        result = BaseAgent._encode_proto_json(data)
        assert isinstance(result, bytes)
        decoded = json.loads(result)
        assert decoded["key"] == "value"
        assert decoded["num"] == 42

    def test_decode_proto_json(self):
        raw = json.dumps({"hello": "world"}).encode()
        result = BaseAgent._decode_proto_json(raw)
        assert result == {"hello": "world"}

    def test_decode_proto_json_empty(self):
        result = BaseAgent._decode_proto_json(b"")
        assert result == {}

    def test_decode_proto_json_invalid(self):
        result = BaseAgent._decode_proto_json(b"\x00\x01\x02")
        assert "_raw" in result


# ---------------------------------------------------------------------------
# gRPC retry tests
# ---------------------------------------------------------------------------


class TestGrpcRetry:
    @pytest.mark.asyncio
    async def test_grpc_call_retries_on_unavailable(self, agent: TestableAgent):
        import grpc

        channel = MagicMock()
        call_count = 0

        async def call_fn(request_data, timeout=None):
            nonlocal call_count
            call_count += 1
            if call_count < 2:
                error = grpc.aio.AioRpcError(
                    grpc.StatusCode.UNAVAILABLE,
                    initial_metadata=None,
                    trailing_metadata=None,
                    details="unavailable",
                    debug_error_string=None,
                )
                raise error
            return b'{"ok": true}'

        channel.unary_unary.return_value = call_fn
        agent.config.max_retries = 3
        agent.config.retry_delay_s = 0.01

        result = await agent._grpc_call(channel, "svc", "method", b"data")
        assert json.loads(result) == {"ok": True}
        assert call_count == 2


# ---------------------------------------------------------------------------
# IntelligenceLevel enum tests
# ---------------------------------------------------------------------------


class TestIntelligenceLevel:
    def test_values(self):
        assert IntelligenceLevel.REACTIVE.value == "reactive"
        assert IntelligenceLevel.OPERATIONAL.value == "operational"
        assert IntelligenceLevel.TACTICAL.value == "tactical"
        assert IntelligenceLevel.STRATEGIC.value == "strategic"

    def test_string_conversion(self):
        level = IntelligenceLevel("tactical")
        assert level == IntelligenceLevel.TACTICAL
