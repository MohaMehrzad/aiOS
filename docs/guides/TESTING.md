# Testing Strategy

## Overview

aiOS testing has 4 levels: unit tests, integration tests, VM tests, and end-to-end tests. Every phase has specific test requirements that must pass before moving to the next phase.

---

## Test Pyramid

```
         ╱╲
        ╱  ╲
       ╱ E2E╲          Few, slow, comprehensive
      ╱──────╲         (Full system in QEMU)
     ╱ VM     ╲
    ╱ Tests    ╲        Boot and service tests
   ╱────────────╲      (QEMU with real rootfs)
  ╱ Integration  ╲
 ╱ Tests          ╲     Component interaction tests
╱──────────────────╲   (Docker/local, mock hardware)
╱    Unit Tests      ╲
╱════════════════════╲  Many, fast, focused
                        (cargo test, pytest)
```

---

## Level 1: Unit Tests

### Rust (`cargo test`)

Every Rust module gets unit tests in the same file:

```rust
// tools/src/fs/read.rs

pub async fn fs_read(input: Value) -> Result<Value> {
    // ... implementation ...
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_fs_read_existing_file() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "hello world").unwrap();

        let input = json!({"path": file.path().to_str().unwrap()});
        let result = fs_read(input).await.unwrap();

        assert_eq!(result["content"], "hello world");
        assert_eq!(result["size"], 11);
    }

    #[tokio::test]
    async fn test_fs_read_nonexistent_file() {
        let input = json!({"path": "/tmp/does_not_exist_12345"});
        let result = fs_read(input).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_fs_read_permission_denied() {
        // Create a file with no read permissions
        // Verify fs_read returns appropriate error
    }
}
```

**Run**: `cargo test --workspace`

**Coverage target**: >80% for core modules (tools, memory, security)

### Python (`pytest`)

```python
# agent-core/python/tests/test_system_agent.py

import pytest
from aios_agent.agents.system import SystemAgent
from unittest.mock import AsyncMock

@pytest.mark.asyncio
async def test_system_agent_file_read():
    agent = SystemAgent("test-agent", config=mock_config())
    agent.tool_client = AsyncMock()
    agent.tool_client.execute.return_value = ToolResult(
        success=True, data={"content": "hello"}
    )

    result = await agent.execute_task(Task(
        description="Read /etc/hostname",
        action="fs.read"
    ))

    assert result.success
    agent.tool_client.execute.assert_called_once()

@pytest.mark.asyncio
async def test_intelligence_routing():
    from aios_agent.intelligence import classify_task

    # Simple check → operational
    task = Task(description="Check if /etc/nginx exists")
    assert classify_task(task) == "operational"

    # Planning → strategic
    task = Task(description="Plan a migration from MySQL to PostgreSQL")
    assert classify_task(task) == "strategic"

    # Config generation → tactical
    task = Task(description="Generate nginx config for reverse proxy")
    assert classify_task(task) == "tactical"
```

**Run**: `pytest agent-core/python/tests/ -v`

---

## Level 2: Integration Tests

Test component interactions without needing a full VM.

### gRPC Integration Tests

Test that services can communicate:

```python
# tests/integration/test_tool_registry.py

import pytest
import grpc
from aios_proto import tools_pb2, tools_pb2_grpc

@pytest.fixture
async def tool_service():
    """Start tool registry service in background."""
    process = await start_service("aios-tools")
    yield process
    process.terminate()

@pytest.mark.asyncio
async def test_tool_execution(tool_service):
    channel = grpc.aio.insecure_channel("unix:///tmp/test-tools.sock")
    stub = tools_pb2_grpc.ToolRegistryStub(channel)

    # Execute fs.read
    response = await stub.Execute(tools_pb2.ExecuteRequest(
        tool_name="fs.read",
        agent_id="test-agent",
        task_id="test-task",
        input_json=b'{"path": "/etc/hostname"}',
        reason="integration test",
    ))

    assert response.success
    output = json.loads(response.output_json)
    assert "content" in output
```

### Memory Integration Tests

```python
# tests/integration/test_memory.py

@pytest.mark.asyncio
async def test_context_assembly():
    """Test that context assembly pulls from all memory tiers."""
    memory = await setup_test_memory()

    # Seed some data
    await memory.working.store_decision(Decision(
        context="nginx configuration",
        chosen="Use reverse proxy config",
        reasoning="Standard approach for this use case"
    ))

    # Assemble context for a related task
    context = await memory.assemble_context(
        "Configure nginx as a reverse proxy",
        max_tokens=2000,
    )

    assert len(context.chunks) > 0
    assert any("nginx" in c.content for c in context.chunks)
```

### Orchestrator Integration Tests

```python
# tests/integration/test_orchestrator.py

@pytest.mark.asyncio
async def test_goal_decomposition():
    """Test that a goal gets decomposed into tasks."""
    orchestrator = await setup_test_orchestrator()

    goal_id = await orchestrator.submit_goal(
        "Check disk usage on all mounted filesystems"
    )

    # Wait for planning
    await asyncio.sleep(2)

    status = await orchestrator.get_goal_status(goal_id)
    assert status.tasks_count > 0
    assert any("disk" in t.description.lower() for t in status.tasks)
```

**Run**: `./tests/integration/run.sh`

---

## Level 3: VM Tests (QEMU)

Test the actual booted system.

