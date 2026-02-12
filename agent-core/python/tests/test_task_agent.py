"""
Tests for TaskAgent -- goal decomposition, plan creation, execution, and delegation.

Covers task dispatch, plan generation via AI, plan execution with dependency
ordering, error handling, and the plan-and-execute flow.
"""

from __future__ import annotations

import json
from typing import Any
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from aios_agent.agents.task import MAX_PLAN_STEPS, TaskAgent
from aios_agent.base import AgentConfig


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def config() -> AgentConfig:
    return AgentConfig(max_retries=1, retry_delay_s=0.01, grpc_timeout_s=2.0)


@pytest.fixture
def agent(config: AgentConfig) -> TaskAgent:
    return TaskAgent(agent_id="task-test-001", config=config)


# ---------------------------------------------------------------------------
# Agent basics
# ---------------------------------------------------------------------------


class TestTaskAgentBasics:
    def test_agent_type(self, agent: TaskAgent):
        assert agent.get_agent_type() == "task"

    def test_capabilities(self, agent: TaskAgent):
        caps = agent.get_capabilities()
        assert "task.plan" in caps
        assert "task.execute" in caps
        assert "task.delegate" in caps
        assert "task.decompose" in caps


# ---------------------------------------------------------------------------
# Task dispatch
# ---------------------------------------------------------------------------


class TestTaskDispatch:
    @pytest.mark.asyncio
    async def test_plan_keyword_dispatches_create_plan(self, agent: TaskAgent):
        with patch.object(agent, "_create_plan", new_callable=AsyncMock,
                          return_value={"steps": []}) as mock:
            await agent.handle_task({"description": "plan how to install nginx"})
        mock.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_decompose_keyword_dispatches_create_plan(self, agent: TaskAgent):
        with patch.object(agent, "_create_plan", new_callable=AsyncMock,
                          return_value={"steps": []}) as mock:
            await agent.handle_task({"description": "decompose goal into steps"})
        mock.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_execute_plan_keyword(self, agent: TaskAgent):
        with patch.object(agent, "_execute_plan", new_callable=AsyncMock,
                          return_value={"success": True}) as mock:
            await agent.handle_task({
                "description": "execute plan for deploy",
                "input_json": {"plan": [{"id": "s1"}]},
            })
        mock.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_delegate_keyword(self, agent: TaskAgent):
        with patch.object(agent, "_delegate_subtask", new_callable=AsyncMock,
                          return_value={"success": True}) as mock:
            await agent.handle_task({"description": "delegate work to system agent"})
        mock.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_default_dispatches_plan_and_execute(self, agent: TaskAgent):
        with patch.object(agent, "_plan_and_execute", new_callable=AsyncMock,
                          return_value={"success": True}) as mock:
            await agent.handle_task({"description": "install nginx and configure it"})
        mock.assert_awaited_once()


# ---------------------------------------------------------------------------
# Plan creation
# ---------------------------------------------------------------------------


class TestCreatePlan:
    @pytest.mark.asyncio
    async def test_creates_plan_from_ai_response(self, agent: TaskAgent):
        ai_plan = json.dumps([
            {"id": "s1", "description": "Install package", "agent_type": "package",
             "tool": "package.install", "input": {"package": "nginx"},
             "depends_on": [], "can_fail": False},
            {"id": "s2", "description": "Configure nginx", "agent_type": "system",
             "tool": "", "input": {}, "depends_on": ["s1"], "can_fail": False},
        ])

        with patch.object(agent, "semantic_search", new_callable=AsyncMock, return_value=[]), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value=ai_plan), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._create_plan({"description": "install and configure nginx"})

        assert result["step_count"] == 2
        steps = result["steps"]
        assert steps[0]["id"] == "s1"
        assert steps[1]["depends_on"] == ["s1"]
        assert all(s["status"] == "pending" for s in steps)

    @pytest.mark.asyncio
    async def test_plan_handles_invalid_json(self, agent: TaskAgent):
        with patch.object(agent, "semantic_search", new_callable=AsyncMock, return_value=[]), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value="not json at all"), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._create_plan({"description": "do something"})

        # Falls back to a single generic step
        assert result["step_count"] == 1
        assert result["steps"][0]["id"] == "step_1"

    @pytest.mark.asyncio
    async def test_plan_extracts_json_from_markdown(self, agent: TaskAgent):
        ai_response = '```json\n[{"id":"s1","description":"step","agent_type":"system","tool":"","input":{},"depends_on":[],"can_fail":false}]\n```'
        with patch.object(agent, "semantic_search", new_callable=AsyncMock, return_value=[]), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value=ai_response), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._create_plan({"description": "do something"})

        assert result["step_count"] == 1

    @pytest.mark.asyncio
    async def test_plan_limits_steps(self, agent: TaskAgent):
        # Create a plan with more than MAX_PLAN_STEPS
        steps = [
            {"id": f"s{i}", "description": f"step {i}", "agent_type": "system",
             "tool": "", "input": {}, "depends_on": [], "can_fail": False}
            for i in range(30)
        ]
        with patch.object(agent, "semantic_search", new_callable=AsyncMock, return_value=[]), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value=json.dumps(steps)), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._create_plan({"description": "big plan"})

        assert result["step_count"] <= MAX_PLAN_STEPS

    @pytest.mark.asyncio
    async def test_plan_deduplicates_step_ids(self, agent: TaskAgent):
        steps = [
            {"id": "dup", "description": "first", "agent_type": "system",
             "tool": "", "input": {}, "depends_on": [], "can_fail": False},
            {"id": "dup", "description": "second", "agent_type": "system",
             "tool": "", "input": {}, "depends_on": [], "can_fail": False},
        ]
        with patch.object(agent, "semantic_search", new_callable=AsyncMock, return_value=[]), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value=json.dumps(steps)), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._create_plan({"description": "dup ids"})

        ids = [s["id"] for s in result["steps"]]
        assert len(set(ids)) == 2  # IDs were deduplicated


