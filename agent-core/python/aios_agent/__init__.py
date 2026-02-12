"""
aios-agent — Python agent framework for aiOS, the AI-native operating system.

This package provides the ``BaseAgent`` abstract class and concrete agent
implementations that communicate with the Rust orchestrator, tool registry,
memory service, and AI runtime via gRPC.

Quick start::

    from aios_agent import BaseAgent, SystemAgent, AgentConfig

    agent = SystemAgent()
    asyncio.run(agent.run())

Agent types
-----------
SystemAgent     — CPU/RAM/disk monitoring, service management
TaskAgent       — General-purpose goal decomposition and execution
NetworkAgent    — Network interfaces, DNS, connectivity, DHCP
SecurityAgent   — IDS, vulnerability scanning, policy enforcement
PackageAgent    — Package install/remove/update, CVE checking
StorageAgent    — Disk health, backups, mount management
MonitoringAgent — Metrics collection, alerting, anomaly detection
LearningAgent   — Pattern recognition, parameter optimisation
"""

from aios_agent.base import AgentConfig, BaseAgent, IntelligenceLevel
from aios_agent.orchestrator_client import OrchestratorClient, OrchestratorClientConfig

# Lazy imports to avoid circular dependencies and keep startup fast.
# The agents sub-package re-exports everything from a single entry point.

__version__ = "0.1.0"

__all__ = [
    # Core
    "BaseAgent",
    "AgentConfig",
    "IntelligenceLevel",
    "OrchestratorClient",
    "OrchestratorClientConfig",
    # Agent types (re-exported from agents sub-package)
    "SystemAgent",
    "TaskAgent",
    "NetworkAgent",
    "SecurityAgent",
    "PackageAgent",
    "StorageAgent",
    "MonitoringAgent",
    "LearningAgent",
]


def __getattr__(name: str):  # noqa: ANN001
    """Lazy-load agent classes on first access."""
    _agent_map = {
        "SystemAgent": "aios_agent.agents.system",
        "TaskAgent": "aios_agent.agents.task",
        "NetworkAgent": "aios_agent.agents.network",
        "SecurityAgent": "aios_agent.agents.security",
        "PackageAgent": "aios_agent.agents.package",
        "StorageAgent": "aios_agent.agents.storage",
        "MonitoringAgent": "aios_agent.agents.monitoring",
        "LearningAgent": "aios_agent.agents.learning",
    }
    if name in _agent_map:
        import importlib

        module = importlib.import_module(_agent_map[name])
        cls = getattr(module, name)
        globals()[name] = cls
        return cls
    raise AttributeError(f"module 'aios_agent' has no attribute {name!r}")
