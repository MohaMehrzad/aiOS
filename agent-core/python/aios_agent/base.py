"""
BaseAgent — Abstract base class for all aiOS agents.

Provides gRPC channel management, tool execution, memory operations,
AI inference (think), and the main agent lifecycle loop.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import time
import uuid
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from enum import Enum
from typing import Any

import grpc

logger = logging.getLogger("aios.agent")


class IntelligenceLevel(str, Enum):
    """Intelligence levels for the think() dispatcher.

    reactive    - Pattern-matched instant responses, no LLM call.
    operational - Small/fast model for routine decisions.
    tactical    - Medium model for multi-step reasoning.
    strategic   - Largest available model for complex planning.
    """

    REACTIVE = "reactive"
    OPERATIONAL = "operational"
    TACTICAL = "tactical"
    STRATEGIC = "strategic"


@dataclass
class AgentConfig:
    """Runtime configuration for an agent instance."""

    orchestrator_addr: str = "localhost:50051"
    tools_addr: str = "localhost:50052"
    memory_addr: str = "localhost:50053"
    runtime_addr: str = "localhost:50054"
    heartbeat_interval_s: float = 10.0
    max_retries: int = 3
    retry_delay_s: float = 1.0
    grpc_timeout_s: float = 30.0
    log_level: str = "INFO"
    extra: dict[str, Any] = field(default_factory=dict)


class BaseAgent(ABC):
    """Abstract base class for every aiOS agent.

    Subclasses must implement:
      - handle_task(task)   — process a Task dispatched by the orchestrator
      - get_capabilities()  — return list of capability strings
      - get_agent_type()    — return the agent type name
    """

    # ------------------------------------------------------------------
    # Construction
    # ------------------------------------------------------------------

    def __init__(self, agent_id: str | None = None, config: AgentConfig | None = None) -> None:
        self.agent_id: str = agent_id or f"{self.get_agent_type()}-{uuid.uuid4().hex[:8]}"
        self.config: AgentConfig = config or AgentConfig(
            orchestrator_addr=os.getenv("AIOS_ORCHESTRATOR_ADDR", "localhost:50051"),
            tools_addr=os.getenv("AIOS_TOOLS_ADDR", "localhost:50052"),
            memory_addr=os.getenv("AIOS_MEMORY_ADDR", "localhost:50053"),
            runtime_addr=os.getenv("AIOS_RUNTIME_ADDR", "localhost:50054"),
        )
        self._start_time: float = time.time()
        self._tasks_completed: int = 0
        self._tasks_failed: int = 0
        self._current_task_id: str | None = None
        self._running: bool = False
        self._shutdown_event: asyncio.Event = asyncio.Event()

        # gRPC channels (lazily created)
        self._orchestrator_channel: grpc.aio.Channel | None = None
        self._tools_channel: grpc.aio.Channel | None = None
        self._memory_channel: grpc.aio.Channel | None = None
        self._runtime_channel: grpc.aio.Channel | None = None

        # Stub caches
        self._orchestrator_stub: Any = None
        self._tools_stub: Any = None
        self._memory_stub: Any = None
        self._runtime_stub: Any = None

        logging.basicConfig(level=getattr(logging, self.config.log_level, logging.INFO))
        logger.info("Agent %s (%s) initialised", self.agent_id, self.get_agent_type())

    # ------------------------------------------------------------------
    # Abstract interface
    # ------------------------------------------------------------------

    @abstractmethod
    async def handle_task(self, task: dict[str, Any]) -> dict[str, Any]:
        """Process a task dict and return a result dict.

        The *task* dictionary mirrors the ``aios.common.Task`` proto:
            id, goal_id, description, assigned_agent, status,
            intelligence_level, required_tools, depends_on,
            input_json (already deserialised), ...
        The returned dict is serialised into ``TaskResult.output_json``.
        """
        ...

    @abstractmethod
    def get_capabilities(self) -> list[str]:
        """Return capability strings this agent advertises."""
        ...

    @abstractmethod
    def get_agent_type(self) -> str:
        """Return the canonical agent type name (e.g. 'system', 'network')."""
        ...

    # ------------------------------------------------------------------
    # gRPC channel helpers
    # ------------------------------------------------------------------

    def _get_orchestrator_channel(self) -> grpc.aio.Channel:
        if self._orchestrator_channel is None:
            self._orchestrator_channel = grpc.aio.insecure_channel(self.config.orchestrator_addr)
        return self._orchestrator_channel

    def _get_tools_channel(self) -> grpc.aio.Channel:
        if self._tools_channel is None:
            self._tools_channel = grpc.aio.insecure_channel(self.config.tools_addr)
        return self._tools_channel

    def _get_memory_channel(self) -> grpc.aio.Channel:
        if self._memory_channel is None:
            self._memory_channel = grpc.aio.insecure_channel(self.config.memory_addr)
        return self._memory_channel

    def _get_runtime_channel(self) -> grpc.aio.Channel:
        if self._runtime_channel is None:
            self._runtime_channel = grpc.aio.insecure_channel(self.config.runtime_addr)
        return self._runtime_channel

    # ------------------------------------------------------------------
    # Generic gRPC unary call helper
    # ------------------------------------------------------------------

    async def _grpc_call(
        self,
        channel: grpc.aio.Channel,
        service_path: str,
        method: str,
        request_data: bytes,
        *,
        timeout: float | None = None,
    ) -> bytes:
        """Perform a raw unary-unary gRPC call and return response bytes.

        This avoids depending on compiled protobuf Python stubs — we
        serialise / deserialise at the call-site using ``json`` and
        hand-crafted proto-like dicts.  The approach lets agents run
        without a build step while remaining wire-compatible with the
        Rust services that *do* use fully-typed protos.
        """
        timeout = timeout or self.config.grpc_timeout_s
        full_method = f"/{service_path}/{method}"
        call = channel.unary_unary(
            full_method,
            request_serializer=lambda x: x,
            response_deserializer=lambda x: x,
        )
        for attempt in range(1, self.config.max_retries + 1):
            try:
                response: bytes = await call(request_data, timeout=timeout)
                return response
            except grpc.aio.AioRpcError as exc:
                if exc.code() in (grpc.StatusCode.UNAVAILABLE, grpc.StatusCode.DEADLINE_EXCEEDED):
                    if attempt < self.config.max_retries:
                        wait = self.config.retry_delay_s * attempt
                        logger.warning(
                            "gRPC call %s attempt %d/%d failed (%s), retrying in %.1fs",
                            full_method,
                            attempt,
                            self.config.max_retries,
                            exc.code(),
                            wait,
                        )
                        await asyncio.sleep(wait)
                        continue
                raise
        raise RuntimeError(f"gRPC call {full_method} failed after {self.config.max_retries} retries")

    # ------------------------------------------------------------------
    # Proto-lite serialisation helpers
    # ------------------------------------------------------------------

    @staticmethod
    def _encode_proto_json(data: dict[str, Any]) -> bytes:
        """Encode a dict as JSON bytes for use as proto ``bytes`` fields."""
        return json.dumps(data, default=str).encode("utf-8")

    @staticmethod
    def _decode_proto_json(raw: bytes) -> Any:
        """Decode JSON bytes from a proto ``bytes`` field."""
        if not raw:
            return {}
        try:
            return json.loads(raw)
        except (json.JSONDecodeError, UnicodeDecodeError):
            return {"_raw": raw.hex()}

    # ------------------------------------------------------------------
    # Tool execution
    # ------------------------------------------------------------------

    async def call_tool(
        self,
        name: str,
        input_json: dict[str, Any] | None = None,
        *,
        reason: str = "",
        task_id: str | None = None,
    ) -> dict[str, Any]:
        """Execute a tool via the ToolRegistry gRPC service.

        Returns the parsed output dict together with execution metadata.
        Raises ``RuntimeError`` when the tool reports failure.
        """
        payload = {
            "tool_name": name,
            "agent_id": self.agent_id,
            "task_id": task_id or self._current_task_id or "",
            "input_json": self._encode_proto_json(input_json or {}).decode(),
            "reason": reason or f"{self.agent_id} executing {name}",
        }
        request_bytes = self._encode_proto_json(payload)

        logger.info("call_tool  name=%s input=%s", name, input_json)
        response_bytes = await self._grpc_call(
            self._get_tools_channel(),
            "aios.tools.ToolRegistry",
            "Execute",
            request_bytes,
        )
        result = self._decode_proto_json(response_bytes)

        output = {}
        if isinstance(result.get("output_json"), (str, bytes)):
            try:
                raw = result["output_json"]
                output = json.loads(raw) if isinstance(raw, str) else json.loads(raw.decode())
            except (json.JSONDecodeError, UnicodeDecodeError):
                output = {"raw": result.get("output_json")}
        elif isinstance(result.get("output_json"), dict):
            output = result["output_json"]

        success = result.get("success", False)
        if not success:
            error_msg = result.get("error", "Unknown tool error")
            logger.error("Tool %s failed: %s", name, error_msg)
            return {
                "success": False,
                "error": error_msg,
                "tool": name,
                "execution_id": result.get("execution_id", ""),
                "duration_ms": result.get("duration_ms", 0),
            }

        logger.info("Tool %s succeeded (exec_id=%s)", name, result.get("execution_id", ""))
        return {
            "success": True,
            "output": output,
            "tool": name,
            "execution_id": result.get("execution_id", ""),
            "duration_ms": result.get("duration_ms", 0),
            "backup_id": result.get("backup_id", ""),
        }

    async def rollback_tool(self, execution_id: str, reason: str = "") -> dict[str, Any]:
        """Rollback a previous tool execution."""
        payload = {"execution_id": execution_id, "reason": reason}
        response_bytes = await self._grpc_call(
            self._get_tools_channel(),
            "aios.tools.ToolRegistry",
            "Rollback",
            self._encode_proto_json(payload),
        )
        return self._decode_proto_json(response_bytes)

    async def list_tools(self, namespace: str = "") -> list[dict[str, Any]]:
        """List available tools, optionally filtered by namespace."""
        payload = {"namespace": namespace}
        response_bytes = await self._grpc_call(
            self._get_tools_channel(),
            "aios.tools.ToolRegistry",
            "ListTools",
            self._encode_proto_json(payload),
        )
        result = self._decode_proto_json(response_bytes)
        return result.get("tools", [])

    # ------------------------------------------------------------------
    # Memory operations
    # ------------------------------------------------------------------

    async def store_memory(self, key: str, value: Any, *, category: str = "agent") -> None:
        """Store a value into agent state via the MemoryService."""
        state_json = json.dumps({"key": key, "value": value}, default=str).encode("utf-8")
        payload = {
            "agent_name": self.agent_id,
            "state_json": state_json.decode("utf-8"),
            "updated_at": int(time.time()),
        }
        await self._grpc_call(
            self._get_memory_channel(),
            "aios.memory.MemoryService",
            "StoreAgentState",
            self._encode_proto_json(payload),
        )
        logger.debug("store_memory key=%s", key)

    async def recall_memory(self, key: str) -> Any:
        """Recall a previously stored value from agent state."""
        payload = {"agent_name": self.agent_id}
        response_bytes = await self._grpc_call(
            self._get_memory_channel(),
            "aios.memory.MemoryService",
            "GetAgentState",
            self._encode_proto_json(payload),
        )
        result = self._decode_proto_json(response_bytes)
        state_raw = result.get("state_json", "{}")
        if isinstance(state_raw, str):
            try:
                state = json.loads(state_raw)
            except json.JSONDecodeError:
                state = {}
        elif isinstance(state_raw, dict):
            state = state_raw
        else:
            state = {}
        return state.get("value") if state.get("key") == key else None

    async def push_event(
        self,
        category: str,
        data: dict[str, Any],
        *,
        critical: bool = False,
    ) -> None:
        """Push an event into operational memory."""
        payload = {
            "id": uuid.uuid4().hex,
            "timestamp": int(time.time()),
            "category": category,
            "source": self.agent_id,
            "data_json": json.dumps(data, default=str),
            "critical": critical,
        }
        await self._grpc_call(
            self._get_memory_channel(),
            "aios.memory.MemoryService",
            "PushEvent",
            self._encode_proto_json(payload),
        )

    async def get_recent_events(
        self,
        count: int = 20,
        category: str = "",
        source: str = "",
    ) -> list[dict[str, Any]]:
        """Retrieve recent events from operational memory."""
        payload = {"count": count, "category": category, "source": source}
        response_bytes = await self._grpc_call(
            self._get_memory_channel(),
            "aios.memory.MemoryService",
            "GetRecentEvents",
            self._encode_proto_json(payload),
        )
        result = self._decode_proto_json(response_bytes)
        return result.get("events", [])

    async def update_metric(self, key: str, value: float) -> None:
        """Push a metric update into operational memory."""
        payload = {"key": key, "value": value, "timestamp": int(time.time())}
        await self._grpc_call(
            self._get_memory_channel(),
            "aios.memory.MemoryService",
            "UpdateMetric",
            self._encode_proto_json(payload),
        )

    async def get_metric(self, key: str) -> float | None:
        """Read a metric value from operational memory."""
        payload = {"key": key}
        response_bytes = await self._grpc_call(
            self._get_memory_channel(),
            "aios.memory.MemoryService",
            "GetMetric",
            self._encode_proto_json(payload),
        )
        result = self._decode_proto_json(response_bytes)
        return result.get("value")

    async def store_pattern(
        self,
        trigger: str,
        action: str,
        success_rate: float = 1.0,
        created_from: str = "",
    ) -> str:
        """Store a learned pattern in working memory."""
        pattern_id = uuid.uuid4().hex[:12]
        payload = {
            "id": pattern_id,
            "trigger": trigger,
            "action": action,
            "success_rate": success_rate,
            "uses": 1,
            "last_used": int(time.time()),
            "created_from": created_from or self.agent_id,
        }
        await self._grpc_call(
            self._get_memory_channel(),
            "aios.memory.MemoryService",
            "StorePattern",
            self._encode_proto_json(payload),
        )
        return pattern_id

    async def find_pattern(self, trigger: str, min_success_rate: float = 0.5) -> dict[str, Any] | None:
        """Find a matching pattern for the given trigger."""
        payload = {"trigger": trigger, "min_success_rate": min_success_rate}
        response_bytes = await self._grpc_call(
            self._get_memory_channel(),
            "aios.memory.MemoryService",
            "FindPattern",
            self._encode_proto_json(payload),
        )
        result = self._decode_proto_json(response_bytes)
        if result.get("found"):
            return result.get("pattern")
        return None

    async def store_decision(
        self,
        context: str,
        options: list[str],
        chosen: str,
        reasoning: str,
        intelligence_level: str = "reactive",
    ) -> None:
        """Log a decision to working memory for future learning."""
        payload = {
            "id": uuid.uuid4().hex,
            "context": context,
            "options_json": json.dumps(options),
            "chosen": chosen,
            "reasoning": reasoning,
            "intelligence_level": intelligence_level,
            "model_used": "",
            "outcome": "",
            "timestamp": int(time.time()),
        }
        await self._grpc_call(
            self._get_memory_channel(),
            "aios.memory.MemoryService",
            "StoreDecision",
            self._encode_proto_json(payload),
        )

    async def semantic_search(
        self,
        query: str,
        collections: list[str] | None = None,
        n_results: int = 5,
    ) -> list[dict[str, Any]]:
        """Search long-term memory semantically."""
        payload = {
            "query": query,
            "collections": collections or [],
            "n_results": n_results,
            "min_relevance": 0.3,
        }
        response_bytes = await self._grpc_call(
            self._get_memory_channel(),
            "aios.memory.MemoryService",
            "SemanticSearch",
            self._encode_proto_json(payload),
        )
        result = self._decode_proto_json(response_bytes)
        return result.get("results", [])

    async def assemble_context(
        self,
        task_description: str,
        max_tokens: int = 4096,
        memory_tiers: list[str] | None = None,
    ) -> list[dict[str, Any]]:
        """Assemble relevant context chunks for a task."""
        payload = {
            "task_description": task_description,
            "max_tokens": max_tokens,
            "memory_tiers": memory_tiers or ["operational", "working", "long_term"],
        }
        response_bytes = await self._grpc_call(
            self._get_memory_channel(),
            "aios.memory.MemoryService",
            "AssembleContext",
            self._encode_proto_json(payload),
        )
        result = self._decode_proto_json(response_bytes)
        return result.get("chunks", [])

    # ------------------------------------------------------------------
    # AI inference (think)
    # ------------------------------------------------------------------

    async def think(
        self,
        prompt: str,
        level: IntelligenceLevel | str = IntelligenceLevel.OPERATIONAL,
        *,
        system_prompt: str = "",
        max_tokens: int = 1024,
        temperature: float = 0.3,
        task_id: str | None = None,
    ) -> str:
        """Request AI inference from the runtime at the specified intelligence level.

        For *reactive* level the agent should handle logic locally; this
        method still sends the call so the runtime can do fast pattern
        matching if available.
        """
        if isinstance(level, str):
            level = IntelligenceLevel(level)

        default_system = (
            f"You are the {self.get_agent_type()} agent of aiOS, an AI-native operating system. "
            f"Agent ID: {self.agent_id}. Answer concisely and precisely."
        )

        payload = {
            "model": "",
            "prompt": prompt,
            "system_prompt": system_prompt or default_system,
            "max_tokens": max_tokens,
            "temperature": temperature,
            "intelligence_level": level.value,
            "requesting_agent": self.agent_id,
            "task_id": task_id or self._current_task_id or "",
        }

        logger.info("think  level=%s prompt_len=%d", level.value, len(prompt))
        response_bytes = await self._grpc_call(
            self._get_runtime_channel(),
            "aios.runtime.AIRuntime",
            "Infer",
            self._encode_proto_json(payload),
        )
        result = self._decode_proto_json(response_bytes)
        text = result.get("text", "")
        logger.info(
            "think  response_len=%d tokens=%s model=%s",
            len(text),
            result.get("tokens_used", "?"),
            result.get("model_used", "?"),
        )
        return text

    # ------------------------------------------------------------------
    # Orchestrator registration and heartbeat
    # ------------------------------------------------------------------

    async def register_with_orchestrator(self) -> bool:
        """Register this agent with the orchestrator."""
        payload = {
            "agent_id": self.agent_id,
            "agent_type": self.get_agent_type(),
            "capabilities": self.get_capabilities(),
            "tool_namespaces": [],
            "status": "active",
            "registered_at": int(time.time()),
        }
        try:
            response_bytes = await self._grpc_call(
                self._get_orchestrator_channel(),
                "aios.orchestrator.Orchestrator",
                "RegisterAgent",
                self._encode_proto_json(payload),
            )
            result = self._decode_proto_json(response_bytes)
            success = result.get("success", False)
            if success:
                logger.info("Registered with orchestrator: %s", self.agent_id)
            else:
                logger.error("Registration rejected: %s", result.get("message", ""))
            return success
        except Exception as exc:
            logger.error("Failed to register with orchestrator: %s", exc)
            return False

    async def unregister_from_orchestrator(self) -> bool:
        """Unregister this agent from the orchestrator."""
        payload = {"id": self.agent_id}
        try:
            response_bytes = await self._grpc_call(
                self._get_orchestrator_channel(),
                "aios.orchestrator.Orchestrator",
                "UnregisterAgent",
                self._encode_proto_json(payload),
            )
            result = self._decode_proto_json(response_bytes)
            return result.get("success", False)
        except Exception as exc:
            logger.error("Failed to unregister: %s", exc)
            return False

    async def _send_heartbeat(self) -> None:
        """Send a single heartbeat to the orchestrator."""
        payload = {
            "agent_id": self.agent_id,
            "status": "busy" if self._current_task_id else "idle",
            "current_task_id": self._current_task_id or "",
            "cpu_usage": 0.0,
            "memory_usage_mb": 0.0,
        }
        try:
            import resource

            rusage = resource.getrusage(resource.RUSAGE_SELF)
            payload["memory_usage_mb"] = rusage.ru_maxrss / (1024 * 1024)
        except (ImportError, AttributeError):
            pass

        try:
            await self._grpc_call(
                self._get_orchestrator_channel(),
                "aios.orchestrator.Orchestrator",
                "Heartbeat",
                self._encode_proto_json(payload),
                timeout=5.0,
            )
        except Exception as exc:
            logger.warning("Heartbeat failed: %s", exc)

    async def heartbeat_loop(self) -> None:
        """Continuously send heartbeats until shutdown."""
        while not self._shutdown_event.is_set():
            await self._send_heartbeat()
            try:
                await asyncio.wait_for(
                    self._shutdown_event.wait(),
                    timeout=self.config.heartbeat_interval_s,
                )
            except asyncio.TimeoutError:
                pass

    # ------------------------------------------------------------------
    # Task execution wrapper
    # ------------------------------------------------------------------

    async def execute_task(self, task: dict[str, Any]) -> dict[str, Any]:
        """Wrapper around handle_task that handles bookkeeping."""
        task_id = task.get("id", uuid.uuid4().hex)
        self._current_task_id = task_id
        start = time.time()

        try:
            logger.info("Executing task %s: %s", task_id, task.get("description", "")[:80])

            # Deserialise input_json if present as raw bytes/string
            if isinstance(task.get("input_json"), (str, bytes)):
                try:
                    raw = task["input_json"]
                    task["input_json"] = json.loads(raw) if isinstance(raw, str) else json.loads(raw.decode())
                except (json.JSONDecodeError, UnicodeDecodeError):
                    task["input_json"] = {}

            result = await self.handle_task(task)
            duration_ms = int((time.time() - start) * 1000)

            self._tasks_completed += 1
            logger.info("Task %s completed in %dms", task_id, duration_ms)

            return {
                "task_id": task_id,
                "success": True,
                "output_json": result,
                "error": "",
                "duration_ms": duration_ms,
                "tokens_used": 0,
                "model_used": "",
            }

        except Exception as exc:
            duration_ms = int((time.time() - start) * 1000)
            self._tasks_failed += 1
            logger.error("Task %s failed: %s", task_id, exc, exc_info=True)
            return {
                "task_id": task_id,
                "success": False,
                "output_json": {},
                "error": str(exc),
                "duration_ms": duration_ms,
                "tokens_used": 0,
                "model_used": "",
            }
        finally:
            self._current_task_id = None

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    async def _close_channels(self) -> None:
        for ch in (
            self._orchestrator_channel,
            self._tools_channel,
            self._memory_channel,
            self._runtime_channel,
        ):
            if ch is not None:
                await ch.close()

    async def run(self) -> None:
        """Main lifecycle: register, heartbeat, and listen for tasks.

        The default implementation registers with the orchestrator, starts
        the heartbeat loop, and waits for shutdown.  Subclasses that have
        their own background loops should override ``run()`` and call
        ``super().run()`` inside an ``asyncio.gather``.
        """
        self._running = True
        try:
            registered = await self.register_with_orchestrator()
            if not registered:
                logger.warning("Running without orchestrator registration")

            heartbeat_task = asyncio.create_task(self.heartbeat_loop())

            logger.info("Agent %s is running", self.agent_id)
            await self._shutdown_event.wait()
            heartbeat_task.cancel()
            try:
                await heartbeat_task
            except asyncio.CancelledError:
                pass
        finally:
            await self.unregister_from_orchestrator()
            await self._close_channels()
            self._running = False
            logger.info("Agent %s shut down", self.agent_id)

    def shutdown(self) -> None:
        """Signal the agent to gracefully shut down."""
        logger.info("Shutdown requested for %s", self.agent_id)
        self._shutdown_event.set()

    @property
    def uptime_seconds(self) -> int:
        return int(time.time() - self._start_time)

    def get_status(self) -> dict[str, Any]:
        """Return a status snapshot for this agent."""
        return {
            "agent_id": self.agent_id,
            "agent_type": self.get_agent_type(),
            "status": "busy" if self._current_task_id else ("running" if self._running else "stopped"),
            "current_task_id": self._current_task_id or "",
            "tasks_completed": self._tasks_completed,
            "tasks_failed": self._tasks_failed,
            "uptime_seconds": self.uptime_seconds,
        }