# ---------------------------------------------------------------------------
# Plan execution
# ---------------------------------------------------------------------------


class TestExecutePlan:
    @pytest.mark.asyncio
    async def test_executes_steps_in_order(self, agent: TaskAgent):
        steps = [
            {"id": "s1", "description": "first", "tool": "t1", "input": {},
             "depends_on": [], "can_fail": False},
            {"id": "s2", "description": "second", "tool": "t2", "input": {},
             "depends_on": ["s1"], "can_fail": False},
        ]

        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {"tool": name}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._execute_plan(steps, {"description": "test"})

        assert result["success"] is True
        assert result["steps_completed"] == 2
        assert result["steps_failed"] == 0
        # s1 must come before s2
        assert result["execution_order"].index("s1") < result["execution_order"].index("s2")

    @pytest.mark.asyncio
    async def test_dependency_failure_cascades(self, agent: TaskAgent):
        steps = [
            {"id": "s1", "description": "fail", "tool": "t1", "input": {},
             "depends_on": [], "can_fail": False},
            {"id": "s2", "description": "depends on s1", "tool": "t2", "input": {},
             "depends_on": ["s1"], "can_fail": False},
        ]

        async def _fail_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": False, "error": "broken"}

        with patch.object(agent, "call_tool", side_effect=_fail_tool), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._execute_plan(steps, {"description": "test"})

        assert result["success"] is False
        assert result["steps_failed"] == 2

    @pytest.mark.asyncio
    async def test_can_fail_step_continues(self, agent: TaskAgent):
        steps = [
            {"id": "s1", "description": "optional", "tool": "t1", "input": {},
             "depends_on": [], "can_fail": True},
            {"id": "s2", "description": "runs anyway", "tool": "t2", "input": {},
             "depends_on": ["s1"], "can_fail": False},
        ]

        call_count = 0

        async def _mixed_tool(name, input_json=None, *, reason="", task_id=None):
            nonlocal call_count
            call_count += 1
            if name == "t1":
                return {"success": False, "error": "optional fail"}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_mixed_tool), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._execute_plan(steps, {"description": "test"})

        # s1 failed but can_fail=True, so s2 should still run
        assert result["steps_completed"] == 2  # Both completed (s1 as failed-but-continued)

    @pytest.mark.asyncio
    async def test_parallel_independent_steps(self, agent: TaskAgent):
        steps = [
            {"id": "s1", "description": "a", "tool": "t1", "input": {},
             "depends_on": [], "can_fail": False},
            {"id": "s2", "description": "b", "tool": "t2", "input": {},
             "depends_on": [], "can_fail": False},
        ]

        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._execute_plan(steps, {"description": "test"})

        assert result["success"] is True
        assert set(result["execution_order"]) == {"s1", "s2"}

    @pytest.mark.asyncio
    async def test_empty_plan(self, agent: TaskAgent):
        with patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._execute_plan([], {"description": "empty"})

        assert result["success"] is True
        assert result["steps_total"] == 0


