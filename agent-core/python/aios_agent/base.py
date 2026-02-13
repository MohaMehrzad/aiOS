"""
BaseAgent — Abstract base class for all aiOS agents.

Provides gRPC channel management, tool execution, memory operations,
AI inference (think), task polling, and the main agent lifecycle loop.

Uses compiled protobuf stubs for wire-compatible gRPC communication
with Rust services (tonic/prost).
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

# Compiled protobuf stubs — wire-compatible with Rust tonic/prost services
from aios_agent.proto import common_pb2
from aios_agent.proto import orchestrator_pb2
from aios_agent.proto import orchestrator_pb2_grpc
from aios_agent.proto import tools_pb2
from aios_agent.proto import tools_pb2_grpc
from aios_agent.proto import runtime_pb2
from aios_agent.proto import runtime_pb2_grpc
from aios_agent.proto import memory_pb2
from aios_agent.proto import memory_pb2_grpc

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
    runtime_addr: str = "localhost:50055"
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
            runtime_addr=os.getenv("AIOS_RUNTIME_ADDR", "localhost:50055"),
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

        # Typed gRPC stub caches (using compiled protobuf stubs)
        self._orchestrator_stub: orchestrator_pb2_grpc.OrchestratorStub | None = None
        self._tools_stub: tools_pb2_grpc.ToolRegistryStub | None = None
        self._memory_stub: memory_pb2_grpc.MemoryServiceStub | None = None
        self._runtime_stub: runtime_pb2_grpc.AIRuntimeStub | None = None

        # Task polling interval (seconds)
        self._task_poll_interval: float = 2.0

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
    # Typed gRPC stub getters (compiled protobuf — wire-compatible)
    # ------------------------------------------------------------------

    def _get_orchestrator_stub(self) -> orchestrator_pb2_grpc.OrchestratorStub:
        if self._orchestrator_stub is None:
            self._orchestrator_stub = orchestrator_pb2_grpc.OrchestratorStub(
                self._get_orchestrator_channel()
            )
        return self._orchestrator_stub

    def _get_tools_stub(self) -> tools_pb2_grpc.ToolRegistryStub:
        if self._tools_stub is None:
            self._tools_stub = tools_pb2_grpc.ToolRegistryStub(
                self._get_tools_channel()
            )
        return self._tools_stub

    def _get_memory_stub(self) -> memory_pb2_grpc.MemoryServiceStub:
        if self._memory_stub is None:
            self._memory_stub = memory_pb2_grpc.MemoryServiceStub(
                self._get_memory_channel()
            )
        return self._memory_stub

    def _get_runtime_stub(self) -> runtime_pb2_grpc.AIRuntimeStub:
        if self._runtime_stub is None:
            self._runtime_stub = runtime_pb2_grpc.AIRuntimeStub(
                self._get_runtime_channel()
            )
        return self._runtime_stub

    # ------------------------------------------------------------------
    # Generic gRPC unary call helper (legacy — kept for memory ops)
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

        Uses compiled protobuf stubs for wire-compatible communication.
        Returns the parsed output dict together with execution metadata.
        """
        input_bytes = json.dumps(input_json or {}, default=str).encode("utf-8")
        request = tools_pb2.ExecuteRequest(
            tool_name=name,
            agent_id=self.agent_id,
            task_id=task_id or self._current_task_id or "",
            input_json=input_bytes,
            reason=reason or f"{self.agent_id} executing {name}",
        )

        logger.info("call_tool  name=%s input=%s", name, input_json)
        stub = self._get_tools_stub()
        response: tools_pb2.ExecuteResponse = await stub.Execute(
            request, timeout=self.config.grpc_timeout_s
        )

        output: dict[str, Any] = {}
        if response.output_json:
            try:
                output = json.loads(response.output_json)
            except (json.JSONDecodeError, UnicodeDecodeError):
                output = {"raw": response.output_json.hex()}

        if not response.success:
            logger.error("Tool %s failed: %s", name, response.error)
            return {
                "success": False,
                "error": response.error,
                "tool": name,
                "execution_id": response.execution_id,
                "duration_ms": response.duration_ms,
            }

        logger.info("Tool %s succeeded (exec_id=%s)", name, response.execution_id)
        return {
            "success": True,
            "output": output,
            "tool": name,
            "execution_id": response.execution_id,
            "duration_ms": response.duration_ms,
            "backup_id": response.backup_id,
        }

    async def rollback_tool(self, execution_id: str, reason: str = "") -> dict[str, Any]:
        """Rollback a previous tool execution."""
        request = tools_pb2.RollbackRequest(
            execution_id=execution_id,
            reason=reason,
        )
        stub = self._get_tools_stub()
        response = await stub.Rollback(request, timeout=self.config.grpc_timeout_s)
        return {"success": response.success, "message": response.message}

    async def list_tools(self, namespace: str = "") -> list[dict[str, Any]]:
        """List available tools, optionally filtered by namespace."""
        request = tools_pb2.ListToolsRequest(namespace=namespace)
        stub = self._get_tools_stub()
        response: tools_pb2.ListToolsResponse = await stub.ListTools(
            request, timeout=self.config.grpc_timeout_s
        )
        return [
            {
                "name": t.name,
                "namespace": t.namespace,
                "description": t.description,
            }
            for t in response.tools
        ]

    # ------------------------------------------------------------------
    # Memory operations
    # ------------------------------------------------------------------

    async def store_memory(self, key: str, value: Any, *, category: str = "agent") -> None:
        """Store a value into agent state via the MemoryService."""
        state_json = json.dumps({"key": key, "value": value}, default=str).encode("utf-8")
        request = memory_pb2.AgentState(
            agent_name=self.agent_id,
            state_json=state_json,
            updated_at=int(time.time()),
        )
        stub = self._get_memory_stub()
        await stub.StoreAgentState(request, timeout=self.config.grpc_timeout_s)
        logger.debug("store_memory key=%s", key)

    async def recall_memory(self, key: str) -> Any:
        """Recall a previously stored value from agent state."""
        request = memory_pb2.AgentStateRequest(agent_name=self.agent_id)
        stub = self._get_memory_stub()
        response: memory_pb2.AgentState = await stub.GetAgentState(
            request, timeout=self.config.grpc_timeout_s
        )
        if not response.state_json:
            return None
        try:
            state = json.loads(response.state_json)
        except (json.JSONDecodeError, UnicodeDecodeError):
            return None
        return state.get("value") if state.get("key") == key else None

    async def push_event(
        self,
        category: str,
        data: dict[str, Any],
        *,
        critical: bool = False,
    ) -> None:
        """Push an event into operational memory."""
        request = memory_pb2.Event(
            id=uuid.uuid4().hex,
            timestamp=int(time.time()),
            category=category,
            source=self.agent_id,
            data_json=json.dumps(data, default=str).encode("utf-8"),
            critical=critical,
        )
        stub = self._get_memory_stub()
        await stub.PushEvent(request, timeout=self.config.grpc_timeout_s)

    async def get_recent_events(
        self,
        count: int = 20,
        category: str = "",
        source: str = "",
    ) -> list[dict[str, Any]]:
        """Retrieve recent events from operational memory."""
        request = memory_pb2.RecentEventsRequest(
            count=count, category=category, source=source
        )
        stub = self._get_memory_stub()
        response: memory_pb2.EventList = await stub.GetRecentEvents(
            request, timeout=self.config.grpc_timeout_s
        )
        events = []
        for e in response.events:
            data = {}
            if e.data_json:
                try:
                    data = json.loads(e.data_json)
                except (json.JSONDecodeError, UnicodeDecodeError):
                    data = {"raw": e.data_json.hex()}
            events.append({
                "id": e.id,
                "timestamp": e.timestamp,
                "category": e.category,
                "source": e.source,
                "data": data,
                "critical": e.critical,
            })
        return events

    async def update_metric(self, key: str, value: float) -> None:
        """Push a metric update into operational memory."""
        request = memory_pb2.MetricUpdate(
            key=key, value=value, timestamp=int(time.time())
        )
        stub = self._get_memory_stub()
        await stub.UpdateMetric(request, timeout=self.config.grpc_timeout_s)

    async def get_metric(self, key: str) -> float | None:
        """Read a metric value from operational memory."""
        request = memory_pb2.MetricRequest(key=key)
        stub = self._get_memory_stub()
        response: memory_pb2.MetricValue = await stub.GetMetric(
            request, timeout=self.config.grpc_timeout_s
        )
        return response.value if response.key else None

    async def store_pattern(
        self,
        trigger: str,
        action: str,
        success_rate: float = 1.0,
        created_from: str = "",
    ) -> str:
        """Store a learned pattern in working memory."""
        pattern_id = uuid.uuid4().hex[:12]
        request = memory_pb2.Pattern(
            id=pattern_id,
            trigger=trigger,
            action=action,
            success_rate=success_rate,
            uses=1,
            last_used=int(time.time()),
            created_from=created_from or self.agent_id,
        )
        stub = self._get_memory_stub()
        await stub.StorePattern(request, timeout=self.config.grpc_timeout_s)
        return pattern_id

    async def find_pattern(self, trigger: str, min_success_rate: float = 0.5) -> dict[str, Any] | None:
        """Find a matching pattern for the given trigger."""
        request = memory_pb2.PatternQuery(
            trigger=trigger, min_success_rate=min_success_rate
        )
        stub = self._get_memory_stub()
        response: memory_pb2.PatternResult = await stub.FindPattern(
            request, timeout=self.config.grpc_timeout_s
        )
        if response.found and response.pattern:
            return {
                "id": response.pattern.id,
                "trigger": response.pattern.trigger,
                "action": response.pattern.action,
                "success_rate": response.pattern.success_rate,
                "uses": response.pattern.uses,
            }
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
        request = memory_pb2.Decision(
            id=uuid.uuid4().hex,
            context=context,
            options_json=json.dumps(options).encode("utf-8"),
            chosen=chosen,
            reasoning=reasoning,
            intelligence_level=intelligence_level,
            model_used="",
            outcome="",
            timestamp=int(time.time()),
        )
        stub = self._get_memory_stub()
        await stub.StoreDecision(request, timeout=self.config.grpc_timeout_s)

    async def semantic_search(
        self,
        query: str,
        collections: list[str] | None = None,
        n_results: int = 5,
    ) -> list[dict[str, Any]]:
        """Search long-term memory semantically."""
        request = memory_pb2.SemanticSearchRequest(
            query=query,
            collections=collections or [],
            n_results=n_results,
            min_relevance=0.3,
        )
        stub = self._get_memory_stub()
        response: memory_pb2.SearchResults = await stub.SemanticSearch(
            request, timeout=self.config.grpc_timeout_s
        )
        return [
            {
                "id": r.id,
                "content": r.content,
                "relevance": r.relevance,
                "collection": r.collection,
            }
            for r in response.results
        ]

    async def assemble_context(
        self,
        task_description: str,
        max_tokens: int = 4096,
        memory_tiers: list[str] | None = None,
    ) -> list[dict[str, Any]]:
        """Assemble relevant context chunks for a task."""
        request = memory_pb2.ContextRequest(
            task_description=task_description,
            max_tokens=max_tokens,
            memory_tiers=memory_tiers or ["operational", "working", "long_term"],
        )
        stub = self._get_memory_stub()
        response: memory_pb2.ContextResponse = await stub.AssembleContext(
            request, timeout=self.config.grpc_timeout_s
        )
        return [
            {
                "source": c.source,
                "content": c.content,
                "relevance": c.relevance,
                "tokens": c.tokens,
            }
            for c in response.chunks
        ]

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

        Uses compiled protobuf stubs for wire-compatible communication.
        """
        if isinstance(level, str):
            level = IntelligenceLevel(level)

        default_system = (
            f"You are the {self.get_agent_type()} agent of aiOS, an AI-native operating system. "
            f"Agent ID: {self.agent_id}. Answer concisely and precisely."
        )

        request = runtime_pb2.InferRequest(
            model="",
            prompt=prompt,
            system_prompt=system_prompt or default_system,
            max_tokens=max_tokens,
            temperature=temperature,
            intelligence_level=level.value,
            requesting_agent=self.agent_id,
            task_id=task_id or self._current_task_id or "",
        )

        logger.info("think  level=%s prompt_len=%d", level.value, len(prompt))
        stub = self._get_runtime_stub()
        response: runtime_pb2.InferResponse = await stub.Infer(
            request, timeout=self.config.grpc_timeout_s
        )
        logger.info(
            "think  response_len=%d tokens=%s model=%s",
            len(response.text),
            response.tokens_used,
            response.model_used,
        )
        return response.text

    # ------------------------------------------------------------------
    # Orchestrator registration and heartbeat
    # ------------------------------------------------------------------

    async def register_with_orchestrator(self) -> bool:
        """Register this agent with the orchestrator using typed protobuf."""
        request = common_pb2.AgentRegistration(
            agent_id=self.agent_id,
            agent_type=self.get_agent_type(),
            capabilities=self.get_capabilities(),
            tool_namespaces=[],
            status="active",
            registered_at=int(time.time()),
        )
        try:
            stub = self._get_orchestrator_stub()
            response: common_pb2.Status = await stub.RegisterAgent(
                request, timeout=self.config.grpc_timeout_s
            )
            if response.success:
                logger.info("Registered with orchestrator: %s", self.agent_id)
            else:
                logger.error("Registration rejected: %s", response.message)
            return response.success
        except Exception as exc:
            logger.error("Failed to register with orchestrator: %s", exc)
            return False

    async def unregister_from_orchestrator(self) -> bool:
        """Unregister this agent from the orchestrator using typed protobuf."""
        request = common_pb2.AgentId(id=self.agent_id)
        try:
            stub = self._get_orchestrator_stub()
            response: common_pb2.Status = await stub.UnregisterAgent(
                request, timeout=self.config.grpc_timeout_s
            )
            return response.success
        except Exception as exc:
            logger.error("Failed to unregister: %s", exc)
            return False

    async def _send_heartbeat(self) -> None:
        """Send a single heartbeat to the orchestrator using typed protobuf."""
        memory_mb = 0.0
        try:
            import resource
            rusage = resource.getrusage(resource.RUSAGE_SELF)
            memory_mb = rusage.ru_maxrss / (1024 * 1024)
        except (ImportError, AttributeError):
            pass

        request = orchestrator_pb2.HeartbeatRequest(
            agent_id=self.agent_id,
            status="busy" if self._current_task_id else "idle",
            current_task_id=self._current_task_id or "",
            cpu_usage=0.0,
            memory_usage_mb=memory_mb,
        )
        try:
            stub = self._get_orchestrator_stub()
            await stub.Heartbeat(request, timeout=5.0)
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

    async def request_capability(
        self,
        capabilities: list[str],
        reason: str = "",
        duration_hours: int = 24,
    ) -> dict[str, Any]:
        """Request additional capabilities from the orchestrator."""
        request = orchestrator_pb2.CapabilityRequest(
            agent_id=self.agent_id,
            capabilities=capabilities,
            reason=reason,
            duration_hours=duration_hours,
        )
        try:
            stub = self._get_orchestrator_stub()
            response = await stub.RequestCapability(
                request, timeout=self.config.grpc_timeout_s
            )
            return {
                "granted": response.granted,
                "capabilities": list(response.capabilities),
                "expires_at": response.expires_at,
                "denial_reason": response.denial_reason,
            }
        except Exception as exc:
            logger.error("Capability request failed: %s", exc)
            return {"granted": False, "denial_reason": str(exc)}

    # ------------------------------------------------------------------
    # Task polling — agents pull tasks from orchestrator
    # ------------------------------------------------------------------

    async def task_poll_loop(self) -> None:
        """Continuously poll for assigned tasks and execute them.

        The orchestrator assigns tasks via ``route_task()``; the agent
        fetches its assignment via ``GetAssignedTask`` and reports the
        result back via ``ReportTaskResult``.
        """
        while not self._shutdown_event.is_set():
            try:
                await self._poll_and_execute()
            except Exception as exc:
                logger.warning("Task poll error: %s", exc)

            try:
                await asyncio.wait_for(
                    self._shutdown_event.wait(),
                    timeout=self._task_poll_interval,
                )
            except asyncio.TimeoutError:
                pass

    async def _poll_and_execute(self) -> None:
        """Poll orchestrator for an assigned task, execute it, report result."""
        stub = self._get_orchestrator_stub()
        request = common_pb2.AgentId(id=self.agent_id)

        try:
            task: common_pb2.Task = await stub.GetAssignedTask(request, timeout=5.0)
        except grpc.aio.AioRpcError as exc:
            if exc.code() != grpc.StatusCode.UNAVAILABLE:
                logger.warning("GetAssignedTask RPC failed: %s", exc.code())
            return

        # Empty task means nothing assigned — id will be empty string
        if not task.id:
            return

        logger.info(
            "Received task %s: %s", task.id, task.description[:80] if task.description else ""
        )

        # Convert protobuf Task to dict for handle_task
        task_dict: dict[str, Any] = {
            "id": task.id,
            "goal_id": task.goal_id,
            "description": task.description,
            "assigned_agent": task.assigned_agent,
            "status": task.status,
            "intelligence_level": task.intelligence_level,
            "required_tools": list(task.required_tools),
            "depends_on": list(task.depends_on),
            "input_json": bytes(task.input_json),
            "created_at": task.created_at,
        }

        # Execute the task
        result = await self.execute_task(task_dict)

        # Report result back to orchestrator
        output_bytes = json.dumps(result.get("output_json", {}), default=str).encode("utf-8")
        report = common_pb2.TaskResult(
            task_id=task.id,
            success=result.get("success", False),
            output_json=output_bytes,
            error=result.get("error", ""),
            duration_ms=result.get("duration_ms", 0),
            tokens_used=result.get("tokens_used", 0),
            model_used=result.get("model_used", ""),
        )

        try:
            await stub.ReportTaskResult(report, timeout=10.0)
            logger.info("Reported result for task %s (success=%s)", task.id, result.get("success"))
        except grpc.aio.AioRpcError as exc:
            logger.error("Failed to report task result for %s: %s", task.id, exc)

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
        """Main lifecycle: register, heartbeat, poll tasks, and execute.

        Registers with the orchestrator, then runs the heartbeat loop
        and task polling loop concurrently.  Subclasses that have their
        own background loops should override ``run()`` and call
        ``super().run()`` inside an ``asyncio.gather``.
        """
        self._running = True
        try:
            registered = await self.register_with_orchestrator()
            if not registered:
                logger.warning("Running without orchestrator registration")

            heartbeat_task = asyncio.create_task(self.heartbeat_loop())
            poll_task = asyncio.create_task(self.task_poll_loop())

            logger.info("Agent %s is running (heartbeat + task polling)", self.agent_id)
            await self._shutdown_event.wait()

            for t in (heartbeat_task, poll_task):
                t.cancel()
                try:
                    await t
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