### Boot Test
```bash
#!/bin/bash
# tests/vm/test_boot.sh
# Boots aiOS in QEMU and verifies it reaches the autonomy loop

TIMEOUT=60  # seconds

# Start QEMU in background, capture serial output
./build/run-qemu.sh &
QEMU_PID=$!

# Wait for boot complete message
if timeout $TIMEOUT grep -q "System is autonomous" <(tail -f /tmp/qemu-serial.log); then
    echo "PASS: System booted to autonomy"
    RESULT=0
else
    echo "FAIL: Boot timeout after ${TIMEOUT}s"
    RESULT=1
fi

kill $QEMU_PID
exit $RESULT
```

### Service Health Test
```bash
#!/bin/bash
# tests/vm/test_services.sh
# Verifies all services are running after boot

# Wait for boot
sleep 30

# Check each service via management API (port forwarded through QEMU)
check_service() {
    local service=$1
    response=$(curl -s http://localhost:9090/api/status | jq -r ".agents.${service}.status")
    if [ "$response" = "healthy" ]; then
        echo "PASS: $service is healthy"
    else
        echo "FAIL: $service status: $response"
        return 1
    fi
}

check_service "system"
check_service "network"
check_service "security"
check_service "monitor"
check_service "package"
```

### Inference Test
```bash
# tests/vm/test_inference.sh
# Verifies local model inference works

response=$(curl -s -X POST http://localhost:9090/api/infer \
    -H "Content-Type: application/json" \
    -d '{"prompt": "What is 2+2? Answer with just the number.", "level": "operational"}')

answer=$(echo "$response" | jq -r '.text')
if echo "$answer" | grep -q "4"; then
    echo "PASS: Inference returned correct answer"
else
    echo "FAIL: Inference returned: $answer"
fi
```

**Run**: `./tests/vm/run-all.sh`

---

## Level 4: End-to-End Tests

Full scenario tests that exercise the complete system.

### E2E Test: System Health Check
```python
# tests/e2e/test_health_goal.py

async def test_health_check_goal():
    """Submit a health check goal and verify it completes."""
    # Submit goal
    goal = await api.submit_goal("Run a full system health check and report results")

    # Wait for completion (timeout 60s)
    result = await api.wait_for_goal(goal.id, timeout=60)

    assert result.status == "completed"
    assert "cpu" in result.report.lower()
    assert "memory" in result.report.lower()
    assert "disk" in result.report.lower()
```

### E2E Test: Package Installation
```python
# tests/e2e/test_install_package.py

async def test_install_nginx():
    """Submit a goal to install nginx and verify it works."""
    goal = await api.submit_goal("Install nginx web server")
    result = await api.wait_for_goal(goal.id, timeout=120)

    assert result.status == "completed"

    # Verify nginx is actually installed and running
    status = await api.submit_goal("Check if nginx is running on port 80")
    result = await api.wait_for_goal(status.id, timeout=30)
    assert "running" in result.report.lower() or "listening" in result.report.lower()
```

### E2E Test: File Management
```python
# tests/e2e/test_file_management.py

async def test_create_and_read_file():
    """Ask AI to create a file and then read it back."""
    # Create
    goal = await api.submit_goal(
        "Create a file at /tmp/test-aios.txt with the content 'Hello from aiOS'"
    )
    result = await api.wait_for_goal(goal.id, timeout=30)
    assert result.status == "completed"

    # Read back
    goal = await api.submit_goal("Read the file /tmp/test-aios.txt and tell me its contents")
    result = await api.wait_for_goal(goal.id, timeout=30)
    assert "Hello from aiOS" in result.report
```

**Run**: `pytest tests/e2e/ -v --timeout=300`

---

## Test Requirements Per Phase

| Phase | Unit | Integration | VM | E2E |
|---|---|---|---|---|
| 1 Setup | Cargo check passes | - | QEMU starts | - |
| 2 Kernel | - | - | Kernel boots <5s | - |
| 3 Base System | init module tests | - | Init runs, mounts fs | - |
| 4 AI Runtime | model manager tests | gRPC inference test | Model loads on boot | Prompt → response |
| 5 Agent Core | goal/task/routing tests | Orchestrator ↔ agent gRPC | All agents start | Goal → completion |
| 6 Tools | All tool unit tests | Tool execution pipeline | Tools work from agent | - |
| 7 Memory | Memory tier tests | Context assembly | Memory persists across reboot | - |
| 8 Networking | - | Network tool tests | Interface auto-configured | - |
| 9 Security | Capability tests | Permission denial tests | Sandboxing enforced | - |
| 10 Packages | - | Package install test | Install nginx in VM | Goal: "install nginx" |
| 11 API Gateway | Budget/routing tests | Claude API test | API calls from VM | Strategic planning |
| 12 Distro | - | Build pipeline test | ISO boots + installs | Full scenario battery |

---

## CI/CD (Future)

When the project matures, set up CI:

```yaml
# .github/workflows/test.yml
name: aiOS Tests
on: [push, pull_request]

jobs:
  unit-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --workspace
      - run: pytest agent-core/python/tests/ -v

  integration-tests:
    runs-on: ubuntu-latest
    needs: unit-tests
    steps:
      - run: ./tests/integration/run.sh

  vm-tests:
    runs-on: ubuntu-latest
    needs: integration-tests
    steps:
      - run: sudo apt install qemu-system-x86
      - run: ./build/build-all.sh
      - run: ./tests/vm/run-all.sh
```

---

## Test Data & Fixtures

Store test fixtures in `tests/fixtures/`:
```
tests/fixtures/
├── config/
│   └── test-config.toml      # Test configuration
├── models/
│   └── dummy-model.gguf      # Tiny dummy model for inference tests
├── rootfs/
│   └── minimal/               # Minimal rootfs for boot tests
└── secrets/
    └── test-secrets.enc       # Test encrypted secrets
```
