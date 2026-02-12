"""
OrchestratorClient — High-level wrapper around the Orchestrator gRPC service.

Provides a clean async API for submitting goals, querying status, managing
agents, and obtaining system-wide information from the aiOS orchestrator.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import time
from dataclasses import dataclass, field
from typing import Any

import grpc

logger = logging.getLogger("aios.orchestrator_client")


@dataclass
class OrchestratorClientConfig:
    """Connection settings for the orchestrator client."""

    address: str = "localhost:50051"
    timeout_s: float = 30.0
    max_retries: int = 3
    retry_delay_s: float = 1.0


class OrchestratorClient:
    """Async client for the aiOS Orchestrator gRPC service.

    Usage::

        async with OrchestratorClient() as client:
            goal_id = await client.submit_goal("Install nginx and configure it")
            status = await client.get_goal_status(goal_id)
    """

    def __init__(self, config: OrchestratorClientConfig | None = None) -> None:
        self.config = config or OrchestratorClientConfig(
            address=os.getenv("AIOS_ORCHESTRATOR_ADDR", "localhost:50051"),
        )
        self._channel: grpc.aio.Channel | None = None

    # ------------------------------------------------------------------
    # Context manager
    # ------------------------------------------------------------------

    async def __aenter__(self) -> OrchestratorClient:
        self.connect()
        return self

    async def __aexit__(self, *_: Any) -> None:
        await self.close()

    def connect(self) -> None:
        """Open the gRPC channel (idempotent)."""
        if self._channel is None:
            self._channel = grpc.aio.insecure_channel(self.config.address)
            logger.info("Connected to orchestrator at %s", self.config.address)

    async def close(self) -> None:
        """Close the gRPC channel."""
        if self._channel is not None:
            await self._channel.close()
            self._channel = None
            logger.info("Disconnected from orchestrator")

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    @staticmethod
    def _encode(data: dict[str, Any]) -> bytes:
        return json.dumps(data, default=str).encode("utf-8")

    @staticmethod
    def _decode(raw: bytes) -> dict[str, Any]:
        if not raw:
            return {}
        try:
            return json.loads(raw)
        except (json.JSONDecodeError, UnicodeDecodeError):
            return {"_raw": raw.hex()}

    async def _call(self, method: str, payload: dict[str, Any]) -> dict[str, Any]:
        """Perform a unary call to the Orchestrator service with retries."""
        if self._channel is None:
            self.connect()
        assert self._channel is not None

        full_method = f"/aios.orchestrator.Orchestrator/{method}"
        call = self._channel.unary_unary(
            full_method,
            request_serializer=lambda x: x,
            response_deserializer=lambda x: x,
        )
        request_bytes = self._encode(payload)

        last_exc: Exception | None = None
        for attempt in range(1, self.config.max_retries + 1):
            try:
                response_bytes: bytes = await call(request_bytes, timeout=self.config.timeout_s)
                return self._decode(response_bytes)
            except grpc.aio.AioRpcError as exc:
                last_exc = exc
                if exc.code() in (grpc.StatusCode.UNAVAILABLE, grpc.StatusCode.DEADLINE_EXCEEDED):
                    if attempt < self.config.max_retries:
                        wait = self.config.retry_delay_s * attempt
                        logger.warning(
                            "%s attempt %d/%d failed (%s), retrying in %.1fs",
                            method,
                            attempt,
                            self.config.max_retries,
                            exc.code(),
                            wait,
                        )
                        await asyncio.sleep(wait)
                        continue
                raise

        raise RuntimeError(
            f"Orchestrator call {method} failed after {self.config.max_retries} retries: {last_exc}"
        )

    # ------------------------------------------------------------------
    # Goal management
    # ------------------------------------------------------------------

    async def submit_goal(
        self,
        description: str,
        priority: int = 5,
        source: str = "python-client",
        tags: list[str] | None = None,
        metadata: dict[str, Any] | None = None,
    ) -> str:
        """Submit a new goal and return its ID.

        Parameters
        ----------
        description:
            Natural-language description of the goal.
        priority:
            Priority 1 (lowest) to 10 (highest).
        source:
            Origin identifier (agent name, CLI, API, etc.).
        tags:
            Optional tags for categorisation.
        metadata:
            Arbitrary metadata dict serialised to JSON bytes.
        """
        payload: dict[str, Any] = {
            "description": description,
            "priority": priority,
            "source": source,
            "tags": tags or [],
        }
        if metadata:
            payload["metadata_json"] = json.dumps(metadata, default=str)

        result = await self._call("SubmitGoal", payload)
        goal_id = result.get("id", "")
        logger.info("Submitted goal '%s' -> %s", description[:60], goal_id)
        return goal_id

    async def get_goal_status(self, goal_id: str) -> dict[str, Any]:
        """Get the current status, tasks, and progress of a goal.

        Returns a dict with keys: goal, tasks, current_phase, progress_percent.
        """
        result = await self._call("GetGoalStatus", {"id": goal_id})
        return {
            "goal": result.get("goal", {}),
            "tasks": result.get("tasks", []),
            "current_phase": result.get("current_phase", "unknown"),
            "progress_percent": result.get("progress_percent", 0.0),
        }

    async def cancel_goal(self, goal_id: str) -> bool:
        """Cancel an active goal. Returns True on success."""
        result = await self._call("CancelGoal", {"id": goal_id})
        success = result.get("success", False)
        if success:
            logger.info("Cancelled goal %s", goal_id)
        else:
            logger.warning("Cancel goal %s failed: %s", goal_id, result.get("message", ""))
        return success

    async def list_goals(
        self,
        status_filter: str = "",
        limit: int = 50,
        offset: int = 0,
    ) -> tuple[list[dict[str, Any]], int]:
        """List goals with optional filtering.

        Returns (goals_list, total_count).
        """
        result = await self._call(
            "ListGoals",
            {"status_filter": status_filter, "limit": limit, "offset": offset},
        )
        return result.get("goals", []), result.get("total", 0)

    # ------------------------------------------------------------------
    # Agent registration
    # ------------------------------------------------------------------

    async def register_agent(
        self,
        agent_id: str,
        agent_type: str,
        capabilities: list[str],
        tool_namespaces: list[str] | None = None,
    ) -> bool:
        """Register an agent with the orchestrator."""
        payload = {
            "agent_id": agent_id,
            "agent_type": agent_type,
            "capabilities": capabilities,
            "tool_namespaces": tool_namespaces or [],
            "status": "active",
            "registered_at": int(time.time()),
        }
        result = await self._call("RegisterAgent", payload)
        return result.get("success", False)

    async def unregister_agent(self, agent_id: str) -> bool:
        """Unregister an agent from the orchestrator."""
        result = await self._call("UnregisterAgent", {"id": agent_id})
        return result.get("success", False)

    async def heartbeat(
        self,
        agent_id: str,
        status: str = "idle",
        current_task_id: str = "",
        cpu_usage: float = 0.0,
        memory_usage_mb: float = 0.0,
    ) -> bool:
        """Send a heartbeat for a specific agent."""
        payload = {
            "agent_id": agent_id,
            "status": status,
            "current_task_id": current_task_id,
            "cpu_usage": cpu_usage,
            "memory_usage_mb": memory_usage_mb,
        }
        result = await self._call("Heartbeat", payload)
        return result.get("success", False)

    async def list_agents(self) -> list[dict[str, Any]]:
        """List all registered agents."""
        result = await self._call("ListAgents", {})
        return result.get("agents", [])

    # ------------------------------------------------------------------
    # System status
    # ------------------------------------------------------------------

    async def get_system_status(self) -> dict[str, Any]:
        """Get the overall system status from the orchestrator.

        Returns dict with: active_goals, pending_tasks, active_agents,
        loaded_models, cpu_percent, memory_used_mb, memory_total_mb,
        autonomy_level, uptime_seconds.
        """
        result = await self._call("GetSystemStatus", {})
        return {
            "active_goals": result.get("active_goals", 0),
            "pending_tasks": result.get("pending_tasks", 0),
            "active_agents": result.get("active_agents", 0),
            "loaded_models": result.get("loaded_models", []),
            "cpu_percent": result.get("cpu_percent", 0.0),
            "memory_used_mb": result.get("memory_used_mb", 0.0),
            "memory_total_mb": result.get("memory_total_mb", 0.0),
            "autonomy_level": result.get("autonomy_level", "unknown"),
            "uptime_seconds": result.get("uptime_seconds", 0),
        }

    # ------------------------------------------------------------------
    # Convenience: wait for goal completion
    # ------------------------------------------------------------------

    async def wait_for_goal(
        self,
        goal_id: str,
        poll_interval_s: float = 2.0,
        timeout_s: float = 300.0,
    ) -> dict[str, Any]:
        """Poll until a goal reaches a terminal state or timeout.

        Returns the final GoalStatusResponse dict.
        Raises ``TimeoutError`` if the goal does not complete within *timeout_s*.
        """
        terminal_states = {"completed", "failed", "cancelled"}
        deadline = time.time() + timeout_s

        while time.time() < deadline:
            status = await self.get_goal_status(goal_id)
            goal_status = status.get("goal", {}).get("status", "")
            if goal_status.lower() in terminal_states:
                return status
            await asyncio.sleep(poll_interval_s)

        raise TimeoutError(f"Goal {goal_id} did not complete within {timeout_s}s")
