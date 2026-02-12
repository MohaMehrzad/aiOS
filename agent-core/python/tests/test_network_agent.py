"""
Tests for NetworkAgent -- interface config, connectivity, DNS, DHCP, and firewall.

Covers task dispatch, interface discovery, connectivity checks, DNS management,
DHCP lease operations, diagnostics, and firewall rule handling.
"""

from __future__ import annotations

import json
from typing import Any
from unittest.mock import AsyncMock, patch

import pytest

from aios_agent.agents.network import DEFAULT_DNS_TEST_DOMAINS, DEFAULT_PING_TARGETS, NetworkAgent
from aios_agent.base import AgentConfig


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def config() -> AgentConfig:
    return AgentConfig(max_retries=1, retry_delay_s=0.01, grpc_timeout_s=2.0)


@pytest.fixture
def agent(config: AgentConfig) -> NetworkAgent:
    return NetworkAgent(agent_id="network-test-001", config=config)


# ---------------------------------------------------------------------------
# Basics
# ---------------------------------------------------------------------------


class TestNetworkAgentBasics:
    def test_agent_type(self, agent: NetworkAgent):
        assert agent.get_agent_type() == "network"

    def test_capabilities(self, agent: NetworkAgent):
        caps = agent.get_capabilities()
        assert "network.configure_interface" in caps
        assert "network.check_connectivity" in caps
        assert "network.manage_dns" in caps
        assert "network.firewall" in caps


# ---------------------------------------------------------------------------
# Task dispatch
# ---------------------------------------------------------------------------


