"""
MonitoringAgent — Collects metrics, generates reports, and alerts on anomalies.

Capabilities:
  - Periodic metric collection across all system domains
  - Report generation (health, performance, trends)
  - Anomaly detection against learned baselines
  - Alert management with severity levels
  - Resource usage tracking and forecasting
"""

from __future__ import annotations

import asyncio
import json
import logging
import math
import time
from typing import Any

from aios_agent.base import BaseAgent, IntelligenceLevel

logger = logging.getLogger("aios.agent.monitoring")

METRIC_COLLECTION_INTERVAL_S = 30.0
ANOMALY_CHECK_INTERVAL_S = 60.0
BASELINE_WINDOW_SIZE = 100  # data points for rolling baseline


class MonitoringAgent(BaseAgent):
    """Agent responsible for system-wide monitoring, alerting, and reporting."""

    def __init__(self, *args: Any, **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        # In-memory rolling baseline buffers (metric_name -> list of values)
        self._baselines: dict[str, list[float]] = {}
        # Active alerts keyed by alert_id
        self._active_alerts: dict[str, dict[str, Any]] = {}

    def get_agent_type(self) -> str:
        return "monitoring"

    def get_capabilities(self) -> list[str]:
        return [
            "monitoring.collect_metrics",
            "monitoring.generate_report",
            "monitoring.check_alerts",
            "monitoring.anomaly_detection",
            "monitoring.resource_forecast",
            "monitoring.dashboard_data",
        ]

    # ------------------------------------------------------------------
    # Task dispatcher
    # ------------------------------------------------------------------

    async def handle_task(self, task: dict[str, Any]) -> dict[str, Any]:
        description = task.get("description", "").lower()
        input_data = task.get("input_json", {}) if isinstance(task.get("input_json"), dict) else {}

        if "collect" in description or "metric" in description and "report" not in description:
            return await self._collect_metrics(input_data)
        if "report" in description or "summary" in description:
            return await self._generate_report(input_data)
        if "alert" in description or "check" in description and "alert" in description:
            return await self._check_alerts(input_data)
        if "anomal" in description or "detect" in description:
            return await self._anomaly_detection(input_data)
        if "forecast" in description or "predict" in description or "capacity" in description:
            return await self._resource_forecast(input_data)
        if "dashboard" in description:
            return await self._dashboard_data(input_data)

        decision = await self.think(
            f"Monitoring task: '{task.get('description', '')}'. "
            f"Options: collect_metrics, generate_report, check_alerts, "
            f"anomaly_detection, resource_forecast, dashboard_data. "
            f"Which action? Reply with ONLY the action name.",
            level=IntelligenceLevel.REACTIVE,
        )
        action = decision.strip().lower()
        if "collect" in action or "metric" in action:
            return await self._collect_metrics(input_data)
        if "report" in action:
            return await self._generate_report(input_data)
        if "alert" in action:
            return await self._check_alerts(input_data)
        if "anomal" in action:
            return await self._anomaly_detection(input_data)
        if "forecast" in action:
            return await self._resource_forecast(input_data)
        return await self._dashboard_data(input_data)

    # ------------------------------------------------------------------
    # Metric collection
    # ------------------------------------------------------------------

    async def _collect_metrics(self, params: dict[str, Any]) -> dict[str, Any]:
        """Collect metrics from all system domains."""
        domains = params.get("domains", ["cpu", "memory", "disk", "network", "load", "processes"])

        # Call the system metrics tool
        sys_result = await self.call_tool(
            "system.metrics",
            {"categories": domains},
            reason="Monitoring: periodic metric collection",
        )

        metrics: dict[str, float] = {}
        if sys_result.get("success"):
            output = sys_result.get("output", {})
            metric_mappings = {
                "cpu_percent": "cpu.usage_percent",
                "memory_percent": "memory.usage_percent",
                "memory_used_mb": "memory.used_mb",
                "memory_total_mb": "memory.total_mb",
                "disk_percent": "disk.usage_percent",
                "disk_used_gb": "disk.used_gb",
                "disk_total_gb": "disk.total_gb",
                "load_1m": "load.1m",
                "load_5m": "load.5m",
                "load_15m": "load.15m",
                "network_rx_bytes": "network.rx_bytes",
                "network_tx_bytes": "network.tx_bytes",
                "process_count": "processes.count",
            }
            for src_key, dst_key in metric_mappings.items():
                val = output.get(src_key)
                if val is not None:
                    try:
                        metrics[dst_key] = float(val)
                    except (ValueError, TypeError):
                        pass

        # Push each metric to the memory service
        for key, value in metrics.items():
            await self.update_metric(key, value)
            # Update rolling baseline
            if key not in self._baselines:
                self._baselines[key] = []
            self._baselines[key].append(value)
            if len(self._baselines[key]) > BASELINE_WINDOW_SIZE:
                self._baselines[key] = self._baselines[key][-BASELINE_WINDOW_SIZE:]

        await self.push_event(
            "monitoring.metrics_collected",
            {"metric_count": len(metrics), "domains": domains},
        )

        return {
            "success": True,
            "metrics_collected": len(metrics),
            "metrics": metrics,
            "timestamp": int(time.time()),
        }

    # ------------------------------------------------------------------
    # Report generation
    # ------------------------------------------------------------------

    async def _generate_report(self, params: dict[str, Any]) -> dict[str, Any]:
        """Generate a system health/performance report."""
        report_type = params.get("type", "health")  # health | performance | full
        timeframe_minutes = params.get("timeframe_minutes", 60)

        # Collect current metrics
        current_metrics = await self._collect_metrics({})
        metrics = current_metrics.get("metrics", {})

        # Get recent events
        events = await self.get_recent_events(count=50)

        # Get system snapshot from memory
        snapshot_result = await self._grpc_call(
            self._get_memory_channel(),
            "aios.memory.MemoryService",
            "GetSystemSnapshot",
            self._encode_proto_json({}),
        )
        snapshot = self._decode_proto_json(snapshot_result)

        # Build report data
        report_data = {
            "timestamp": int(time.time()),
            "type": report_type,
            "timeframe_minutes": timeframe_minutes,
            "current_metrics": metrics,
            "system_snapshot": snapshot,
            "recent_events_count": len(events),
            "active_alerts": list(self._active_alerts.values()),
        }

        # Calculate baseline statistics for trends
        trends: dict[str, dict[str, float]] = {}
        for metric_name, values in self._baselines.items():
            if len(values) >= 5:
                mean = sum(values) / len(values)
                variance = sum((v - mean) ** 2 for v in values) / len(values)
                std_dev = math.sqrt(variance) if variance > 0 else 0.0
                trend_direction = 0.0
                if len(values) >= 10:
                    recent = sum(values[-5:]) / 5
                    older = sum(values[-10:-5]) / 5
                    trend_direction = recent - older
                trends[metric_name] = {
                    "mean": round(mean, 2),
                    "std_dev": round(std_dev, 2),
                    "current": values[-1],
                    "min": min(values),
                    "max": max(values),
                    "trend_direction": round(trend_direction, 2),
                    "data_points": len(values),
                }
        report_data["trends"] = trends

        # AI-generated summary
        summary = await self.think(
            f"Generate a {report_type} report summary.\n\n"
            f"Current metrics:\n"
            + "\n".join(f"  {k}: {v}" for k, v in sorted(metrics.items()))
            + f"\n\nTrends (mean -> current):\n"
            + "\n".join(
                f"  {k}: {v['mean']} -> {v['current']} (trend: {'+' if v['trend_direction'] > 0 else ''}{v['trend_direction']})"
                for k, v in sorted(trends.items())
            )
            + f"\n\nActive alerts: {len(self._active_alerts)}\n"
            f"Recent events: {len(events)}\n\n"
            f"Provide a 3-5 sentence executive summary covering system health, "
            f"notable trends, and any concerns.",
            level=IntelligenceLevel.OPERATIONAL,
        )

        report_data["summary"] = summary.strip()

        await self.store_memory(f"report:{report_type}", {
            "timestamp": int(time.time()),
            "metric_count": len(metrics),
            "alert_count": len(self._active_alerts),
        })

        return {
            "success": True,
            "report": report_data,
        }

    # ------------------------------------------------------------------
    # Alert checking
    # ------------------------------------------------------------------

    async def _check_alerts(self, params: dict[str, Any]) -> dict[str, Any]:
        """Evaluate alert conditions against current metrics."""
        # Define alert thresholds
        alert_rules: list[dict[str, Any]] = params.get("rules", [
            {"metric": "cpu.usage_percent", "operator": ">", "threshold": 90, "severity": "critical", "name": "cpu_critical"},
            {"metric": "cpu.usage_percent", "operator": ">", "threshold": 80, "severity": "warning", "name": "cpu_warning"},
            {"metric": "memory.usage_percent", "operator": ">", "threshold": 95, "severity": "critical", "name": "memory_critical"},
            {"metric": "memory.usage_percent", "operator": ">", "threshold": 85, "severity": "warning", "name": "memory_warning"},
            {"metric": "disk.usage_percent", "operator": ">", "threshold": 95, "severity": "critical", "name": "disk_critical"},
            {"metric": "disk.usage_percent", "operator": ">", "threshold": 85, "severity": "warning", "name": "disk_warning"},
            {"metric": "load.1m", "operator": ">", "threshold": 8.0, "severity": "warning", "name": "load_warning"},
        ])

        new_alerts: list[dict[str, Any]] = []
        resolved_alerts: list[str] = []

        for rule in alert_rules:
            metric_name = rule["metric"]
            threshold = rule["threshold"]
            operator = rule["operator"]
            alert_name = rule["name"]

            metric_value = await self.get_metric(metric_name)
            if metric_value is None:
                continue

            triggered = False
            if operator == ">" and metric_value > threshold:
                triggered = True
            elif operator == "<" and metric_value < threshold:
                triggered = True
            elif operator == ">=" and metric_value >= threshold:
                triggered = True
            elif operator == "<=" and metric_value <= threshold:
                triggered = True
            elif operator == "==" and metric_value == threshold:
                triggered = True

            if triggered:
                if alert_name not in self._active_alerts:
                    alert = {
                        "name": alert_name,
                        "metric": metric_name,
                        "value": metric_value,
                        "threshold": threshold,
                        "operator": operator,
                        "severity": rule.get("severity", "warning"),
                        "triggered_at": int(time.time()),
                    }
                    self._active_alerts[alert_name] = alert
                    new_alerts.append(alert)
                    logger.warning(
                        "ALERT %s: %s = %.1f %s %.1f",
                        alert_name, metric_name, metric_value, operator, threshold,
                    )
            else:
                if alert_name in self._active_alerts:
                    resolved_alerts.append(alert_name)
                    del self._active_alerts[alert_name]
                    logger.info("Alert resolved: %s", alert_name)

        if new_alerts:
            await self.push_event(
                "monitoring.alerts_triggered",
                {"new_alerts": new_alerts, "total_active": len(self._active_alerts)},
                critical=any(a["severity"] == "critical" for a in new_alerts),
            )

        if resolved_alerts:
            await self.push_event(
                "monitoring.alerts_resolved",
                {"resolved": resolved_alerts, "total_active": len(self._active_alerts)},
            )

        return {
            "success": True,
            "new_alerts": new_alerts,
            "resolved_alerts": resolved_alerts,
            "active_alerts": list(self._active_alerts.values()),
            "total_active": len(self._active_alerts),
        }

    # ------------------------------------------------------------------
    # Anomaly detection
    # ------------------------------------------------------------------

    async def _anomaly_detection(self, params: dict[str, Any]) -> dict[str, Any]:
        """Detect anomalies by comparing current values to rolling baselines."""
        threshold_sigma = params.get("sigma_threshold", 2.5)
        anomalies: list[dict[str, Any]] = []

        for metric_name, values in self._baselines.items():
            if len(values) < 10:
                continue

            mean = sum(values) / len(values)
            variance = sum((v - mean) ** 2 for v in values) / len(values)
            std_dev = math.sqrt(variance) if variance > 0 else 0.001
            current = values[-1]
            z_score = (current - mean) / std_dev

            if abs(z_score) > threshold_sigma:
                anomalies.append({
                    "metric": metric_name,
                    "current_value": round(current, 2),
                    "baseline_mean": round(mean, 2),
                    "std_dev": round(std_dev, 2),
                    "z_score": round(z_score, 2),
                    "direction": "above" if z_score > 0 else "below",
                    "severity": "critical" if abs(z_score) > 4 else "warning",
                })

        # AI analysis of anomalies
        analysis = ""
        if anomalies:
            analysis_text = await self.think(
                f"Anomaly detection found {len(anomalies)} anomalies:\n"
                + "\n".join(
                    f"- {a['metric']}: {a['current_value']} ({a['direction']} baseline, z={a['z_score']})"
                    for a in anomalies
                )
                + "\n\nAre these anomalies concerning? What might cause them? "
                f"Provide brief analysis and recommended actions.",
                level=IntelligenceLevel.TACTICAL,
            )
            analysis = analysis_text.strip()

            await self.push_event(
                "monitoring.anomalies_detected",
                {"count": len(anomalies), "anomalies": anomalies},
                critical=any(a["severity"] == "critical" for a in anomalies),
            )

        return {
            "success": True,
            "anomalies_found": len(anomalies),
            "sigma_threshold": threshold_sigma,
            "metrics_evaluated": len(self._baselines),
            "anomalies": anomalies,
            "analysis": analysis,
        }

    # ------------------------------------------------------------------
    # Resource forecast
    # ------------------------------------------------------------------

    async def _resource_forecast(self, params: dict[str, Any]) -> dict[str, Any]:
        """Forecast resource usage based on trend analysis."""
        forecast_hours = params.get("hours", 24)
        metrics_to_forecast = params.get("metrics", [
            "cpu.usage_percent",
            "memory.usage_percent",
            "disk.usage_percent",
        ])

        forecasts: dict[str, dict[str, Any]] = {}

        for metric_name in metrics_to_forecast:
            values = self._baselines.get(metric_name, [])
            if len(values) < 20:
                forecasts[metric_name] = {
                    "insufficient_data": True,
                    "data_points": len(values),
                    "minimum_required": 20,
                }
                continue

            # Simple linear regression for trend
            n = len(values)
            x_vals = list(range(n))
            x_mean = sum(x_vals) / n
            y_mean = sum(values) / n

            numerator = sum((x - x_mean) * (y - y_mean) for x, y in zip(x_vals, values))
            denominator = sum((x - x_mean) ** 2 for x in x_vals)

            slope = numerator / denominator if denominator != 0 else 0.0
            intercept = y_mean - slope * x_mean

            # Project forward
            collection_interval_hours = METRIC_COLLECTION_INTERVAL_S / 3600
            future_points = int(forecast_hours / collection_interval_hours)
            projected_value = intercept + slope * (n + future_points)

            # Determine if capacity will be exceeded
            capacity_warning = False
            hours_to_capacity = None
            if "percent" in metric_name and slope > 0:
                remaining = 100.0 - values[-1]
                if remaining > 0 and slope > 0:
                    points_to_full = remaining / slope
                    hours_to_capacity = points_to_full * collection_interval_hours
                    if hours_to_capacity < forecast_hours:
                        capacity_warning = True

            forecasts[metric_name] = {
                "current": round(values[-1], 2),
                "projected": round(max(0, projected_value), 2),
                "trend_per_hour": round(slope / collection_interval_hours, 4) if collection_interval_hours > 0 else 0.0,
                "forecast_hours": forecast_hours,
                "capacity_warning": capacity_warning,
                "hours_to_capacity": round(hours_to_capacity, 1) if hours_to_capacity is not None else None,
                "data_points": n,
            }

        # AI summary
        summary = await self.think(
            f"Resource forecast for next {forecast_hours}h:\n"
            + "\n".join(
                f"- {k}: current={v.get('current', '?')}, projected={v.get('projected', '?')}, "
                f"trend={v.get('trend_per_hour', '?')}/hr"
                + (f" WARNING: capacity in {v.get('hours_to_capacity', '?')}h" if v.get("capacity_warning") else "")
                for k, v in forecasts.items()
                if not v.get("insufficient_data")
            )
            + "\n\nProvide a brief forecast summary with any capacity planning recommendations.",
            level=IntelligenceLevel.OPERATIONAL,
        )

        return {
            "success": True,
            "forecast_hours": forecast_hours,
            "forecasts": forecasts,
            "capacity_warnings": [k for k, v in forecasts.items() if v.get("capacity_warning")],
            "summary": summary.strip(),
        }

    # ------------------------------------------------------------------
    # Dashboard data
    # ------------------------------------------------------------------

    async def _dashboard_data(self, params: dict[str, Any]) -> dict[str, Any]:
        """Assemble data for a monitoring dashboard."""
        # Current metrics
        metrics_data = await self._collect_metrics({})
        metrics = metrics_data.get("metrics", {})

        # Active alerts
        alert_data = await self._check_alerts({})

        # Recent events
        events = await self.get_recent_events(count=20)

        # Baselines summary
        baselines_summary: dict[str, dict[str, float]] = {}
        for metric_name, values in self._baselines.items():
            if values:
                baselines_summary[metric_name] = {
                    "current": values[-1],
                    "mean": round(sum(values) / len(values), 2),
                    "min": min(values),
                    "max": max(values),
                    "data_points": len(values),
                }

        return {
            "success": True,
            "timestamp": int(time.time()),
            "metrics": metrics,
            "active_alerts": alert_data.get("active_alerts", []),
            "recent_events": events,
            "baselines": baselines_summary,
            "uptime_seconds": self.uptime_seconds,
        }

    # ------------------------------------------------------------------
    # Background loops
    # ------------------------------------------------------------------

    async def _metric_collection_loop(self) -> None:
        """Periodically collect metrics."""
        while not self._shutdown_event.is_set():
            try:
                await self._collect_metrics({})
            except Exception as exc:
                logger.error("Metric collection error: %s", exc)
            try:
                await asyncio.wait_for(
                    self._shutdown_event.wait(),
                    timeout=METRIC_COLLECTION_INTERVAL_S,
                )
            except asyncio.TimeoutError:
                pass

    async def _alert_check_loop(self) -> None:
        """Periodically check alert conditions and run anomaly detection."""
        while not self._shutdown_event.is_set():
            try:
                await self._check_alerts({})
                await self._anomaly_detection({})
            except Exception as exc:
                logger.error("Alert check error: %s", exc)
            try:
                await asyncio.wait_for(
                    self._shutdown_event.wait(),
                    timeout=ANOMALY_CHECK_INTERVAL_S,
                )
            except asyncio.TimeoutError:
                pass

    async def run(self) -> None:
        self._running = True
        try:
            await self.register_with_orchestrator()
            await asyncio.gather(
                self.heartbeat_loop(),
                self._metric_collection_loop(),
                self._alert_check_loop(),
                self._shutdown_event.wait(),
            )
        finally:
            await self.unregister_from_orchestrator()
            await self._close_channels()
            self._running = False