# ---------------------------------------------------------------------------
# Delegation
# ---------------------------------------------------------------------------


class TestDelegation:
    @pytest.mark.asyncio
    async def test_delegate_submits_goal(self, agent: TaskAgent):
        mock_client = MagicMock()
        mock_client.submit_goal = AsyncMock(return_value="goal-xyz")
        mock_client.wait_for_goal = AsyncMock(return_value={
            "goal": {"status": "completed"},
            "progress_percent": 100.0,
            "tasks": [],
        })
        mock_client.__aenter__ = AsyncMock(return_value=mock_client)
        mock_client.__aexit__ = AsyncMock(return_value=None)

        with patch("aios_agent.agents.task.OrchestratorClient", return_value=mock_client):
            result = await agent._delegate_subtask(
                {"description": "install nginx", "agent_type": "package"},
                {"id": "parent-task"},
            )

        assert result["success"] is True
        assert result["goal_id"] == "goal-xyz"
        mock_client.submit_goal.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_delegate_timeout(self, agent: TaskAgent):
        mock_client = MagicMock()
        mock_client.submit_goal = AsyncMock(return_value="goal-slow")
        mock_client.wait_for_goal = AsyncMock(side_effect=TimeoutError("timed out"))
        mock_client.__aenter__ = AsyncMock(return_value=mock_client)
        mock_client.__aexit__ = AsyncMock(return_value=None)

        with patch("aios_agent.agents.task.OrchestratorClient", return_value=mock_client):
            result = await agent._delegate_subtask(
                {"description": "slow task"},
                {"id": "parent"},
            )

        assert result["success"] is False
        assert "timed out" in result["error"]


# ---------------------------------------------------------------------------
# Plan-and-execute combined flow
# ---------------------------------------------------------------------------


class TestPlanAndExecute:
    @pytest.mark.asyncio
    async def test_plan_and_execute_success(self, agent: TaskAgent):
        plan_result = {
            "plan_id": "p1",
            "goal": "test",
            "steps": [
                {"id": "s1", "description": "step", "tool": "t1", "input": {},
                 "depends_on": [], "can_fail": False, "status": "pending", "result": None},
            ],
            "step_count": 1,
        }

        with patch.object(agent, "_create_plan", new_callable=AsyncMock, return_value=plan_result), \
             patch.object(agent, "_execute_plan", new_callable=AsyncMock,
                          return_value={"success": True, "steps_completed": 1}), \
             patch.object(agent, "store_decision", new_callable=AsyncMock):
            result = await agent._plan_and_execute({"description": "full flow"})

        assert result["success"] is True
        assert "plan" in result
        assert "execution" in result

    @pytest.mark.asyncio
    async def test_plan_and_execute_empty_plan(self, agent: TaskAgent):
        with patch.object(agent, "_create_plan", new_callable=AsyncMock,
                          return_value={"steps": [], "step_count": 0}):
            result = await agent._plan_and_execute({"description": "nothing"})

        assert result["success"] is False
        assert "Failed to create" in result["error"]


# ---------------------------------------------------------------------------
# _execute_single_step tests
# ---------------------------------------------------------------------------


class TestExecuteSingleStep:
    @pytest.mark.asyncio
    async def test_step_with_tool(self, agent: TaskAgent):
        step = {"id": "s1", "tool": "my.tool", "input": {"a": 1}, "depends_on": [], "description": "test"}

        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {"done": True}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._execute_single_step(step, {})

        assert result["success"] is True

    @pytest.mark.asyncio
    async def test_step_without_tool_delegates(self, agent: TaskAgent):
        step = {"id": "s1", "tool": "", "input": {}, "depends_on": [],
                "description": "do something", "agent_type": "network"}

        with patch.object(agent, "_delegate_subtask", new_callable=AsyncMock,
                          return_value={"success": True}):
            result = await agent._execute_single_step(step, {})

        assert result["success"] is True

    @pytest.mark.asyncio
    async def test_step_injects_dependency_outputs(self, agent: TaskAgent):
        step = {"id": "s2", "tool": "t", "input": {"base": 1},
                "depends_on": ["s1"], "description": "second"}
        completed = {"s1": {"output": {"from_s1": "data"}}}
        captured_input = {}

        async def _capture_tool(name, input_json=None, *, reason="", task_id=None):
            captured_input.update(input_json or {})
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_capture_tool):
            await agent._execute_single_step(step, completed)

        assert captured_input["base"] == 1
        assert captured_input["_dep_s1"] == {"from_s1": "data"}
