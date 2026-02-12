# Python Packaging & Project Structure

## Overview

The Python components of aiOS (agent runtime, agents, memory client, tool client) are packaged as a single installable Python package: `aios-agent`.

---

## Project Layout

```
agent-core/python/
├── pyproject.toml          # Package definition
├── aios_agent/
│   ├── __init__.py
│   ├── base.py             # BaseAgent class
│   ├── orchestrator_client.py  # gRPC client for orchestrator
│   ├── tool_client.py      # gRPC client for tool registry
│   ├── runtime_client.py   # gRPC client for AI runtime
│   ├── memory_client.py    # gRPC client for memory system
│   ├── intelligence.py     # Intelligence level routing
│   ├── config.py           # Agent configuration loading
│   ├── agents/
│   │   ├── __init__.py
│   │   ├── system.py
│   │   ├── network.py
│   │   ├── security.py
│   │   ├── monitor.py
│   │   ├── package.py
│   │   ├── storage.py
│   │   ├── task.py
│   │   └── dev.py
│   └── proto/              # Generated protobuf Python code
│       ├── __init__.py
│       ├── common_pb2.py
│       ├── common_pb2_grpc.py
│       ├── orchestrator_pb2.py
│       ├── orchestrator_pb2_grpc.py
│       ├── agent_pb2.py
│       ├── agent_pb2_grpc.py
│       ├── tools_pb2.py
│       ├── tools_pb2_grpc.py
│       ├── memory_pb2.py
│       ├── memory_pb2_grpc.py
│       ├── runtime_pb2.py
│       └── runtime_pb2_grpc.py
└── tests/
    ├── conftest.py
    ├── test_base_agent.py
    ├── test_intelligence.py
    ├── test_tool_client.py
    └── test_agents/
        ├── test_system.py
        ├── test_network.py
        └── test_monitor.py
```

---

## pyproject.toml

```toml
[project]
name = "aios-agent"
version = "0.1.0"
description = "aiOS Agent Runtime — AI agents for the aiOS operating system"
requires-python = ">=3.12"
dependencies = [
    "grpcio>=1.60",
    "grpcio-tools>=1.60",
    "protobuf>=4.0",
    "pydantic>=2.0",
    "aiosqlite>=0.20",
    "httpx>=0.27",
    "anthropic>=0.40",
    "openai>=1.50",
    "chromadb>=0.5",
    "tomli>=2.0",
]

[project.optional-dependencies]
dev = [
    "pytest>=8.0",
    "pytest-asyncio>=0.24",
    "ruff>=0.5",
    "mypy>=1.10",
]

[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[tool.hatch.build.targets.wheel]
packages = ["aios_agent"]

[tool.ruff]
target-version = "py312"
line-length = 100

[tool.ruff.lint]
select = ["E", "F", "I", "N", "UP", "B", "A", "SIM"]

[tool.pytest.ini_options]
asyncio_mode = "auto"
testpaths = ["tests"]

[tool.mypy]
python_version = "3.12"
strict = true

# Entry points for running agents directly
[project.scripts]
aios-agent-system = "aios_agent.agents.system:main"
aios-agent-network = "aios_agent.agents.network:main"
aios-agent-security = "aios_agent.agents.security:main"
aios-agent-monitor = "aios_agent.agents.monitor:main"
aios-agent-package = "aios_agent.agents.package:main"
aios-agent-storage = "aios_agent.agents.storage:main"
aios-agent-task = "aios_agent.agents.task:main"
aios-agent-dev = "aios_agent.agents.dev:main"
```

---

## Proto Generation

Generate Python code from proto files:

```bash
#!/bin/bash
# agent-core/python/generate_proto.sh
set -euo pipefail

PROTO_DIR="../proto"
OUT_DIR="aios_agent/proto"

mkdir -p "$OUT_DIR"

python -m grpc_tools.protoc \
    --proto_path="$PROTO_DIR" \
    --python_out="$OUT_DIR" \
    --grpc_python_out="$OUT_DIR" \
    "$PROTO_DIR"/common.proto \
    "$PROTO_DIR"/orchestrator.proto \
    "$PROTO_DIR"/agent.proto \
    "$PROTO_DIR"/tools.proto \
    "$PROTO_DIR"/memory.proto \
    "$PROTO_DIR"/runtime.proto

# Fix imports (grpc_tools generates absolute imports, we need relative)
sed -i 's/^import common_pb2/from . import common_pb2/' "$OUT_DIR"/*_pb2*.py

echo "Proto files generated in $OUT_DIR"
```

---

## Installing in Development

```bash
cd agent-core/python
pip install -e ".[dev]"
```

## Installing in rootfs

```bash
# During rootfs build
cd agent-core/python
pip install --prefix=/usr --root="$ROOTFS" .
```

---

## Agent Entry Point Pattern

Each agent has a `main()` function as entry point:

```python
# aios_agent/agents/system.py

import asyncio
from aios_agent.base import BaseAgent
from aios_agent.config import load_agent_config

class SystemAgent(BaseAgent):
    # ... implementation ...
    pass

def main():
    config = load_agent_config("/etc/aios/agents/system.toml")
    agent = SystemAgent("aios-agent-system", config)
    asyncio.run(agent.start())

if __name__ == "__main__":
    main()
```

This allows running agents as:
```bash
# Via entry point
aios-agent-system

# Or directly
python -m aios_agent.agents.system
```
