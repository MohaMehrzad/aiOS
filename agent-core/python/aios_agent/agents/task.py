"""
TaskAgent — General-purpose task executor.

Capabilities:
  - Parses natural-language goal descriptions
  - Builds multi-step execution plans using AI reasoning
  - Delegates subtasks to other agents via the orchestrator
  - Manages execution pipelines with dependency tracking
"""

from __future__ import annotations

import asyncio
import json
import logging
import time
import uuid
from typing import Any

from aios_agent.base import BaseAgent, IntelligenceLevel

logger = logging.getLogger("aios.agent.task")

MAX_PLAN_STEPS = 20
SUBTASK_TIMEOUT_S = 120.0


class TaskAgent(BaseAgent):
    """General-purpose agent that decomposes goals into plans and executes them."""

    def get_agent_type(self) -> str:
        return "task"

    def get_capabilities(self) -> list[str]:
        return [
            "task.plan",
            "task.execute",
            "task.delegate",
            "task.pipeline",
            "task.decompose",
            "task.coordinate",
        ]

    # ------------------------------------------------------------------
    # Task dispatcher
    # ------------------------------------------------------------------

    async def handle_task(self, task: dict[str, Any]) -> dict[str, Any]:
        description = task.get("description", "").lower()
        input_data = task.get("input_json", {}) if isinstance(task.get("input_json"), dict) else {}

        if "plan" in description or "decompose" in description:
            return await self._create_plan(task)
        if "execute" in description and "plan" in description:
            plan = input_data.get("plan", [])
            return await self._execute_plan(plan, task)
        if "delegate" in description:
            return await self._delegate_subtask(input_data, task)

        # Default: create a plan from the description and execute it
        return await self._plan_and_execute(task)

    # ------------------------------------------------------------------
    # Plan creation
    # ------------------------------------------------------------------

    async def _create_plan(self, task: dict[str, Any]) -> dict[str, Any]:
        """Use AI to decompose a goal into an execution plan."""
        description = task.get("description", "")

        # Search memory for similar past plans
        past_context = await self.semantic_search(
            f"execution plan for: {description}",
            collections=["procedures", "decisions"],
            n_results=3,
        )

        context_text = ""
        if past_context:
            context_text = "Relevant past experience:\n" + "\n".join(
                f"- {r.get('content', '')[:200]}" for r in past_context
            )

        # Ask AI to create the plan
        plan_prompt = (
            f"Create an execution plan for the following goal:\n"
            f"Goal: {description}\n\n"
            f"{context_text}\n\n"
            f"Return a JSON array of steps. Each step must have:\n"
            f"  - \"id\": unique string identifier\n"
            f"  - \"description\": what this step does\n"
            f"  - \"agent_type\": which agent should handle it (system, network, security, package, storage, monitoring, task)\n"
            f"  - \"tool\": the tool to call (or empty if delegation)\n"
            f"  - \"input\": dict of input parameters\n"
            f"  - \"depends_on\": list of step IDs this depends on (empty list if none)\n"
            f"  - \"can_fail\": boolean, whether plan can continue if this step fails\n\n"
            f"Return ONLY valid JSON, no markdown, no explanation. Max {MAX_PLAN_STEPS} steps."
        )

        plan_text = await self.think(plan_prompt, level=IntelligenceLevel.TACTICAL)

        # Parse the plan
        plan_text = plan_text.strip()
        if plan_text.startswith("```"):
            lines = plan_text.split("\n")
            plan_text = "\n".join(lines[1:-1] if lines[-1].strip() == "```" else lines[1:])

        try:
            plan_steps = json.loads(plan_text)
        except json.JSONDecodeError:
            # Try to extract JSON array from the response
            start = plan_text.find("[")
            end = plan_text.rfind("]")
            if start >= 0 and end > start:
                try:
                    plan_steps = json.loads(plan_text[start : end + 1])
                except json.JSONDecodeError:
                    plan_steps = [
                        {
                            "id": "step_1",
                            "description": task.get("description", "Execute task"),
                            "agent_type": "system",
                            "tool": "",
                            "input": {},
                            "depends_on": [],
                            "can_fail": False,
                        }
                    ]
            else:
                plan_steps = [
                    {
                        "id": "step_1",
                        "description": task.get("description", "Execute task"),
                        "agent_type": "system",
                        "tool": "",
                        "input": {},
                        "depends_on": [],
                        "can_fail": False,
                    }
                ]

        # Validate and normalise plan
        validated_steps: list[dict[str, Any]] = []
        seen_ids: set[str] = set()
        for step in plan_steps[:MAX_PLAN_STEPS]:
            step_id = step.get("id", f"step_{len(validated_steps) + 1}")
            if step_id in seen_ids:
                step_id = f"{step_id}_{uuid.uuid4().hex[:4]}"
            seen_ids.add(step_id)

            validated_steps.append({
                "id": step_id,
                "description": step.get("description", ""),
                "agent_type": step.get("agent_type", "system"),
                "tool": step.get("tool", ""),
                "input": step.get("input", {}),
                "depends_on": [d for d in step.get("depends_on", []) if d in seen_ids or d == step_id],
                "can_fail": step.get("can_fail", False),
                "status": "pending",
                "result": None,
            })

        await self.store_memory("last_plan", {
            "goal": task.get("description", ""),
            "steps": validated_steps,
            "created_at": int(time.time()),
        })

        return {
            "plan_id": uuid.uuid4().hex[:12],
            "goal": task.get("description", ""),
            "steps": validated_steps,
            "step_count": len(validated_steps),
        }

    # ------------------------------------------------------------------
    # Plan execution
    # ------------------------------------------------------------------

    async def _execute_plan(
        self,
        plan_steps: list[dict[str, Any]],
        task: dict[str, Any],
    ) -> dict[str, Any]:
        """Execute a plan step by step, respecting dependencies."""
        completed: dict[str, dict[str, Any]] = {}
        failed: list[dict[str, Any]] = []
        execution_order: list[str] = []

        # Build a dependency graph
        steps_by_id: dict[str, dict[str, Any]] = {}
        for step in plan_steps:
            steps_by_id[step["id"]] = step

        remaining = set(steps_by_id.keys())

        while remaining:
            # Find steps whose dependencies are all satisfied
            ready: list[str] = []
            for step_id in remaining:
                step = steps_by_id[step_id]
                deps = step.get("depends_on", [])
                # A step is ready if all deps are completed (or dep not in graph)
                if all(d in completed or d not in remaining for d in deps):
                    # Check if any required dep failed and step cannot tolerate it
                    dep_failed = any(d in [f["id"] for f in failed] for d in deps)
                    if dep_failed and not step.get("can_fail", False):
                        failed.append({
                            "id": step_id,
                            "error": "Dependency failed",
                            "skipped": True,
                        })
                        remaining.discard(step_id)
                        continue
                    ready.append(step_id)

            if not ready:
                # All remaining steps have unsatisfied dependencies — deadlock
                for step_id in remaining:
                    failed.append({"id": step_id, "error": "Deadlocked dependency", "skipped": True})
                break

            # Execute ready steps concurrently
            async def _run_step(sid: str) -> tuple[str, dict[str, Any]]:
                step = steps_by_id[sid]
                try:
                    result = await asyncio.wait_for(
                        self._execute_single_step(step, completed),
                        timeout=SUBTASK_TIMEOUT_S,
                    )
                    return sid, result
                except asyncio.TimeoutError:
                    return sid, {"success": False, "error": f"Step {sid} timed out"}
                except Exception as exc:
                    return sid, {"success": False, "error": str(exc)}

            results = await asyncio.gather(*[_run_step(sid) for sid in ready])

            for step_id, result in results:
                remaining.discard(step_id)
                execution_order.append(step_id)
                if result.get("success", False):
                    completed[step_id] = result
                    steps_by_id[step_id]["status"] = "completed"
                    steps_by_id[step_id]["result"] = result
                else:
                    steps_by_id[step_id]["status"] = "failed"
                    steps_by_id[step_id]["result"] = result
                    if not steps_by_id[step_id].get("can_fail", False):
                        failed.append({"id": step_id, "error": result.get("error", "")})
                    else:
                        # Mark as completed-with-failure but continue
                        completed[step_id] = result

        overall_success = len(failed) == 0

        await self.push_event(
            "task.plan_executed",
            {
                "goal": task.get("description", ""),
                "steps_total": len(plan_steps),
                "steps_completed": len(completed),
                "steps_failed": len(failed),
                "success": overall_success,
            },
        )

        return {
            "success": overall_success,
            "steps_total": len(plan_steps),
            "steps_completed": len(completed),
            "steps_failed": len(failed),
            "execution_order": execution_order,
            "results": {sid: res for sid, res in completed.items()},
            "failures": failed,
        }

    async def _execute_single_step(
        self,
        step: dict[str, Any],
        completed: dict[str, dict[str, Any]],
    ) -> dict[str, Any]:
        """Execute one step of the plan."""
        step_id = step["id"]
        tool = step.get("tool", "")
        input_data = dict(step.get("input", {}))

        # Inject outputs from dependencies into input
        for dep_id in step.get("depends_on", []):
            if dep_id in completed:
                dep_output = completed[dep_id].get("output", {})
                input_data[f"_dep_{dep_id}"] = dep_output

        logger.info("Executing step %s: %s (tool=%s)", step_id, step.get("description", ""), tool)

        if tool:
            result = await self.call_tool(
                tool,
                input_data,
                reason=f"Plan step {step_id}: {step.get('description', '')}",
            )
            return result

        # No tool specified — delegate as a subtask to the appropriate agent
        return await self._delegate_subtask(
            {
                "description": step.get("description", ""),
                "agent_type": step.get("agent_type", "system"),
                "input": input_data,
            },
            {"id": step_id},
        )

    # ------------------------------------------------------------------
    # Delegation
    # ------------------------------------------------------------------

    async def _delegate_subtask(
        self,
        params: dict[str, Any],
        parent_task: dict[str, Any],
    ) -> dict[str, Any]:
        """Delegate a subtask by submitting it as a goal to the orchestrator."""
        description = params.get("description", "")
        agent_type = params.get("agent_type", "")

        # Submit as a sub-goal
        from aios_agent.orchestrator_client import OrchestratorClient

        async with OrchestratorClient() as client:
            goal_id = await client.submit_goal(
                description=description,
                priority=5,
                source=self.agent_id,
                tags=["subtask", agent_type] if agent_type else ["subtask"],
                metadata={
                    "parent_task_id": parent_task.get("id", ""),
                    "agent_type_hint": agent_type,
                    "input": params.get("input", {}),
                },
            )

            # Wait for the sub-goal to complete
            try:
                status = await client.wait_for_goal(goal_id, timeout_s=SUBTASK_TIMEOUT_S)
                goal_state = status.get("goal", {}).get("status", "unknown")
                return {
                    "success": goal_state == "completed",
                    "goal_id": goal_id,
                    "status": goal_state,
                    "progress": status.get("progress_percent", 0.0),
                    "tasks": status.get("tasks", []),
                }
            except TimeoutError:
                return {
                    "success": False,
                    "goal_id": goal_id,
                    "error": f"Subtask timed out after {SUBTASK_TIMEOUT_S}s",
                }

    # ------------------------------------------------------------------
    # Integrated plan-and-execute
    # ------------------------------------------------------------------

    async def _plan_and_execute(self, task: dict[str, Any]) -> dict[str, Any]:
        """Create a plan from the task description and immediately execute it."""
        plan_result = await self._create_plan(task)
        steps = plan_result.get("steps", [])

        if not steps:
            return {
                "success": False,
                "error": "Failed to create execution plan",
                "plan": plan_result,
            }

        execution_result = await self._execute_plan(steps, task)

        # Store the complete execution as a procedure for future learning
        await self.store_decision(
            context=task.get("description", ""),
            options=[s.get("description", "") for s in steps],
            chosen="full_plan_execution",
            reasoning=f"Executed {len(steps)}-step plan",
            intelligence_level="tactical",
        )

        return {
            "success": execution_result.get("success", False),
            "plan": plan_result,
            "execution": execution_result,
        }


if __name__ == "__main__":
    import os
    agent = TaskAgent(agent_id=os.getenv("AIOS_AGENT_NAME", "task-agent"))
    asyncio.run(agent.run())
