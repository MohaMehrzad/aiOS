"""
LearningAgent — Pattern recognition, parameter optimization, and improvement suggestions.

Capabilities:
  - Analyse historical decisions and outcomes to recognise patterns
  - Optimise system parameters based on observed performance
  - Suggest improvements to tool selection and execution strategies
  - Track success rates and update pattern confidence scores
"""

from __future__ import annotations

import asyncio
import json
import logging
import math
import time
from typing import Any

from aios_agent.base import BaseAgent, IntelligenceLevel

logger = logging.getLogger("aios.agent.learning")

LEARNING_CYCLE_INTERVAL_S = 300.0  # 5 minutes
MIN_DATA_POINTS = 10
IMPROVEMENT_CONFIDENCE_THRESHOLD = 0.7


class LearningAgent(BaseAgent):
    """Agent responsible for system-wide learning and optimisation."""

    def __init__(self, *args: Any, **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        # Local caches for fast access during analysis
        self._decision_cache: list[dict[str, Any]] = []
        self._pattern_cache: dict[str, dict[str, Any]] = {}
        self._performance_history: dict[str, list[float]] = {}

    def get_agent_type(self) -> str:
        return "learning"

    def get_capabilities(self) -> list[str]:
        return [
            "learning.analyze_patterns",
            "learning.optimize_parameters",
            "learning.suggest_improvements",
            "learning.update_patterns",
            "learning.performance_analysis",
            "learning.tool_effectiveness",
        ]

    # ------------------------------------------------------------------
    # Task dispatcher
    # ------------------------------------------------------------------

    async def handle_task(self, task: dict[str, Any]) -> dict[str, Any]:
        description = task.get("description", "").lower()
        input_data = task.get("input_json", {}) if isinstance(task.get("input_json"), dict) else {}

        if "pattern" in description or "recogni" in description or "analys" in description and "pattern" in description:
            return await self._analyze_patterns(input_data)
        if "optimi" in description or "parameter" in description or "tune" in description:
            return await self._optimize_parameters(input_data)
        if "suggest" in description or "improve" in description or "recommend" in description:
            return await self._suggest_improvements(input_data)
        if "tool" in description and ("effect" in description or "performance" in description):
            return await self._tool_effectiveness(input_data)
        if "performance" in description or "trend" in description:
            return await self._performance_analysis(input_data)

        decision = await self.think(
            f"Learning task: '{task.get('description', '')}'. "
            f"Options: analyze_patterns, optimize_parameters, suggest_improvements, "
            f"tool_effectiveness, performance_analysis. "
            f"Which action? Reply with ONLY the action name.",
            level=IntelligenceLevel.REACTIVE,
        )
        action = decision.strip().lower()
        if "pattern" in action:
            return await self._analyze_patterns(input_data)
        if "optimi" in action or "param" in action:
            return await self._optimize_parameters(input_data)
        if "tool" in action:
            return await self._tool_effectiveness(input_data)
        if "perform" in action:
            return await self._performance_analysis(input_data)
        return await self._suggest_improvements(input_data)

    # ------------------------------------------------------------------
    # Pattern analysis
    # ------------------------------------------------------------------

    async def _analyze_patterns(self, params: dict[str, Any]) -> dict[str, Any]:
        """Analyse past decisions and events to discover recurring patterns."""
        timeframe_hours = params.get("timeframe_hours", 24)
        min_occurrences = params.get("min_occurrences", 3)

        # Fetch recent events
        events = await self.get_recent_events(count=200, category="")

        # Fetch recent decisions from memory
        context_chunks = await self.assemble_context(
            task_description="Historical decisions and tool calls for pattern analysis",
            max_tokens=8000,
            memory_tiers=["working", "long_term"],
        )

        # Parse events into trigger-action pairs
        trigger_action_pairs: list[dict[str, str]] = []
        for event in events:
            data = event.get("data_json", "{}")
            if isinstance(data, str):
                try:
                    data = json.loads(data)
                except json.JSONDecodeError:
                    data = {}
            trigger = event.get("category", "unknown")
            action = data.get("action", data.get("type", "unknown"))
            outcome = data.get("outcome", data.get("success", "unknown"))
            trigger_action_pairs.append({
                "trigger": trigger,
                "action": str(action),
                "outcome": str(outcome),
                "timestamp": event.get("timestamp", 0),
            })

        # Count trigger-action frequencies
        frequency_map: dict[str, dict[str, int]] = {}
        success_map: dict[str, dict[str, list[bool]]] = {}

        for pair in trigger_action_pairs:
            trigger = pair["trigger"]
            action = pair["action"]
            outcome = pair["outcome"]

            if trigger not in frequency_map:
                frequency_map[trigger] = {}
                success_map[trigger] = {}
            frequency_map[trigger][action] = frequency_map[trigger].get(action, 0) + 1

            if action not in success_map[trigger]:
                success_map[trigger][action] = []
            success_map[trigger][action].append(
                outcome in ("True", "true", "1", "success", "completed")
            )

        # Identify significant patterns
        discovered_patterns: list[dict[str, Any]] = []
        for trigger, actions in frequency_map.items():
            for action, count in actions.items():
                if count >= min_occurrences:
                    successes = success_map.get(trigger, {}).get(action, [])
                    success_rate = sum(successes) / len(successes) if successes else 0.0

                    discovered_patterns.append({
                        "trigger": trigger,
                        "action": action,
                        "occurrences": count,
                        "success_rate": round(success_rate, 3),
                        "confidence": round(min(1.0, count / 20.0 * success_rate), 3),
                    })

        discovered_patterns.sort(key=lambda p: p["confidence"], reverse=True)

        # Store high-confidence patterns
        stored_count = 0
        for pattern in discovered_patterns:
            if pattern["confidence"] >= IMPROVEMENT_CONFIDENCE_THRESHOLD:
                pattern_id = await self.store_pattern(
                    trigger=pattern["trigger"],
                    action=pattern["action"],
                    success_rate=pattern["success_rate"],
                    created_from=self.agent_id,
                )
                pattern["pattern_id"] = pattern_id
                stored_count += 1

        # AI analysis of discovered patterns
        analysis = ""
        if discovered_patterns:
            analysis_text = await self.think(
                f"Pattern analysis discovered {len(discovered_patterns)} patterns:\n"
                + "\n".join(
                    f"- '{p['trigger']}' -> '{p['action']}' "
                    f"(n={p['occurrences']}, success={p['success_rate']:.0%}, conf={p['confidence']:.2f})"
                    for p in discovered_patterns[:15]
                )
                + "\n\nWhich patterns are most valuable for the system? "
                f"Any patterns that should be promoted to automatic rules? "
                f"Any concerning anti-patterns?",
                level=IntelligenceLevel.STRATEGIC,
            )
            analysis = analysis_text.strip()

        await self.store_memory("last_pattern_analysis", {
            "timestamp": int(time.time()),
            "patterns_found": len(discovered_patterns),
            "patterns_stored": stored_count,
        })

        return {
            "success": True,
            "events_analyzed": len(events),
            "patterns_discovered": len(discovered_patterns),
            "patterns_stored": stored_count,
            "patterns": discovered_patterns,
            "analysis": analysis,
        }

    # ------------------------------------------------------------------
    # Parameter optimization
    # ------------------------------------------------------------------

    async def _optimize_parameters(self, params: dict[str, Any]) -> dict[str, Any]:
        """Optimise system parameters based on observed performance data."""
        target_metrics = params.get("target_metrics", [
            "cpu.usage_percent",
            "memory.usage_percent",
            "disk.usage_percent",
        ])

        # Collect current parameter values and performance data
        current_params: dict[str, Any] = {}
        performance_data: dict[str, dict[str, float]] = {}

        for metric_name in target_metrics:
            metric_val = await self.get_metric(metric_name)
            if metric_val is not None:
                if metric_name not in self._performance_history:
                    self._performance_history[metric_name] = []
                self._performance_history[metric_name].append(metric_val)
                # Keep bounded history
                if len(self._performance_history[metric_name]) > 200:
                    self._performance_history[metric_name] = self._performance_history[metric_name][-200:]

                values = self._performance_history[metric_name]
                performance_data[metric_name] = {
                    "current": metric_val,
                    "mean": sum(values) / len(values),
                    "min": min(values),
                    "max": max(values),
                    "std_dev": math.sqrt(
                        sum((v - sum(values) / len(values)) ** 2 for v in values) / len(values)
                    ) if len(values) > 1 else 0.0,
                    "data_points": len(values),
                }

        # Get tunable parameters
        tunable_result = await self.call_tool(
            "system.get_tunable_params", {},
            reason="Fetching tunable system parameters for optimization",
        )
        if tunable_result.get("success"):
            current_params = tunable_result.get("output", {}).get("parameters", {})

        # Ask AI to suggest parameter changes
        optimization_prompt = (
            f"System parameter optimization analysis.\n\n"
            f"Current tunable parameters:\n"
            + json.dumps(current_params, indent=2, default=str)
            + f"\n\nPerformance data:\n"
            + "\n".join(
                f"- {k}: current={v['current']:.1f}, mean={v['mean']:.1f}, "
                f"stddev={v['std_dev']:.1f}, range=[{v['min']:.1f}, {v['max']:.1f}]"
                for k, v in performance_data.items()
            )
            + "\n\nSuggest specific parameter changes to improve performance. "
            f"For each suggestion, provide:\n"
            f"1. Parameter name\n2. Current value\n3. Suggested value\n4. Expected impact\n"
            f"Format as JSON array of objects."
        )

        suggestion_text = await self.think(optimization_prompt, level=IntelligenceLevel.STRATEGIC)

        # Parse AI suggestions
        suggestions: list[dict[str, Any]] = []
        try:
            # Try to extract JSON from the response
            text = suggestion_text.strip()
            start = text.find("[")
            end = text.rfind("]")
            if start >= 0 and end > start:
                suggestions = json.loads(text[start : end + 1])
        except json.JSONDecodeError:
            # If JSON parsing fails, create a single suggestion from the text
            suggestions = [{
                "parameter": "review_needed",
                "suggestion": suggestion_text.strip()[:500],
                "expected_impact": "See detailed analysis",
            }]

        # Apply safe suggestions automatically
        applied: list[dict[str, Any]] = []
        skipped: list[dict[str, Any]] = []

        for suggestion in suggestions:
            param_name = suggestion.get("parameter", "")
            new_value = suggestion.get("suggested_value", suggestion.get("new_value"))

            if not param_name or new_value is None or param_name == "review_needed":
                skipped.append(suggestion)
                continue

            # Only auto-apply if confidence is high and within safe bounds
            if params.get("auto_apply", False):
                apply_result = await self.call_tool(
                    "system.set_tunable_param",
                    {"parameter": param_name, "value": new_value},
                    reason=f"Optimization: setting {param_name}={new_value}",
                )
                if apply_result.get("success"):
                    applied.append(suggestion)
                    await self.store_decision(
                        context=f"Parameter optimization: {param_name}",
                        options=[str(current_params.get(param_name, "unknown")), str(new_value)],
                        chosen=str(new_value),
                        reasoning=suggestion.get("expected_impact", ""),
                        intelligence_level="strategic",
                    )
                else:
                    skipped.append({**suggestion, "error": apply_result.get("error", "")})
            else:
                skipped.append(suggestion)

        return {
            "success": True,
            "metrics_analyzed": len(performance_data),
            "parameters_reviewed": len(current_params),
            "suggestions": suggestions,
            "applied": applied,
            "skipped": skipped,
            "performance_data": performance_data,
        }

    # ------------------------------------------------------------------
    # Improvement suggestions
    # ------------------------------------------------------------------

    async def _suggest_improvements(self, params: dict[str, Any]) -> dict[str, Any]:
        """Generate suggestions for system-wide improvements."""
        # Gather comprehensive context
        pattern_data = await self._analyze_patterns({"min_occurrences": 2})
        tool_data = await self._tool_effectiveness({})
        perf_data = await self._performance_analysis({})

        # Build a comprehensive analysis prompt
        improvement_prompt = (
            f"System improvement analysis for aiOS.\n\n"
            f"Pattern analysis: {pattern_data.get('patterns_discovered', 0)} patterns found, "
            f"top patterns:\n"
            + "\n".join(
                f"  - {p['trigger']} -> {p['action']} (success: {p['success_rate']:.0%})"
                for p in pattern_data.get("patterns", [])[:5]
            )
            + f"\n\nTool effectiveness:\n"
            + "\n".join(
                f"  - {t.get('tool', '?')}: {t.get('success_rate', 0):.0%} success, "
                f"avg {t.get('avg_duration_ms', 0):.0f}ms"
                for t in tool_data.get("tools", [])[:10]
            )
            + f"\n\nPerformance trends:\n"
            + "\n".join(
                f"  - {k}: trend={v.get('trend', 'stable')}, health={v.get('health', 'unknown')}"
                for k, v in perf_data.get("metrics_analysis", {}).items()
            )
            + "\n\nBased on this data, suggest:\n"
            f"1. Top 3 operational improvements\n"
            f"2. Top 3 configuration changes\n"
            f"3. Any automation opportunities\n"
            f"4. Potential issues to watch\n"
            f"Be specific and actionable."
        )

        suggestions_text = await self.think(improvement_prompt, level=IntelligenceLevel.STRATEGIC)

        # Parse into structured suggestions
        sections = {
            "operational_improvements": [],
            "configuration_changes": [],
            "automation_opportunities": [],
            "watch_items": [],
        }

        current_section = "operational_improvements"
        for line in suggestions_text.strip().split("\n"):
            line = line.strip()
            if not line:
                continue
            lower_line = line.lower()
            if "operational" in lower_line or "improvement" in lower_line and ":" in line:
                current_section = "operational_improvements"
                continue
            if "configuration" in lower_line or "config" in lower_line and ":" in line:
                current_section = "configuration_changes"
                continue
            if "automation" in lower_line and ":" in line:
                current_section = "automation_opportunities"
                continue
            if "watch" in lower_line or "issue" in lower_line or "potential" in lower_line and ":" in line:
                current_section = "watch_items"
                continue
            cleaned = line.lstrip("- 0123456789.)").strip()
            if cleaned:
                sections[current_section].append(cleaned)

        await self.store_memory("last_improvement_suggestions", {
            "timestamp": int(time.time()),
            "suggestion_count": sum(len(v) for v in sections.values()),
        })

        return {
            "success": True,
            "suggestions": sections,
            "raw_analysis": suggestions_text.strip(),
            "data_sources": {
                "patterns_analyzed": pattern_data.get("patterns_discovered", 0),
                "tools_analyzed": len(tool_data.get("tools", [])),
                "metrics_analyzed": len(perf_data.get("metrics_analysis", {})),
            },
        }

    # ------------------------------------------------------------------
    # Tool effectiveness analysis
    # ------------------------------------------------------------------

    async def _tool_effectiveness(self, params: dict[str, Any]) -> dict[str, Any]:
        """Analyse tool call effectiveness from historical data."""
        # Get tool call history from events
        events = await self.get_recent_events(count=500, category="tool_call")

        # Also try tool-specific events
        if not events:
            events = await self.get_recent_events(count=500, category="")
            events = [
                e for e in events
                if "tool" in e.get("category", "").lower()
                or "tool" in json.dumps(e.get("data_json", {})).lower()
            ]

        # Aggregate by tool name
        tool_stats: dict[str, dict[str, Any]] = {}
        for event in events:
            data = event.get("data_json", "{}")
            if isinstance(data, str):
                try:
                    data = json.loads(data)
                except json.JSONDecodeError:
                    data = {}

            tool_name = data.get("tool", data.get("tool_name", "unknown"))
            if tool_name == "unknown":
                continue

            if tool_name not in tool_stats:
                tool_stats[tool_name] = {
                    "tool": tool_name,
                    "calls": 0,
                    "successes": 0,
                    "failures": 0,
                    "total_duration_ms": 0,
                    "durations": [],
                }

            stats = tool_stats[tool_name]
            stats["calls"] += 1
            success = data.get("success", True)
            if success in (True, "true", "True", 1, "1"):
                stats["successes"] += 1
            else:
                stats["failures"] += 1

            duration = data.get("duration_ms", 0)
            try:
                duration = int(duration)
                stats["total_duration_ms"] += duration
                stats["durations"].append(duration)
            except (ValueError, TypeError):
                pass

        # Calculate metrics per tool
        tools_analysis: list[dict[str, Any]] = []
        for tool_name, stats in tool_stats.items():
            success_rate = stats["successes"] / stats["calls"] if stats["calls"] > 0 else 0.0
            avg_duration = stats["total_duration_ms"] / stats["calls"] if stats["calls"] > 0 else 0.0

            durations = stats["durations"]
            p95_duration = 0.0
            if durations:
                sorted_d = sorted(durations)
                p95_idx = int(len(sorted_d) * 0.95)
                p95_duration = sorted_d[min(p95_idx, len(sorted_d) - 1)]

            tools_analysis.append({
                "tool": tool_name,
                "total_calls": stats["calls"],
                "success_rate": round(success_rate, 3),
                "failure_rate": round(1 - success_rate, 3),
                "avg_duration_ms": round(avg_duration, 1),
                "p95_duration_ms": p95_duration,
            })

        tools_analysis.sort(key=lambda t: t["total_calls"], reverse=True)

        # Identify underperforming tools
        underperforming = [
            t for t in tools_analysis
            if t["success_rate"] < 0.8 and t["total_calls"] >= 5
        ]

        # Store analysis
        await self.store_memory("tool_effectiveness", {
            "timestamp": int(time.time()),
            "tools_analyzed": len(tools_analysis),
            "underperforming": len(underperforming),
        })

        return {
            "success": True,
            "events_analyzed": len(events),
            "tools_analyzed": len(tools_analysis),
            "tools": tools_analysis,
            "underperforming": underperforming,
        }

    # ------------------------------------------------------------------
    # Performance analysis
    # ------------------------------------------------------------------

    async def _performance_analysis(self, params: dict[str, Any]) -> dict[str, Any]:
        """Analyse overall system performance trends."""
        target_metrics = params.get("metrics", [
            "cpu.usage_percent",
            "memory.usage_percent",
            "disk.usage_percent",
            "load.1m",
        ])

        metrics_analysis: dict[str, dict[str, Any]] = {}

        for metric_name in target_metrics:
            values = self._performance_history.get(metric_name, [])
            metric_val = await self.get_metric(metric_name)
            if metric_val is not None:
                if metric_name not in self._performance_history:
                    self._performance_history[metric_name] = []
                self._performance_history[metric_name].append(metric_val)
                values = self._performance_history[metric_name]

            if not values:
                metrics_analysis[metric_name] = {"data": "insufficient", "data_points": 0}
                continue

            n = len(values)
            mean = sum(values) / n
            std_dev = math.sqrt(sum((v - mean) ** 2 for v in values) / n) if n > 1 else 0.0

            # Trend detection via simple moving averages
            trend = "stable"
            if n >= 10:
                recent_avg = sum(values[-5:]) / 5
                older_avg = sum(values[-10:-5]) / 5
                diff_pct = ((recent_avg - older_avg) / older_avg * 100) if older_avg != 0 else 0
                if diff_pct > 5:
                    trend = "increasing"
                elif diff_pct < -5:
                    trend = "decreasing"

            # Health assessment
            health = "good"
            current = values[-1]
            if "percent" in metric_name:
                if current > 90:
                    health = "critical"
                elif current > 75:
                    health = "warning"
            elif "load" in metric_name:
                if current > 4.0:
                    health = "critical"
                elif current > 2.0:
                    health = "warning"

            metrics_analysis[metric_name] = {
                "current": round(current, 2),
                "mean": round(mean, 2),
                "std_dev": round(std_dev, 2),
                "min": round(min(values), 2),
                "max": round(max(values), 2),
                "trend": trend,
                "health": health,
                "data_points": n,
            }

        # Overall health score (0-100)
        health_scores = {
            "good": 100,
            "warning": 60,
            "critical": 20,
        }
        individual_scores = [
            health_scores.get(v.get("health", "good"), 100)
            for v in metrics_analysis.values()
            if isinstance(v, dict) and "health" in v
        ]
        overall_health = sum(individual_scores) / len(individual_scores) if individual_scores else 100.0

        return {
            "success": True,
            "metrics_analysis": metrics_analysis,
            "overall_health_score": round(overall_health, 1),
            "timestamp": int(time.time()),
        }

    # ------------------------------------------------------------------
    # Goal suggestions from learned patterns (Phase 4.4)
    # ------------------------------------------------------------------

    async def analyze_patterns_and_suggest_goals(self) -> list[dict[str, Any]]:
        """Query memory for high-success patterns and recurring failure clusters,
        then suggest goals the orchestrator should create proactively."""
        suggested_goals: list[dict[str, Any]] = []

        # Gather patterns with high confidence
        pattern_result = await self._analyze_patterns({"min_occurrences": 3})
        high_confidence_patterns = [
            p for p in pattern_result.get("patterns", [])
            if p.get("confidence", 0) >= IMPROVEMENT_CONFIDENCE_THRESHOLD
        ]

        # Gather tool effectiveness data
        tool_data = await self._tool_effectiveness({})
        underperforming_tools = tool_data.get("underperforming", [])

        # Suggest goals for underperforming tools
        for tool in underperforming_tools:
            tool_name = tool.get("tool", "unknown")
            success_rate = tool.get("success_rate", 0)
            suggested_goals.append({
                "description": (
                    f"Investigate poor performance of tool '{tool_name}' "
                    f"(success rate: {success_rate:.0%}). Consider alternative "
                    f"approaches or fix underlying issues."
                ),
                "priority": 6,
                "source": f"learning:tool_effectiveness:{tool_name}",
            })

        # Suggest goals for failure patterns
        failure_patterns = [
            p for p in high_confidence_patterns
            if p.get("success_rate", 1.0) < 0.5
        ]
        for pattern in failure_patterns[:3]:
            suggested_goals.append({
                "description": (
                    f"Recurring failure pattern: '{pattern['trigger']}' -> "
                    f"'{pattern['action']}' fails {1 - pattern['success_rate']:.0%} "
                    f"of the time ({pattern['occurrences']} occurrences). "
                    f"Investigate root cause and implement fix."
                ),
                "priority": 7,
                "source": f"learning:failure_pattern:{pattern['trigger']}",
            })

        # Suggest optimization goals from high-success patterns
        optimization_patterns = [
            p for p in high_confidence_patterns
            if p.get("success_rate", 0) >= 0.9
            and p.get("occurrences", 0) >= 10
        ]
        for pattern in optimization_patterns[:2]:
            suggested_goals.append({
                "description": (
                    f"High-frequency pattern detected: '{pattern['trigger']}' -> "
                    f"'{pattern['action']}' ({pattern['occurrences']} times, "
                    f"{pattern['success_rate']:.0%} success). Consider automating "
                    f"as a scheduled rule or plugin."
                ),
                "priority": 4,
                "source": f"learning:automation:{pattern['trigger']}",
            })

        # Performance trend goals
        perf_data = await self._performance_analysis({})
        for metric_name, analysis in perf_data.get("metrics_analysis", {}).items():
            if isinstance(analysis, dict):
                if analysis.get("health") == "critical":
                    suggested_goals.append({
                        "description": (
                            f"Critical metric: {metric_name} at {analysis.get('current', '?')} "
                            f"(trend: {analysis.get('trend', 'unknown')}). "
                            f"Take immediate corrective action."
                        ),
                        "priority": 9,
                        "source": f"learning:metric_critical:{metric_name}",
                    })
                elif analysis.get("trend") == "increasing" and "percent" in metric_name:
                    suggested_goals.append({
                        "description": (
                            f"Rising metric: {metric_name} trending upward "
                            f"(current: {analysis.get('current', '?')}). "
                            f"Investigate and prevent threshold breach."
                        ),
                        "priority": 5,
                        "source": f"learning:trend_rising:{metric_name}",
                    })

        logger.info("Generated %d goal suggestions from learned patterns", len(suggested_goals))

        await self.store_memory("last_goal_suggestions", {
            "timestamp": int(time.time()),
            "suggestions_count": len(suggested_goals),
        })

        return suggested_goals

    # ------------------------------------------------------------------
    # Background learning cycle
    # ------------------------------------------------------------------

    async def _learning_cycle(self) -> None:
        """Periodic learning cycle: analyse patterns, optimise, and suggest."""
        while not self._shutdown_event.is_set():
            try:
                logger.info("Learning cycle starting")

                # Phase 1: Collect performance data
                await self._performance_analysis({})

                # Phase 2: Analyse patterns (only if enough data)
                events = await self.get_recent_events(count=10)
                if len(events) >= MIN_DATA_POINTS:
                    pattern_result = await self._analyze_patterns({"min_occurrences": 3})
                    if pattern_result.get("patterns_discovered", 0) > 0:
                        logger.info(
                            "Learning cycle found %d patterns",
                            pattern_result["patterns_discovered"],
                        )

                # Phase 3: Tool effectiveness review
                await self._tool_effectiveness({})

                logger.info("Learning cycle complete")
            except Exception as exc:
                logger.error("Learning cycle error: %s", exc)

            try:
                await asyncio.wait_for(
                    self._shutdown_event.wait(),
                    timeout=LEARNING_CYCLE_INTERVAL_S,
                )
            except asyncio.TimeoutError:
                pass

    async def run(self) -> None:
        self._running = True
        try:
            await self.register_with_orchestrator()
            await asyncio.gather(
                self.heartbeat_loop(),
                self._learning_cycle(),
                self._shutdown_event.wait(),
            )
        finally:
            await self.unregister_from_orchestrator()
            await self._close_channels()
            self._running = False


if __name__ == "__main__":
    import os
    agent = LearningAgent(agent_id=os.getenv("AIOS_AGENT_NAME", "learning-agent"))
    asyncio.run(agent.run())
