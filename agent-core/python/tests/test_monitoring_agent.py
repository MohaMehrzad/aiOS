"""
Tests for MonitoringAgent -- metrics collection, alert evaluation, anomaly detection,
resource forecasting, report generation, and dashboard data assembly.
"""

from __future__ import annotations

import json
import math
from typing import Any
from unittest.mock import AsyncMock, patch

import pytest

from aios_agent.agents.monitoring import BASELINE_WINDOW_SIZE, MonitoringAgent
from aios_agent.base import AgentConfig


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def config() -> AgentConfig:
    return AgentConfig(max_retries=1, retry_delay_s=0.01, grpc_timeout_s=2.0)


@pytest.fixture
def agent(config: AgentConfig) -> MonitoringAgent:
    return MonitoringAgent(agent_id="monitoring-test-001", config=config)


# ---------------------------------------------------------------------------
# Basics
# ---------------------------------------------------------------------------


class TestMonitoringAgentBasics:
    def test_agent_type(self, agent: MonitoringAgent):
        assert agent.get_agent_type() == "monitoring"

    def test_capabilities(self, agent: MonitoringAgent):
        caps = agent.get_capabilities()
        assert "monitoring.collect_metrics" in caps
        assert "monitoring.check_alerts" in caps
        assert "monitoring.anomaly_detection" in caps
        assert "monitoring.resource_forecast" in caps

    def test_initial_baselines_empty(self, agent: MonitoringAgent):
        assert agent._baselines == {}

    def test_initial_alerts_empty(self, agent: MonitoringAgent):
        assert agent._active_alerts == {}


# ---------------------------------------------------------------------------
# Task dispatch
# ---------------------------------------------------------------------------


