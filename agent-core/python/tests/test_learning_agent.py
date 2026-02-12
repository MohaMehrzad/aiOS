"""
Tests for LearningAgent -- pattern analysis, parameter optimization,
tool effectiveness tracking, performance analysis, and improvement suggestions.
"""

from __future__ import annotations

import json
import math
from typing import Any
from unittest.mock import AsyncMock, patch

import pytest

from aios_agent.agents.learning import IMPROVEMENT_CONFIDENCE_THRESHOLD, LearningAgent
from aios_agent.base import AgentConfig


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def config() -> AgentConfig:
    return AgentConfig(max_retries=1, retry_delay_s=0.01, grpc_timeout_s=2.0)


@pytest.fixture
def agent(config: AgentConfig) -> LearningAgent:
    return LearningAgent(agent_id="learning-test-001", config=config)


# ---------------------------------------------------------------------------
# Basics
# ---------------------------------------------------------------------------


class TestLearningAgentBasics:
    def test_agent_type(self, agent: LearningAgent):
        assert agent.get_agent_type() == "learning"

    def test_capabilities(self, agent: LearningAgent):
        caps = agent.get_capabilities()
        assert "learning.analyze_patterns" in caps
        assert "learning.optimize_parameters" in caps
        assert "learning.tool_effectiveness" in caps

    def test_initial_caches_empty(self, agent: LearningAgent):
        assert agent._decision_cache == []
        assert agent._pattern_cache == {}
        assert agent._performance_history == {}


# ---------------------------------------------------------------------------
# Task dispatch
# ---------------------------------------------------------------------------


