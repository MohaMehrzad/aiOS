"""
Tests for WebAgent — browsing, searching, API interaction, monitoring, notifications.
"""

from __future__ import annotations

from typing import Any
from unittest.mock import AsyncMock, patch

import pytest

from aios_agent.agents.web import WebAgent
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
def agent(config: AgentConfig) -> WebAgent:
    return WebAgent(agent_id="test-web-001", config=config)


# ---------------------------------------------------------------------------
# Agent identity
# ---------------------------------------------------------------------------


def test_agent_type(agent: WebAgent) -> None:
    assert agent.get_agent_type() == "web"


def test_capabilities(agent: WebAgent) -> None:
    caps = agent.get_capabilities()
    assert "web.browse" in caps
    assert "web.search" in caps
    assert "web.api_interact" in caps
    assert "web.monitor_url" in caps
    assert "web.notify" in caps
    assert "net_read" in caps
    assert "net_write" in caps


# ---------------------------------------------------------------------------
# Task dispatch
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_dispatch_browse(agent: WebAgent) -> None:
    """Tasks with 'browse' route to _browse."""
    with patch.object(agent, "_browse", new_callable=AsyncMock) as mock:
        mock.return_value = {"success": True}
        result = await agent.handle_task({
            "description": "browse https://example.com",
            "input_json": {"url": "https://example.com"},
        })
        assert result["success"]
        mock.assert_awaited_once()


@pytest.mark.asyncio
async def test_dispatch_search(agent: WebAgent) -> None:
    """Tasks with 'search' route to _search."""
    with patch.object(agent, "_search", new_callable=AsyncMock) as mock:
        mock.return_value = {"success": True}
        result = await agent.handle_task({
            "description": "search for python async patterns",
            "input_json": {"query": "python async"},
        })
        assert result["success"]
        mock.assert_awaited_once()


@pytest.mark.asyncio
async def test_dispatch_api(agent: WebAgent) -> None:
    """Tasks with 'api call' route to _api_interact."""
    with patch.object(agent, "_api_interact", new_callable=AsyncMock) as mock:
        mock.return_value = {"success": True}
        result = await agent.handle_task({
            "description": "call the weather api",
            "input_json": {"url": "https://api.example.com"},
        })
        assert result["success"]
        mock.assert_awaited_once()


@pytest.mark.asyncio
async def test_dispatch_monitor(agent: WebAgent) -> None:
    """Tasks with 'monitor' route to _monitor_url."""
    with patch.object(agent, "_monitor_url", new_callable=AsyncMock) as mock:
        mock.return_value = {"success": True}
        result = await agent.handle_task({
            "description": "monitor https://status.example.com",
            "input_json": {"url": "https://status.example.com"},
        })
        assert result["success"]
        mock.assert_awaited_once()


@pytest.mark.asyncio
async def test_dispatch_notify(agent: WebAgent) -> None:
    """Tasks with 'webhook' route to _notify."""
    with patch.object(agent, "_notify", new_callable=AsyncMock) as mock:
        mock.return_value = {"success": True}
        result = await agent.handle_task({
            "description": "send webhook notification",
            "input_json": {"url": "https://hooks.example.com"},
        })
        assert result["success"]
        mock.assert_awaited_once()


@pytest.mark.asyncio
async def test_dispatch_url_fallback(agent: WebAgent) -> None:
    """Tasks with a URL in input but no keyword should fallback to browse."""
    with patch.object(agent, "_browse", new_callable=AsyncMock) as mock:
        mock.return_value = {"success": True}
        result = await agent.handle_task({
            "description": "check this page",
            "input_json": {"url": "https://example.com"},
        })
        assert result["success"]
        mock.assert_awaited_once()


# ---------------------------------------------------------------------------
# Browse
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_browse_requires_url(agent: WebAgent) -> None:
    """Browse should fail without a URL."""
    result = await agent._browse({}, {"description": "browse something"})
    assert not result["success"]
    assert "URL" in result["error"]


@pytest.mark.asyncio
async def test_browse_fetches_and_summarizes(agent: WebAgent) -> None:
    """Browse should call scrape tool and think for summary."""
    with patch.object(agent, "call_tool", new_callable=AsyncMock) as mock_tool, \
         patch.object(agent, "think", new_callable=AsyncMock) as mock_think:
        mock_tool.return_value = {
            "success": True,
            "output": {
                "title": "Example",
                "text": "A" * 200,  # Long enough to trigger summary
                "content_length": 200,
                "truncated": False,
            },
        }
        mock_think.return_value = "This is a summary."

        result = await agent._browse(
            {"url": "https://example.com"},
            {"id": "task-1", "description": "browse"},
        )
        assert result["success"]
        assert result["title"] == "Example"
        assert result["summary"] == "This is a summary."
        mock_tool.assert_awaited_once()
        mock_think.assert_awaited_once()


# ---------------------------------------------------------------------------
# Search
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_search_calls_api(agent: WebAgent) -> None:
    """Search should call web.api_call with DuckDuckGo."""
    with patch.object(agent, "call_tool", new_callable=AsyncMock) as mock_tool:
        mock_tool.return_value = {
            "success": True,
            "output": {
                "data": {
                    "Abstract": "Python is a programming language.",
                    "AbstractSource": "Wikipedia",
                    "AbstractURL": "https://en.wikipedia.org/wiki/Python",
                    "Answer": "",
                    "RelatedTopics": [
                        {"Text": "Python tutorial", "FirstURL": "https://example.com"},
                    ],
                },
            },
        }

        result = await agent._search(
            {"query": "python programming"},
            {"description": "search"},
        )
        assert result["success"]
        assert result["abstract"] == "Python is a programming language."
        assert len(result["related_topics"]) == 1


# ---------------------------------------------------------------------------
# Notify
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_notify_requires_url(agent: WebAgent) -> None:
    """Notify should fail without webhook URL."""
    result = await agent._notify({}, {"description": "notify"})
    assert not result["success"]
    assert "URL" in result["error"]


@pytest.mark.asyncio
async def test_notify_sends_webhook(agent: WebAgent) -> None:
    """Notify should call web.webhook tool."""
    with patch.object(agent, "call_tool", new_callable=AsyncMock) as mock_tool, \
         patch.object(agent, "push_event", new_callable=AsyncMock):
        mock_tool.return_value = {"success": True}

        result = await agent._notify(
            {
                "url": "https://hooks.example.com/webhook",
                "payload": {"message": "test"},
            },
            {"description": "notify"},
        )
        assert result["success"]
        mock_tool.assert_awaited_once()
        call_args = mock_tool.call_args
        assert call_args[0][0] == "web.webhook"


# ---------------------------------------------------------------------------
# Monitor URL
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_monitor_url_first_check(agent: WebAgent) -> None:
    """First monitor check should store snapshot and not report change."""
    with patch.object(agent, "call_tool", new_callable=AsyncMock) as mock_tool, \
         patch.object(agent, "recall_memory", new_callable=AsyncMock) as mock_recall, \
         patch.object(agent, "store_memory", new_callable=AsyncMock):
        mock_tool.return_value = {
            "success": True,
            "output": {"body": "Hello World", "status": 200},
        }
        mock_recall.return_value = None  # First check

        result = await agent._monitor_url(
            {"url": "https://example.com"},
            {"description": "monitor"},
        )
        assert result["success"]
        assert not result["changed"]
        assert "First check" in result["changes"]