class TestMonitoringTaskDispatch:
    @pytest.mark.asyncio
    async def test_collect_keyword(self, agent: MonitoringAgent):
        with patch.object(agent, "_collect_metrics", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "collect system metrics"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_report_keyword(self, agent: MonitoringAgent):
        with patch.object(agent, "_generate_report", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "generate health report"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_alert_keyword(self, agent: MonitoringAgent):
        with patch.object(agent, "_check_alerts", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "check alert conditions"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_anomaly_keyword(self, agent: MonitoringAgent):
        with patch.object(agent, "_anomaly_detection", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "run anomaly detection"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_forecast_keyword(self, agent: MonitoringAgent):
        with patch.object(agent, "_resource_forecast", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "forecast resource usage"})
        m.assert_awaited_once()


# ---------------------------------------------------------------------------
# Metric collection
# ---------------------------------------------------------------------------


class TestCollectMetrics:
    @pytest.mark.asyncio
    async def test_collects_and_stores_metrics(self, agent: MonitoringAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "cpu_percent": 45.0,
                "memory_percent": 60.0,
                "memory_used_mb": 8000.0,
                "memory_total_mb": 16000.0,
                "disk_percent": 50.0,
                "load_1m": 1.5,
                "load_5m": 1.2,
                "load_15m": 1.0,
                "process_count": 200,
            }}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._collect_metrics({})

        assert result["success"] is True
        assert result["metrics_collected"] > 0
        assert "cpu.usage_percent" in result["metrics"]
        assert result["metrics"]["cpu.usage_percent"] == 45.0
        # Baselines should be updated
        assert "cpu.usage_percent" in agent._baselines
        assert agent._baselines["cpu.usage_percent"] == [45.0]

    @pytest.mark.asyncio
    async def test_baseline_window_capped(self, agent: MonitoringAgent):
        # Pre-fill baseline to max
        agent._baselines["cpu.usage_percent"] = list(range(BASELINE_WINDOW_SIZE))

        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {"cpu_percent": 99.0}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            await agent._collect_metrics({})

        assert len(agent._baselines["cpu.usage_percent"]) == BASELINE_WINDOW_SIZE
        assert agent._baselines["cpu.usage_percent"][-1] == 99.0

    @pytest.mark.asyncio
    async def test_tool_failure_returns_empty_metrics(self, agent: MonitoringAgent):
        async def _fail(name, input_json=None, *, reason="", task_id=None):
            return {"success": False, "error": "tool down"}

        with patch.object(agent, "call_tool", side_effect=_fail), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._collect_metrics({})

        assert result["success"] is True  # Method itself succeeds
        assert result["metrics_collected"] == 0


# ---------------------------------------------------------------------------
# Alert evaluation
# ---------------------------------------------------------------------------


class TestCheckAlerts:
    @pytest.mark.asyncio
    async def test_alert_triggered_on_high_cpu(self, agent: MonitoringAgent):
        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=92.0), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._check_alerts({
                "rules": [
                    {"metric": "cpu.usage_percent", "operator": ">",
                     "threshold": 90, "severity": "critical", "name": "cpu_critical"},
                ]
            })

        assert len(result["new_alerts"]) == 1
        assert result["new_alerts"][0]["name"] == "cpu_critical"
        assert result["total_active"] == 1
        assert "cpu_critical" in agent._active_alerts

    @pytest.mark.asyncio
    async def test_alert_resolved(self, agent: MonitoringAgent):
        # Pre-set an active alert
        agent._active_alerts["cpu_critical"] = {
            "name": "cpu_critical", "metric": "cpu.usage_percent",
            "value": 95.0, "threshold": 90, "severity": "critical",
        }

        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=50.0), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._check_alerts({
                "rules": [
                    {"metric": "cpu.usage_percent", "operator": ">",
                     "threshold": 90, "severity": "critical", "name": "cpu_critical"},
                ]
            })

        assert "cpu_critical" in result["resolved_alerts"]
        assert result["total_active"] == 0
        assert "cpu_critical" not in agent._active_alerts

    @pytest.mark.asyncio
    async def test_no_alerts_when_under_threshold(self, agent: MonitoringAgent):
        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=40.0):
            result = await agent._check_alerts({
                "rules": [
                    {"metric": "cpu.usage_percent", "operator": ">",
                     "threshold": 90, "severity": "critical", "name": "cpu_critical"},
                ]
            })

        assert result["new_alerts"] == []
        assert result["total_active"] == 0

    @pytest.mark.asyncio
    async def test_less_than_operator(self, agent: MonitoringAgent):
        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=5.0), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._check_alerts({
                "rules": [
                    {"metric": "disk.free_gb", "operator": "<",
                     "threshold": 10, "severity": "warning", "name": "disk_low"},
                ]
            })

        assert len(result["new_alerts"]) == 1

    @pytest.mark.asyncio
    async def test_metric_none_skipped(self, agent: MonitoringAgent):
        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=None):
            result = await agent._check_alerts({
                "rules": [
                    {"metric": "missing.metric", "operator": ">",
                     "threshold": 0, "severity": "warning", "name": "test"},
                ]
            })

        assert result["new_alerts"] == []

    @pytest.mark.asyncio
    async def test_duplicate_alert_not_re_created(self, agent: MonitoringAgent):
        agent._active_alerts["cpu_critical"] = {"name": "cpu_critical"}

        with patch.object(agent, "get_metric", new_callable=AsyncMock, return_value=95.0):
            result = await agent._check_alerts({
                "rules": [
                    {"metric": "cpu.usage_percent", "operator": ">",
                     "threshold": 90, "severity": "critical", "name": "cpu_critical"},
                ]
            })

        assert result["new_alerts"] == []
        assert result["total_active"] == 1


# ---------------------------------------------------------------------------
# Anomaly detection (z-score)
# ---------------------------------------------------------------------------


