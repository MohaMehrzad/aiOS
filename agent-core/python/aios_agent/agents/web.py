"""
WebAgent — Web interaction and monitoring agent.

Capabilities:
  - Browse and summarize web pages
  - Search for information via web APIs
  - Interact with external REST APIs
  - Monitor URLs for changes
  - Send notifications via webhooks
"""

from __future__ import annotations

import json
import logging
import time
from typing import Any

from aios_agent.base import BaseAgent, IntelligenceLevel

logger = logging.getLogger("aios.agent.web")


class WebAgent(BaseAgent):
    """Agent that interacts with the web: browsing, APIs, monitoring, notifications."""

    def get_agent_type(self) -> str:
        return "web"

    def get_capabilities(self) -> list[str]:
        return [
            "web.browse",
            "web.search",
            "web.api_interact",
            "web.monitor_url",
            "web.notify",
            "net_read",
            "net_write",
        ]

    # ------------------------------------------------------------------
    # Task dispatcher
    # ------------------------------------------------------------------

    async def handle_task(self, task: dict[str, Any]) -> dict[str, Any]:
        description = task.get("description", "").lower()
        input_data = task.get("input_json", {}) if isinstance(task.get("input_json"), dict) else {}

        if "browse" in description or "fetch" in description or "scrape" in description:
            return await self._browse(input_data, task)
        if "search" in description:
            return await self._search(input_data, task)
        if "api" in description or "call" in description:
            return await self._api_interact(input_data, task)
        if "monitor" in description or "watch" in description:
            return await self._monitor_url(input_data, task)
        if "notify" in description or "webhook" in description:
            return await self._notify(input_data, task)

        # Default: treat as a browse request if URL is provided
        if input_data.get("url"):
            return await self._browse(input_data, task)

        return {"success": False, "error": f"Could not determine web action from: {description}"}

    # ------------------------------------------------------------------
    # Browse and summarize
    # ------------------------------------------------------------------

    async def _browse(
        self,
        params: dict[str, Any],
        task: dict[str, Any],
    ) -> dict[str, Any]:
        """Fetch a web page and summarize its content using AI."""
        url = params.get("url", "")
        if not url:
            # Try to extract URL from task description
            words = task.get("description", "").split()
            url = next((w for w in words if w.startswith("http")), "")

        if not url:
            return {"success": False, "error": "No URL provided for browsing"}

        selector = params.get("selector", "")

        # Fetch the page
        scrape_result = await self.call_tool(
            "web.scrape",
            {"url": url, "selector": selector, "max_length": 50000},
            reason=f"Browsing: {url}",
        )

        if not scrape_result.get("success"):
            return scrape_result

        page_data = scrape_result.get("output", {})
        title = page_data.get("title", "")
        text = page_data.get("text", "")

        # Use AI to summarize the content
        summary = ""
        if text and len(text) > 100:
            summary_prompt = (
                f"Summarize the following web page content concisely:\n\n"
                f"Title: {title}\n"
                f"URL: {url}\n"
                f"Content (first 5000 chars):\n{text[:5000]}\n\n"
                f"Provide a 2-3 sentence summary of the key information."
            )
            try:
                summary = await self.think(
                    summary_prompt,
                    level=IntelligenceLevel.OPERATIONAL,
                    task_id=task.get("id"),
                )
            except Exception as exc:
                logger.warning("Failed to summarize page: %s", exc)
                summary = text[:500]

        return {
            "success": True,
            "url": url,
            "title": title,
            "summary": summary,
            "content_length": page_data.get("content_length", 0),
            "truncated": page_data.get("truncated", False),
        }

    # ------------------------------------------------------------------
    # Search for information
    # ------------------------------------------------------------------

    async def _search(
        self,
        params: dict[str, Any],
        task: dict[str, Any],
    ) -> dict[str, Any]:
        """Search for information using web APIs."""
        query = params.get("query", task.get("description", ""))
        search_engine = params.get("engine", "duckduckgo")

        # Use DuckDuckGo instant answer API (no API key needed)
        search_url = "https://api.duckduckgo.com/"

        result = await self.call_tool(
            "web.api_call",
            {
                "url": search_url,
                "method": "GET",
                "query_params": {
                    "q": query,
                    "format": "json",
                    "no_html": "1",
                    "skip_disambig": "1",
                },
            },
            reason=f"Searching for: {query}",
        )

        if not result.get("success"):
            return result

        api_data = result.get("output", {}).get("data", {})

        # Parse search results
        search_results = {
            "success": True,
            "query": query,
            "engine": search_engine,
            "abstract": api_data.get("Abstract", ""),
            "abstract_source": api_data.get("AbstractSource", ""),
            "abstract_url": api_data.get("AbstractURL", ""),
            "answer": api_data.get("Answer", ""),
            "related_topics": [],
        }

        for topic in api_data.get("RelatedTopics", [])[:5]:
            if isinstance(topic, dict) and "Text" in topic:
                search_results["related_topics"].append({
                    "text": topic.get("Text", ""),
                    "url": topic.get("FirstURL", ""),
                })

        return search_results

    # ------------------------------------------------------------------
    # API interaction
    # ------------------------------------------------------------------

    async def _api_interact(
        self,
        params: dict[str, Any],
        task: dict[str, Any],
    ) -> dict[str, Any]:
        """Call an external REST API."""
        url = params.get("url", "")
        if not url:
            return {"success": False, "error": "No URL provided for API call"}

        method = params.get("method", "GET")
        headers = params.get("headers", {})
        body = params.get("body", None)
        auth_bearer = params.get("auth_bearer", "")

        result = await self.call_tool(
            "web.api_call",
            {
                "url": url,
                "method": method,
                "headers": headers,
                "body": body or {},
                "auth_bearer": auth_bearer,
            },
            reason=f"API call: {method} {url}",
        )

        if not result.get("success"):
            return result

        api_output = result.get("output", {})

        # Use AI to interpret the response if requested
        if params.get("interpret", False):
            interpret_prompt = (
                f"Interpret this API response:\n"
                f"URL: {url}\n"
                f"Method: {method}\n"
                f"Status: {api_output.get('status', 'unknown')}\n"
                f"Response: {json.dumps(api_output.get('data', {}))[:3000]}\n\n"
                f"Provide a brief interpretation of what this response means."
            )
            try:
                interpretation = await self.think(
                    interpret_prompt,
                    level=IntelligenceLevel.OPERATIONAL,
                    task_id=task.get("id"),
                )
                api_output["interpretation"] = interpretation
            except Exception as exc:
                logger.warning("Failed to interpret API response: %s", exc)

        return api_output

    # ------------------------------------------------------------------
    # URL monitoring
    # ------------------------------------------------------------------

    async def _monitor_url(
        self,
        params: dict[str, Any],
        task: dict[str, Any],
    ) -> dict[str, Any]:
        """Monitor a URL for changes by comparing content snapshots."""
        url = params.get("url", "")
        if not url:
            return {"success": False, "error": "No URL provided for monitoring"}

        # Fetch current content
        current_result = await self.call_tool(
            "web.http_request",
            {"url": url, "method": "GET"},
            reason=f"Monitoring URL: {url}",
        )

        if not current_result.get("success"):
            return current_result

        current_body = current_result.get("output", {}).get("body", "")
        current_status = current_result.get("output", {}).get("status", 0)

        # Check against stored previous snapshot
        prev_snapshot = await self.recall_memory(f"url_monitor:{url}")

        changed = False
        changes_description = "First check — no previous snapshot."

        if prev_snapshot is not None:
            prev_body = prev_snapshot.get("body", "")
            prev_status = prev_snapshot.get("status", 0)

            if current_status != prev_status:
                changed = True
                changes_description = f"HTTP status changed: {prev_status} -> {current_status}"
            elif current_body != prev_body:
                changed = True
                # Calculate rough diff size
                diff_chars = abs(len(current_body) - len(prev_body))
                changes_description = (
                    f"Content changed ({diff_chars} chars difference, "
                    f"prev={len(prev_body)} bytes, current={len(current_body)} bytes)"
                )
            else:
                changes_description = "No changes detected."

        # Store current snapshot for next comparison
        await self.store_memory(f"url_monitor:{url}", {
            "body": current_body[:10000],  # Store first 10KB
            "status": current_status,
            "checked_at": int(time.time()),
        })

        result: dict[str, Any] = {
            "success": True,
            "url": url,
            "status": current_status,
            "changed": changed,
            "changes": changes_description,
            "content_length": len(current_body),
            "checked_at": int(time.time()),
        }

        # Send notification if changed and webhook configured
        webhook_url = params.get("notify_webhook", "")
        if changed and webhook_url:
            await self._notify(
                {
                    "url": webhook_url,
                    "payload": {
                        "event": "url_changed",
                        "monitored_url": url,
                        "changes": changes_description,
                        "timestamp": int(time.time()),
                    },
                },
                task,
            )
            result["notification_sent"] = True

        return result

    # ------------------------------------------------------------------
    # Send notifications
    # ------------------------------------------------------------------

    async def _notify(
        self,
        params: dict[str, Any],
        task: dict[str, Any],
    ) -> dict[str, Any]:
        """Send a notification via webhook."""
        url = params.get("url", params.get("webhook_url", ""))
        if not url:
            return {"success": False, "error": "No webhook URL provided for notification"}

        payload = params.get("payload", {})
        if not payload:
            payload = {
                "source": "aiOS",
                "agent": self.agent_id,
                "message": task.get("description", "Notification from aiOS"),
                "timestamp": int(time.time()),
            }

        headers = params.get("headers", {})
        secret = params.get("secret", "")

        result = await self.call_tool(
            "web.webhook",
            {
                "url": url,
                "payload": payload,
                "headers": headers,
                "secret": secret,
            },
            reason=f"Sending webhook notification to {url}",
        )

        if result.get("success"):
            await self.push_event(
                "web.notification_sent",
                {"url": url, "payload_keys": list(payload.keys())},
            )

        return result


if __name__ == "__main__":
    import asyncio
    import os
    agent = WebAgent(agent_id=os.getenv("AIOS_AGENT_NAME", "web-agent"))
    asyncio.run(agent.run())