class TestLearningTaskDispatch:
    @pytest.mark.asyncio
    async def test_pattern_keyword(self, agent: LearningAgent):
        with patch.object(agent, "_analyze_patterns", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "analyze patterns in events"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_optimize_keyword(self, agent: LearningAgent):
        with patch.object(agent, "_optimize_parameters", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "optimize system parameters"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_suggest_keyword(self, agent: LearningAgent):
        with patch.object(agent, "_suggest_improvements", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "suggest improvements"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_tool_effectiveness_keyword(self, agent: LearningAgent):
        with patch.object(agent, "_tool_effectiveness", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "tool effectiveness analysis"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_performance_keyword(self, agent: LearningAgent):
        with patch.object(agent, "_performance_analysis", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "show performance trends"})
        m.assert_awaited_once()


# ---------------------------------------------------------------------------
# Pattern analysis
# ---------------------------------------------------------------------------


class TestPatternAnalysis:
    @pytest.mark.asyncio
    async def test_discovers_recurring_patterns(self, agent: LearningAgent):
        # Create events with a pattern: "system.health" trigger -> "restart" action
        events = [
            {
                "category": "system.health",
                "data_json": json.dumps({"action": "restart", "success": True}),
                "timestamp": 1000 + i,
            }
            for i in range(5)
        ]

        with patch.object(agent, "get_recent_events", new_callable=AsyncMock,
                          return_value=events), \
             patch.object(agent, "assemble_context", new_callable=AsyncMock,
                          return_value=[]), \
             patch.object(agent, "store_pattern", new_callable=AsyncMock,
                          return_value="pat-001"), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="Good patterns found"), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._analyze_patterns({"min_occurrences": 3})

        assert result["success"] is True
        assert result["patterns_discovered"] >= 1
        # Find the system.health -> restart pattern
        health_patterns = [p for p in result["patterns"]
                           if p["trigger"] == "system.health" and p["action"] == "restart"]
        assert len(health_patterns) == 1
        assert health_patterns[0]["occurrences"] == 5
        assert health_patterns[0]["success_rate"] == 1.0

    @pytest.mark.asyncio
    async def test_low_occurrence_filtered_out(self, agent: LearningAgent):
        events = [
            {
                "category": "rare.event",
                "data_json": json.dumps({"action": "something", "success": True}),
                "timestamp": 1000,
            }
        ]

        with patch.object(agent, "get_recent_events", new_callable=AsyncMock,
                          return_value=events), \
             patch.object(agent, "assemble_context", new_callable=AsyncMock,
                          return_value=[]), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._analyze_patterns({"min_occurrences": 3})

        assert result["patterns_discovered"] == 0

    @pytest.mark.asyncio
    async def test_high_confidence_patterns_stored(self, agent: LearningAgent):
        # Many occurrences + high success = high confidence
        events = [
            {
                "category": "network.issue",
                "data_json": json.dumps({"action": "restart_interface", "success": True}),
                "timestamp": 1000 + i,
            }
            for i in range(20)
        ]

        stored_ids = []

        async def _track_store(trigger, action, success_rate, created_from=""):
            stored_ids.append(f"{trigger}->{action}")
            return "pat-stored"

        with patch.object(agent, "get_recent_events", new_callable=AsyncMock,
                          return_value=events), \
             patch.object(agent, "assemble_context", new_callable=AsyncMock,
                          return_value=[]), \
             patch.object(agent, "store_pattern", side_effect=_track_store), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value="Good"), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._analyze_patterns({"min_occurrences": 3})

        assert result["patterns_stored"] > 0
        assert len(stored_ids) > 0

    @pytest.mark.asyncio
    async def test_confidence_calculation(self, agent: LearningAgent):
        # 10 occurrences, 80% success -> confidence = min(1.0, 10/20 * 0.8) = 0.4
        events = []
        for i in range(10):
            events.append({
                "category": "test.trigger",
                "data_json": json.dumps({
                    "action": "test_action",
                    "success": True if i < 8 else False,
                }),
                "timestamp": i,
            })

        with patch.object(agent, "get_recent_events", new_callable=AsyncMock,
                          return_value=events), \
             patch.object(agent, "assemble_context", new_callable=AsyncMock,
                          return_value=[]), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._analyze_patterns({"min_occurrences": 3})

        pattern = [p for p in result["patterns"]
                   if p["trigger"] == "test.trigger"][0]
        assert pattern["success_rate"] == 0.8
        # confidence = min(1.0, 10/20 * 0.8) = 0.4
        assert abs(pattern["confidence"] - 0.4) < 0.01

    @pytest.mark.asyncio
    async def test_empty_events(self, agent: LearningAgent):
        with patch.object(agent, "get_recent_events", new_callable=AsyncMock,
                          return_value=[]), \
             patch.object(agent, "assemble_context", new_callable=AsyncMock,
                          return_value=[]), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._analyze_patterns({})

        assert result["patterns_discovered"] == 0
        assert result["events_analyzed"] == 0


# ---------------------------------------------------------------------------
# Parameter optimization
# ---------------------------------------------------------------------------


class TestOptimizeParameters:
    @pytest.mark.asyncio
    async def test_collects_performance_data(self, agent: LearningAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "parameters": {"vm.swappiness": 60, "net.core.somaxconn": 128}
            }}

        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=55.0), \
             patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value='[{"parameter": "vm.swappiness", "suggested_value": 30, "expected_impact": "lower swap usage"}]'):
            result = await agent._optimize_parameters({
                "target_metrics": ["cpu.usage_percent"],
            })

        assert result["success"] is True
        assert result["metrics_analyzed"] == 1
        assert len(result["suggestions"]) == 1
        assert result["suggestions"][0]["parameter"] == "vm.swappiness"

    @pytest.mark.asyncio
    async def test_auto_apply_parameter(self, agent: LearningAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "system.get_tunable_params":
                return {"success": True, "output": {"parameters": {"vm.swappiness": 60}}}
            if name == "system.set_tunable_param":
                return {"success": True}
            return {"success": True, "output": {}}

        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=70.0), \
             patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value='[{"parameter": "vm.swappiness", "suggested_value": 30, "expected_impact": "better"}]'), \
             patch.object(agent, "store_decision", new_callable=AsyncMock):
            result = await agent._optimize_parameters({
                "target_metrics": ["cpu.usage_percent"],
                "auto_apply": True,
            })

        assert len(result["applied"]) == 1

    @pytest.mark.asyncio
    async def test_invalid_ai_json_fallback(self, agent: LearningAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {"parameters": {}}}

        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=50.0), \
             patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="Just some text, not JSON"):
            result = await agent._optimize_parameters({
                "target_metrics": ["cpu.usage_percent"],
            })

        # Should create a fallback suggestion
        assert result["success"] is True
        assert len(result["suggestions"]) == 1
        assert result["suggestions"][0]["parameter"] == "review_needed"

    @pytest.mark.asyncio
    async def test_performance_history_bounded(self, agent: LearningAgent):
        agent._performance_history["cpu.usage_percent"] = list(range(200))

        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {"parameters": {}}}

        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=55.0), \
             patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value="[]"):
            await agent._optimize_parameters({"target_metrics": ["cpu.usage_percent"]})

        assert len(agent._performance_history["cpu.usage_percent"]) == 200