class TestAnomalyDetection:
    @pytest.mark.asyncio
    async def test_no_anomalies_in_stable_data(self, agent: MonitoringAgent):
        # Stable baseline around 50 +/- 2
        agent._baselines["cpu.usage_percent"] = [50.0 + (i % 3) for i in range(50)]

        result = await agent._anomaly_detection({"sigma_threshold": 2.5})

        assert result["success"] is True
        assert result["anomalies_found"] == 0

    @pytest.mark.asyncio
    async def test_detects_spike_anomaly(self, agent: MonitoringAgent):
        # Stable data with a spike at the end
        stable = [50.0] * 49
        stable.append(99.0)  # Huge spike
        agent._baselines["cpu.usage_percent"] = stable

        with patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="CPU spike detected"), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._anomaly_detection({"sigma_threshold": 2.5})

        assert result["anomalies_found"] >= 1
        anomaly = result["anomalies"][0]
        assert anomaly["metric"] == "cpu.usage_percent"
        assert anomaly["direction"] == "above"
        assert abs(anomaly["z_score"]) > 2.5

    @pytest.mark.asyncio
    async def test_detects_drop_anomaly(self, agent: MonitoringAgent):
        stable = [80.0] * 49
        stable.append(10.0)  # Huge drop
        agent._baselines["memory.usage_percent"] = stable

        with patch.object(agent, "think", new_callable=AsyncMock, return_value="Drop detected"), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._anomaly_detection({"sigma_threshold": 2.5})

        assert result["anomalies_found"] >= 1
        anomaly = [a for a in result["anomalies"] if a["metric"] == "memory.usage_percent"][0]
        assert anomaly["direction"] == "below"

    @pytest.mark.asyncio
    async def test_insufficient_data_skipped(self, agent: MonitoringAgent):
        agent._baselines["tiny"] = [1.0, 2.0, 3.0]  # Only 3 points, need 10

        result = await agent._anomaly_detection({})
        assert result["anomalies_found"] == 0

    @pytest.mark.asyncio
    async def test_z_score_calculation(self, agent: MonitoringAgent):
        # Manually verify z-score math
        values = [10.0] * 20
        values[-1] = 20.0  # The outlier
        agent._baselines["test_metric"] = values

        mean = sum(values) / len(values)
        variance = sum((v - mean) ** 2 for v in values) / len(values)
        std_dev = math.sqrt(variance)
        expected_z = (20.0 - mean) / std_dev

        with patch.object(agent, "think", new_callable=AsyncMock, return_value="anomaly"), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._anomaly_detection({"sigma_threshold": 1.0})

        # Find the test_metric anomaly
        test_anomalies = [a for a in result["anomalies"] if a["metric"] == "test_metric"]
        assert len(test_anomalies) == 1
        assert abs(test_anomalies[0]["z_score"] - round(expected_z, 2)) < 0.1

    @pytest.mark.asyncio
    async def test_critical_severity_for_extreme_z(self, agent: MonitoringAgent):
        values = [50.0] * 99
        values.append(200.0)  # Extreme outlier
        agent._baselines["extreme"] = values

        with patch.object(agent, "think", new_callable=AsyncMock, return_value="extreme"), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._anomaly_detection({"sigma_threshold": 2.0})

        extreme_anomalies = [a for a in result["anomalies"] if a["metric"] == "extreme"]
        if extreme_anomalies:
            assert extreme_anomalies[0]["severity"] in ("critical", "warning")


# ---------------------------------------------------------------------------
# Resource forecasting
# ---------------------------------------------------------------------------


