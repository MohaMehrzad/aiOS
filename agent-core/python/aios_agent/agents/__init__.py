"""
aiOS Agent implementations.

Each agent extends BaseAgent and handles a specific domain of OS management.
"""

from aios_agent.agents.creator import CreatorAgent
from aios_agent.agents.learning import LearningAgent
from aios_agent.agents.monitoring import MonitoringAgent
from aios_agent.agents.network import NetworkAgent
from aios_agent.agents.package import PackageAgent
from aios_agent.agents.security import SecurityAgent
from aios_agent.agents.storage import StorageAgent
from aios_agent.agents.system import SystemAgent
from aios_agent.agents.task import TaskAgent
from aios_agent.agents.web import WebAgent

__all__ = [
    "SystemAgent",
    "TaskAgent",
    "NetworkAgent",
    "SecurityAgent",
    "PackageAgent",
    "StorageAgent",
    "MonitoringAgent",
    "LearningAgent",
    "CreatorAgent",
    "WebAgent",
]

AGENT_REGISTRY: dict[str, type] = {
    "system": SystemAgent,
    "task": TaskAgent,
    "network": NetworkAgent,
    "security": SecurityAgent,
    "package": PackageAgent,
    "storage": StorageAgent,
    "monitoring": MonitoringAgent,
    "learning": LearningAgent,
    "creator": CreatorAgent,
    "web": WebAgent,
}
