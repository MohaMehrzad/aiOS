"""
Tests for CreatorAgent — project scaffolding, code generation, and git init.
"""

from __future__ import annotations

from typing import Any
from unittest.mock import AsyncMock, patch

import pytest

from aios_agent.agents.creator import CreatorAgent
from aios_agent.base import AgentConfig


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def config() -> AgentConfig:
    return AgentConfig(
        max_retries=1,
        retry_delay_s=0.01,
        grpc_timeout_s=2.0,
    )


@pytest.fixture
def agent(config: AgentConfig) -> CreatorAgent:
    return CreatorAgent(agent_id="test-creator-001", config=config)


# ---------------------------------------------------------------------------
# Agent identity
# ---------------------------------------------------------------------------


def test_agent_type(agent: CreatorAgent) -> None:
    assert agent.get_agent_type() == "creator"


def test_capabilities(agent: CreatorAgent) -> None:
    caps = agent.get_capabilities()
    assert "creator.scaffold" in caps
    assert "creator.generate_code" in caps
    assert "creator.init_repo" in caps
    assert "creator.full_project" in caps
    assert "code_gen" in caps
    assert "git_write" in caps


# ---------------------------------------------------------------------------
# Task dispatch
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_dispatch_scaffold(agent: CreatorAgent) -> None:
    """Tasks with 'scaffold' route to _scaffold_project."""
    with patch.object(agent, "_scaffold_project", new_callable=AsyncMock) as mock:
        mock.return_value = {"success": True}
        result = await agent.handle_task({
            "description": "scaffold a new python project",
            "input_json": {"name": "my-app", "project_type": "python", "path": "/tmp"},
        })
        assert result["success"]
        mock.assert_awaited_once()


@pytest.mark.asyncio
async def test_dispatch_generate_code(agent: CreatorAgent) -> None:
    """Tasks with 'generate code' route to _generate_code."""
    with patch.object(agent, "_generate_code", new_callable=AsyncMock) as mock:
        mock.return_value = {"success": True}
        result = await agent.handle_task({
            "description": "generate code for a REST handler",
            "input_json": {"file_path": "/tmp/handler.py"},
        })
        assert result["success"]
        mock.assert_awaited_once()


@pytest.mark.asyncio
async def test_dispatch_init_repo(agent: CreatorAgent) -> None:
    """Tasks with 'init repo' route to _init_repo."""
    with patch.object(agent, "_init_repo", new_callable=AsyncMock) as mock:
        mock.return_value = {"success": True}
        result = await agent.handle_task({
            "description": "init git repo",
            "input_json": {"path": "/tmp/project"},
        })
        assert result["success"]
        mock.assert_awaited_once()


@pytest.mark.asyncio
async def test_dispatch_full_project(agent: CreatorAgent) -> None:
    """Tasks with 'create project' route to _full_project."""
    with patch.object(agent, "_full_project", new_callable=AsyncMock) as mock:
        mock.return_value = {"success": True}
        result = await agent.handle_task({
            "description": "create a new web project",
            "input_json": {},
        })
        assert result["success"]
        mock.assert_awaited_once()


# ---------------------------------------------------------------------------
# Scaffold project
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_scaffold_calls_tool(agent: CreatorAgent) -> None:
    """Scaffold should call code.scaffold tool."""
    with patch.object(agent, "call_tool", new_callable=AsyncMock) as mock_tool, \
         patch.object(agent, "push_event", new_callable=AsyncMock):
        mock_tool.return_value = {
            "success": True,
            "output": {"path": "/tmp/my-app", "files_created": ["f1", "f2"]},
        }
        result = await agent._scaffold_project(
            {"name": "my-app", "project_type": "rust", "path": "/tmp"},
            {"description": "scaffold"},
        )
        assert result["success"]
        mock_tool.assert_awaited_once()
        call_args = mock_tool.call_args
        assert call_args[0][0] == "code.scaffold"


# ---------------------------------------------------------------------------
# Generate code
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_generate_code_requires_file_path(agent: CreatorAgent) -> None:
    """Generate code should fail without file_path."""
    result = await agent._generate_code({}, {"description": "test"})
    assert not result["success"]
    assert "file_path" in result["error"]


@pytest.mark.asyncio
async def test_generate_code_calls_tools(agent: CreatorAgent) -> None:
    """Generate code should call think() and code.generate tool."""
    with patch.object(agent, "think", new_callable=AsyncMock) as mock_think, \
         patch.object(agent, "call_tool", new_callable=AsyncMock) as mock_tool, \
         patch.object(agent, "semantic_search", new_callable=AsyncMock) as mock_search, \
         patch.object(agent, "store_decision", new_callable=AsyncMock):
        mock_think.return_value = '{"outline": "test", "key_functions": [], "dependencies": []}'
        mock_search.return_value = []
        mock_tool.return_value = {
            "success": True,
            "output": {"file_path": "/tmp/test.py", "lines": 10},
        }
        result = await agent._generate_code(
            {"file_path": "/tmp/test.py", "description": "test module"},
            {"id": "task-1", "description": "generate code"},
        )
        assert result["success"]
        mock_think.assert_awaited_once()


# ---------------------------------------------------------------------------
# Init repo
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_init_repo_requires_path(agent: CreatorAgent) -> None:
    """Init repo should fail without path."""
    result = await agent._init_repo({}, {"description": "test"})
    assert not result["success"]
    assert "path" in result["error"]


@pytest.mark.asyncio
async def test_init_repo_calls_git_tools(agent: CreatorAgent) -> None:
    """Init repo should call git.init, git.add, git.commit."""
    call_order: list[str] = []

    async def mock_call_tool(name: str, *args: Any, **kwargs: Any) -> dict[str, Any]:
        call_order.append(name)
        return {
            "success": True,
            "output": {"commit_hash": "abc123"} if name == "git.commit" else {},
        }

    with patch.object(agent, "call_tool", side_effect=mock_call_tool):
        result = await agent._init_repo({"path": "/tmp/project"}, {"description": "test"})
        assert result["success"]
        assert call_order == ["git.init", "git.add", "git.commit"]