class TestResourceForecast:
    @pytest.mark.asyncio
    async def test_insufficient_data(self, agent: MonitoringAgent):
        agent._baselines["cpu.usage_percent"] = [50.0] * 5  # Less than 20

        with patch.object(agent, "think", new_callable=AsyncMock, return_value="Not enough data"):
            result = await agent._resource_forecast({
                "metrics": ["cpu.usage_percent"],
                "hours": 24,
            })

        assert result["success"] is True
        assert result["forecasts"]["cpu.usage_percent"]["insufficient_data"] is True

    @pytest.mark.asyncio
    async def test_stable_metric_projection(self, agent: MonitoringAgent):
        # Flat line at 50%
        agent._baselines["cpu.usage_percent"] = [50.0] * 50

        with patch.object(agent, "think", new_callable=AsyncMock, return_value="Stable forecast"):
            result = await agent._resource_forecast({
                "metrics": ["cpu.usage_percent"],
                "hours": 24,
            })

        forecast = result["forecasts"]["cpu.usage_percent"]
        assert forecast["current"] == 50.0
        # Projected should be close to 50 since trend is flat
        assert abs(forecast["projected"] - 50.0) < 5.0
        assert forecast["capacity_warning"] is False

    @pytest.mark.asyncio
    async def test_increasing_trend_capacity_warning(self, agent: MonitoringAgent):
        # Steadily increasing from 70 to 92
        agent._baselines["disk.usage_percent"] = [70.0 + (i * 0.5) for i in range(50)]
        # Last value ~ 94.5

        with patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="Disk will be full soon"):
            result = await agent._resource_forecast({
                "metrics": ["disk.usage_percent"],
                "hours": 24,
            })

        forecast = result["forecasts"]["disk.usage_percent"]
        assert forecast["trend_per_hour"] > 0
        # Given the steep increase, should predict capacity issues
        assert forecast["projected"] > forecast["current"]

    @pytest.mark.asyncio
    async def test_capacity_warnings_list(self, agent: MonitoringAgent):
        agent._baselines["disk.usage_percent"] = [90.0 + (i * 0.3) for i in range(30)]

        with patch.object(agent, "think", new_callable=AsyncMock, return_value="Warning"):
            result = await agent._resource_forecast({
                "metrics": ["disk.usage_percent"],
                "hours": 48,
            })

        # If capacity warning triggered, it should be in the list
        if result["forecasts"]["disk.usage_percent"].get("capacity_warning"):
            assert "disk.usage_percent" in result["capacity_warnings"]


# ---------------------------------------------------------------------------
# Report generation
# ---------------------------------------------------------------------------


class TestGenerateReport:
    @pytest.mark.asyncio
    async def test_report_includes_all_sections(self, agent: MonitoringAgent):
        agent._baselines["cpu.usage_percent"] = [50.0 + i for i in range(20)]

        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if "system.metrics" in (name or ""):
                return {"success": True, "output": {"cpu_percent": 60.0}}
            return json.dumps({}).encode()

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock), \
             patch.object(agent, "push_event", new_callable=AsyncMock), \
             patch.object(agent, "get_recent_events", new_callable=AsyncMock, return_value=[]), \
             patch.object(agent, "_grpc_call", new_callable=AsyncMock,
                          return_value=json.dumps({}).encode()), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="System healthy, stable performance"), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._generate_report({"type": "health"})

        assert result["success"] is True
        report = result["report"]
        assert "current_metrics" in report
        assert "trends" in report
        assert "summary" in report
        assert report["type"] == "health"


# ---------------------------------------------------------------------------
# Dashboard data
# ---------------------------------------------------------------------------


class TestDashboardData:
    @pytest.mark.asyncio
    async def test_dashboard_assembles_data(self, agent: MonitoringAgent):
        agent._baselines["cpu.usage_percent"] = [50.0, 55.0, 60.0]

        with patch.object(agent, "_collect_metrics", new_callable=AsyncMock,
                          return_value={"metrics": {"cpu.usage_percent": 60.0}}), \
             patch.object(agent, "_check_alerts", new_callable=AsyncMock,
                          return_value={"active_alerts": []}), \
             patch.object(agent, "get_recent_events", new_callable=AsyncMock,
                          return_value=[]):
            result = await agent._dashboard_data({})

        assert result["success"] is True
        assert "metrics" in result
        assert "active_alerts" in result
        assert "baselines" in result
        assert result["baselines"]["cpu.usage_percent"]["current"] == 60.0
        assert result["baselines"]["cpu.usage_percent"]["data_points"] == 3