# ---------------------------------------------------------------------------
# Tool effectiveness tracking
# ---------------------------------------------------------------------------


class TestToolEffectiveness:
    @pytest.mark.asyncio
    async def test_tool_stats_aggregation(self, agent: LearningAgent):
        events = [
            {
                "category": "tool_call",
                "data_json": json.dumps({
                    "tool": "system.metrics",
                    "success": True,
                    "duration_ms": 50,
                }),
            },
            {
                "category": "tool_call",
                "data_json": json.dumps({
                    "tool": "system.metrics",
                    "success": True,
                    "duration_ms": 100,
                }),
            },
            {
                "category": "tool_call",
                "data_json": json.dumps({
                    "tool": "system.metrics",
                    "success": False,
                    "duration_ms": 200,
                }),
            },
            {
                "category": "tool_call",
                "data_json": json.dumps({
                    "tool": "network.ping",
                    "success": True,
                    "duration_ms": 30,
                }),
            },
        ]

        with patch.object(agent, "get_recent_events", new_callable=AsyncMock,
                          return_value=events), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._tool_effectiveness({})

        assert result["success"] is True
        assert result["tools_analyzed"] == 2

        # Find system.metrics stats
        sys_tool = [t for t in result["tools"] if t["tool"] == "system.metrics"][0]
        assert sys_tool["total_calls"] == 3
        assert abs(sys_tool["success_rate"] - 0.667) < 0.01
        assert sys_tool["avg_duration_ms"] > 0

    @pytest.mark.asyncio
    async def test_underperforming_tools_identified(self, agent: LearningAgent):
        # Tool with low success rate and enough calls
        events = []
        for i in range(10):
            events.append({
                "category": "tool_call",
                "data_json": json.dumps({
                    "tool": "bad.tool",
                    "success": i < 3,  # Only 30% success
                    "duration_ms": 100,
                }),
            })

        with patch.object(agent, "get_recent_events", new_callable=AsyncMock,
                          return_value=events), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._tool_effectiveness({})

        assert len(result["underperforming"]) == 1
        assert result["underperforming"][0]["tool"] == "bad.tool"

    @pytest.mark.asyncio
    async def test_no_events(self, agent: LearningAgent):
        with patch.object(agent, "get_recent_events", new_callable=AsyncMock,
                          return_value=[]), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._tool_effectiveness({})

        assert result["tools_analyzed"] == 0
        assert result["underperforming"] == []

    @pytest.mark.asyncio
    async def test_p95_duration(self, agent: LearningAgent):
        events = []
        for i in range(20):
            events.append({
                "category": "tool_call",
                "data_json": json.dumps({
                    "tool": "fast.tool",
                    "success": True,
                    "duration_ms": 10 + i,  # 10-29ms
                }),
            })

        with patch.object(agent, "get_recent_events", new_callable=AsyncMock,
                          return_value=events), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._tool_effectiveness({})

        tool = [t for t in result["tools"] if t["tool"] == "fast.tool"][0]
        assert tool["p95_duration_ms"] > tool["avg_duration_ms"]


# ---------------------------------------------------------------------------
# Performance analysis
# ---------------------------------------------------------------------------