class TestNetworkTaskDispatch:
    @pytest.mark.asyncio
    async def test_interface_keyword(self, agent: NetworkAgent):
        with patch.object(agent, "_configure_interface", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "configure interface eth0"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_connectivity_keyword(self, agent: NetworkAgent):
        with patch.object(agent, "_check_connectivity", new_callable=AsyncMock,
                          return_value={"healthy": True}) as m:
            await agent.handle_task({"description": "check connectivity"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_dns_keyword(self, agent: NetworkAgent):
        with patch.object(agent, "_manage_dns", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "manage dns settings"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_firewall_keyword(self, agent: NetworkAgent):
        with patch.object(agent, "_manage_firewall", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "configure firewall rules"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_dhcp_keyword(self, agent: NetworkAgent):
        with patch.object(agent, "_manage_dhcp", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "request dhcp lease"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_diagnose_keyword(self, agent: NetworkAgent):
        with patch.object(agent, "_diagnose_network", new_callable=AsyncMock,
                          return_value={"healthy": True}) as m:
            await agent.handle_task({"description": "diagnose the network"})
        m.assert_awaited_once()


# ---------------------------------------------------------------------------
# Interface discovery and configuration
# ---------------------------------------------------------------------------


class TestConfigureInterface:
    @pytest.mark.asyncio
    async def test_no_interface_lists_available(self, agent: NetworkAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "network.list_interfaces":
                return {"success": True, "output": {
                    "interfaces": [{"name": "eth0"}, {"name": "lo"}]
                }}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._configure_interface({})

        assert result["success"] is False
        assert "eth0" in result["available_interfaces"]

    @pytest.mark.asyncio
    async def test_up_down_action(self, agent: NetworkAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._configure_interface({"interface": "eth0", "action": "up"})

        assert result["success"] is True
        assert result["action"] == "up"

    @pytest.mark.asyncio
    async def test_no_ip_address_returns_error(self, agent: NetworkAgent):
        result = await agent._configure_interface({"interface": "eth0"})
        assert result["success"] is False
        assert "No IP address" in result["error"]

    @pytest.mark.asyncio
    async def test_successful_configuration(self, agent: NetworkAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {}, "execution_id": "ex1"}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value="YES, valid config"), \
             patch.object(agent, "store_memory", new_callable=AsyncMock), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._configure_interface({
                "interface": "eth0",
                "ip_address": "192.168.1.100",
                "netmask": "255.255.255.0",
                "gateway": "192.168.1.1",
            })

        assert result["success"] is True
        assert result["ip_address"] == "192.168.1.100"

    @pytest.mark.asyncio
    async def test_safety_check_rejects(self, agent: NetworkAgent):
        with patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="NO, invalid IP range"):
            result = await agent._configure_interface({
                "interface": "eth0",
                "ip_address": "999.999.999.999",
            })

        assert result["success"] is False
        assert "safety check" in result["error"]


# ---------------------------------------------------------------------------
# Connectivity checking
# ---------------------------------------------------------------------------


class TestConnectivityCheck:
    @pytest.mark.asyncio
    async def test_all_healthy(self, agent: NetworkAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "network.ping":
                return {"success": True, "output": {
                    "avg_rtt_ms": 10.0,
                    "packet_loss_pct": 0.0,
                }}
            if name == "network.dns_resolve":
                return {"success": True, "output": {
                    "addresses": ["1.2.3.4"],
                    "ttl": 300,
                }}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock):
            result = await agent._check_connectivity({})

        assert result["healthy"] is True
        assert len(result["results"]["ping"]) == len(DEFAULT_PING_TARGETS)
        assert len(result["results"]["dns"]) == len(DEFAULT_DNS_TEST_DOMAINS)

    @pytest.mark.asyncio
    async def test_ping_failure_marks_unhealthy(self, agent: NetworkAgent):
        async def _fail_ping(name, input_json=None, *, reason="", task_id=None):
            if name == "network.ping":
                return {"success": False, "error": "timeout"}
            return {"success": True, "output": {"addresses": ["1.2.3.4"], "ttl": 300}}

        with patch.object(agent, "call_tool", side_effect=_fail_ping), \
             patch.object(agent, "update_metric", new_callable=AsyncMock), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._check_connectivity({})

        assert result["healthy"] is False

    @pytest.mark.asyncio
    async def test_port_checks(self, agent: NetworkAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "network.ping":
                return {"success": True, "output": {"avg_rtt_ms": 5.0, "packet_loss_pct": 0.0}}
            if name == "network.dns_resolve":
                return {"success": True, "output": {"addresses": ["1.1.1.1"], "ttl": 60}}
            if name == "network.port_check":
                return {"success": True, "output": {"open": True}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock):
            result = await agent._check_connectivity({
                "port_checks": [{"host": "example.com", "port": 443}]
            })

        assert "example.com:443" in result["results"]["port"]
        assert result["results"]["port"]["example.com:443"]["open"] is True

    @pytest.mark.asyncio
    async def test_high_packet_loss_is_unhealthy(self, agent: NetworkAgent):
        async def _lossy(name, input_json=None, *, reason="", task_id=None):
            if name == "network.ping":
                return {"success": True, "output": {"avg_rtt_ms": 500.0, "packet_loss_pct": 80.0}}
            return {"success": True, "output": {"addresses": ["1.1.1.1"], "ttl": 60}}

        with patch.object(agent, "call_tool", side_effect=_lossy), \
             patch.object(agent, "update_metric", new_callable=AsyncMock), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._check_connectivity({})

        assert result["healthy"] is False


# ---------------------------------------------------------------------------
# DNS management
# ---------------------------------------------------------------------------


class TestDnsManagement:
    @pytest.mark.asyncio
    async def test_set_resolvers(self, agent: NetworkAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._manage_dns({
                "action": "set_resolvers",
                "resolvers": ["8.8.8.8", "1.1.1.1"],
            })

        assert result["success"] is True
        assert result["action"] == "set_resolvers"
        assert result["resolvers"] == ["8.8.8.8", "1.1.1.1"]

    @pytest.mark.asyncio
    async def test_flush_cache(self, agent: NetworkAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._manage_dns({"action": "flush_cache"})

        assert result["success"] is True
        assert result["action"] == "flush_cache"

    @pytest.mark.asyncio
    async def test_resolve_domain(self, agent: NetworkAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {"addresses": ["93.184.216.34"]}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._manage_dns({
                "action": "resolve",
                "domain": "example.com",
            })

        assert result["success"] is True
        assert "93.184.216.34" in result["addresses"]

    @pytest.mark.asyncio
    async def test_add_record(self, agent: NetworkAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._manage_dns({
                "action": "add_record",
                "domain": "test.local",
                "record_type": "A",
                "record_value": "10.0.0.5",
            })

        assert result["success"] is True
        assert result["domain"] == "test.local"

    @pytest.mark.asyncio
    async def test_default_status(self, agent: NetworkAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {"resolvers": ["8.8.8.8"]}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._manage_dns({})

        assert result["action"] == "status"


# ---------------------------------------------------------------------------
# Firewall management
# ---------------------------------------------------------------------------


class TestFirewallManagement:
    @pytest.mark.asyncio
    async def test_add_rule_safe(self, agent: NetworkAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value="YES, safe rule"):
            result = await agent._manage_firewall({
                "action": "add_rule",
                "rule": {"protocol": "tcp", "port": 80, "action": "allow"},
            })

        assert result["success"] is True
        assert result["action"] == "add_rule"

    @pytest.mark.asyncio
    async def test_add_rule_rejected_by_safety(self, agent: NetworkAgent):
        with patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="NO, could lock us out"):
            result = await agent._manage_firewall({
                "action": "add_rule",
                "rule": {"protocol": "tcp", "port": 22, "action": "deny"},
            })

        assert result["success"] is False
        assert "safety check" in result["error"]

    @pytest.mark.asyncio
    async def test_firewall_status(self, agent: NetworkAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "rules": [{"port": 22, "action": "allow"}]
            }}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._manage_firewall({})

        assert result["action"] == "status"
        assert len(result["rules"]) == 1

    @pytest.mark.asyncio
    async def test_remove_rule(self, agent: NetworkAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._manage_firewall({
                "action": "remove_rule",
                "rule": {"id": "rule-1"},
            })

        assert result["success"] is True
        assert result["action"] == "remove_rule"


# ---------------------------------------------------------------------------
# List interfaces
# ---------------------------------------------------------------------------


class TestListInterfaces:
    @pytest.mark.asyncio
    async def test_list_success(self, agent: NetworkAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "interfaces": [
                    {"name": "eth0", "status": "up", "ip": "10.0.0.2"},
                    {"name": "lo", "status": "up", "ip": "127.0.0.1"},
                ]
            }}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._list_interfaces()

        assert result["success"] is True
        assert result["interface_count"] == 2

    @pytest.mark.asyncio
    async def test_list_failure(self, agent: NetworkAgent):
        async def _fail(name, input_json=None, *, reason="", task_id=None):
            return {"success": False, "error": "no access"}

        with patch.object(agent, "call_tool", side_effect=_fail):
            result = await agent._list_interfaces()

        assert result["success"] is False