class TestPerformanceAnalysis:
    @pytest.mark.asyncio
    async def test_stable_metrics(self, agent: LearningAgent):
        agent._performance_history["cpu.usage_percent"] = [50.0] * 20

        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=50.0):
            result = await agent._performance_analysis({
                "metrics": ["cpu.usage_percent"],
            })

        assert result["success"] is True
        analysis = result["metrics_analysis"]["cpu.usage_percent"]
        assert analysis["trend"] == "stable"
        assert analysis["health"] == "good"
        assert analysis["current"] == 50.0

    @pytest.mark.asyncio
    async def test_increasing_trend(self, agent: LearningAgent):
        values = [50.0 + i * 2 for i in range(20)]
        agent._performance_history["cpu.usage_percent"] = values

        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=values[-1]):
            result = await agent._performance_analysis({
                "metrics": ["cpu.usage_percent"],
            })

        analysis = result["metrics_analysis"]["cpu.usage_percent"]
        assert analysis["trend"] == "increasing"

    @pytest.mark.asyncio
    async def test_critical_health(self, agent: LearningAgent):
        agent._performance_history["memory.usage_percent"] = [95.0] * 20

        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=95.0):
            result = await agent._performance_analysis({
                "metrics": ["memory.usage_percent"],
            })

        analysis = result["metrics_analysis"]["memory.usage_percent"]
        assert analysis["health"] == "critical"

    @pytest.mark.asyncio
    async def test_warning_health(self, agent: LearningAgent):
        agent._performance_history["cpu.usage_percent"] = [80.0] * 20

        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=80.0):
            result = await agent._performance_analysis({
                "metrics": ["cpu.usage_percent"],
            })

        analysis = result["metrics_analysis"]["cpu.usage_percent"]
        assert analysis["health"] == "warning"

    @pytest.mark.asyncio
    async def test_overall_health_score(self, agent: LearningAgent):
        agent._performance_history["cpu.usage_percent"] = [50.0] * 20
        agent._performance_history["memory.usage_percent"] = [95.0] * 20

        with patch.object(agent, "get_metric", new_callable=AsyncMock,
                          side_effect=[50.0, 95.0]):
            result = await agent._performance_analysis({
                "metrics": ["cpu.usage_percent", "memory.usage_percent"],
            })

        # good=100, critical=20  -> (100+20)/2 = 60
        assert result["overall_health_score"] == 60.0

    @pytest.mark.asyncio
    async def test_insufficient_data(self, agent: LearningAgent):
        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=None):
            result = await agent._performance_analysis({
                "metrics": ["missing.metric"],
            })

        assert result["metrics_analysis"]["missing.metric"]["data"] == "insufficient"

    @pytest.mark.asyncio
    async def test_load_health_assessment(self, agent: LearningAgent):
        agent._performance_history["load.1m"] = [5.0] * 20

        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=5.0):
            result = await agent._performance_analysis({"metrics": ["load.1m"]})

        assert result["metrics_analysis"]["load.1m"]["health"] == "critical"


# ---------------------------------------------------------------------------
# Improvement suggestions (integration)
# ---------------------------------------------------------------------------


class TestSuggestImprovements:
    @pytest.mark.asyncio
    async def test_combines_all_data_sources(self, agent: LearningAgent):
        with patch.object(agent, "_analyze_patterns", new_callable=AsyncMock,
                          return_value={"patterns_discovered": 3, "patterns": [
                              {"trigger": "t", "action": "a", "success_rate": 0.9}
                          ]}), \
             patch.object(agent, "_tool_effectiveness", new_callable=AsyncMock,
                          return_value={"tools": [
                              {"tool": "system.metrics", "success_rate": 0.95, "avg_duration_ms": 50}
                          ]}), \
             patch.object(agent, "_performance_analysis", new_callable=AsyncMock,
                          return_value={"metrics_analysis": {
                              "cpu.usage_percent": {"trend": "stable", "health": "good"}
                          }}), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value=(
                              "Operational Improvements:\n"
                              "1. Increase health check frequency\n"
                              "Configuration Changes:\n"
                              "1. Tune vm.swappiness\n"
                              "Automation Opportunities:\n"
                              "1. Auto-restart failing services\n"
                              "Watch Items:\n"
                              "1. Disk usage trending up"
                          )), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._suggest_improvements({})

        assert result["success"] is True
        assert len(result["suggestions"]["operational_improvements"]) >= 1
        assert len(result["suggestions"]["configuration_changes"]) >= 1
        assert result["data_sources"]["patterns_analyzed"] == 3
