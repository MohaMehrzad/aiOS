"""
Tests for StorageAgent -- disk health, backup creation/restore, filesystem check,
mount management, and capacity reporting.
"""

from __future__ import annotations

import json
from typing import Any
from unittest.mock import AsyncMock, patch

import pytest

from aios_agent.agents.storage import DISK_CRIT_THRESHOLD, DISK_WARN_THRESHOLD, StorageAgent
from aios_agent.base import AgentConfig


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def config() -> AgentConfig:
    return AgentConfig(max_retries=1, retry_delay_s=0.01, grpc_timeout_s=2.0)


@pytest.fixture
def agent(config: AgentConfig) -> StorageAgent:
    return StorageAgent(agent_id="storage-test-001", config=config)


# ---------------------------------------------------------------------------
# Basics
# ---------------------------------------------------------------------------


class TestStorageAgentBasics:
    def test_agent_type(self, agent: StorageAgent):
        assert agent.get_agent_type() == "storage"

    def test_capabilities(self, agent: StorageAgent):
        caps = agent.get_capabilities()
        assert "storage.check_disk_health" in caps
        assert "storage.create_backup" in caps
        assert "storage.restore_backup" in caps
        assert "storage.filesystem_check" in caps


# ---------------------------------------------------------------------------
# Task dispatch
# ---------------------------------------------------------------------------


class TestStorageTaskDispatch:
    @pytest.mark.asyncio
    async def test_health_keyword(self, agent: StorageAgent):
        with patch.object(agent, "_check_disk_health", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "check disk health"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_backup_keyword(self, agent: StorageAgent):
        with patch.object(agent, "_create_backup", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "create backup of /home"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_restore_keyword(self, agent: StorageAgent):
        with patch.object(agent, "_restore_backup", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "restore from backup"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_fsck_keyword(self, agent: StorageAgent):
        with patch.object(agent, "_filesystem_check", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "run fsck on /dev/sda1"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_capacity_keyword(self, agent: StorageAgent):
        with patch.object(agent, "_capacity_report", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "show disk space usage"})
        m.assert_awaited_once()

    @pytest.mark.asyncio
    async def test_mount_keyword(self, agent: StorageAgent):
        with patch.object(agent, "_manage_mounts", new_callable=AsyncMock,
                          return_value={"success": True}) as m:
            await agent.handle_task({"description": "mount /dev/sdb1"})
        m.assert_awaited_once()


# ---------------------------------------------------------------------------
# Disk health
# ---------------------------------------------------------------------------


class TestDiskHealth:
    @pytest.mark.asyncio
    async def test_healthy_disks(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "storage.list_block_devices":
                return {"success": True, "output": {
                    "devices": [{"name": "sda", "type": "disk"}]
                }}
            if name == "storage.smart_data":
                return {"success": True, "output": {
                    "overall_health": "PASSED",
                    "temperature_celsius": 35,
                    "power_on_hours": 1000,
                    "reallocated_sectors": 0,
                    "pending_sectors": 0,
                }}
            if name == "storage.io_stats":
                return {"success": True, "output": {
                    "read_iops": 100,
                    "write_iops": 50,
                    "utilization_percent": 20.0,
                }}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._check_disk_health({})

        assert result["success"] is True
        assert result["all_healthy"] is True
        assert result["devices_checked"] == 1
        assert result["reports"][0]["health_status"] == "PASSED"
        assert result["reports"][0]["temp_warning"] is False

    @pytest.mark.asyncio
    async def test_unhealthy_disk(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "storage.smart_data":
                return {"success": True, "output": {
                    "overall_health": "FAILED",
                    "temperature_celsius": 65,
                    "power_on_hours": 50000,
                    "reallocated_sectors": 100,
                    "pending_sectors": 5,
                }}
            if name == "storage.io_stats":
                return {"success": True, "output": {"utilization_percent": 95.0}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock), \
             patch.object(agent, "store_memory", new_callable=AsyncMock), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="Disk failing, backup data immediately"), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._check_disk_health({"devices": ["sda"]})

        assert result["all_healthy"] is False
        assert "sda" in result["unhealthy_disks"]
        assert result["reports"][0]["temp_critical"] is True
        assert len(result["warnings"]) > 0

    @pytest.mark.asyncio
    async def test_no_devices_found(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "storage.list_block_devices":
                return {"success": True, "output": {"devices": []}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._check_disk_health({})

        assert result["success"] is False
        assert "No disk devices" in result["error"]


# ---------------------------------------------------------------------------
# Backup creation
# ---------------------------------------------------------------------------


class TestCreateBackup:
    @pytest.mark.asyncio
    async def test_successful_backup(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "storage.check_space":
                return {"success": True, "output": {"available_gb": 100.0}}
            if name == "storage.estimate_size":
                return {"success": True, "output": {"estimated_gb": 5.0}}
            if name == "storage.create_backup":
                return {"success": True, "output": {
                    "backup_id": "bkp-001",
                    "size_gb": 4.5,
                }, "execution_id": "ex-b1"}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "recall_memory", new_callable=AsyncMock, return_value=None), \
             patch.object(agent, "store_memory", new_callable=AsyncMock), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._create_backup({
                "paths": ["/etc", "/home"],
                "destination": "/backup",
            })

        assert result["success"] is True
        assert result["backup_id"] == "bkp-001"
        assert result["type"] == "incremental"

    @pytest.mark.asyncio
    async def test_insufficient_space(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "storage.check_space":
                return {"success": True, "output": {"available_gb": 0.5}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._create_backup({})

        assert result["success"] is False
        assert "Insufficient space" in result["error"]

    @pytest.mark.asyncio
    async def test_backup_failure_emits_event(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "storage.check_space":
                return {"success": True, "output": {"available_gb": 100.0}}
            if name == "storage.estimate_size":
                return {"success": True, "output": {"estimated_gb": 5.0}}
            if name == "storage.create_backup":
                return {"success": False, "error": "I/O error"}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "recall_memory", new_callable=AsyncMock, return_value=None), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._create_backup({})

        assert result["success"] is False
        agent.push_event.assert_awaited_once()  # critical event

    @pytest.mark.asyncio
    async def test_incremental_uses_reference(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "storage.check_space":
                return {"success": True, "output": {"available_gb": 100.0}}
            if name == "storage.estimate_size":
                return {"success": True, "output": {"estimated_gb": 2.0}}
            if name == "storage.create_backup":
                assert (input_json or {}).get("reference_backup") == "bkp-prev"
                return {"success": True, "output": {"backup_id": "bkp-002", "size_gb": 1.0},
                        "execution_id": "ex1"}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "recall_memory", new_callable=AsyncMock,
                          return_value={"backup_id": "bkp-prev"}), \
             patch.object(agent, "store_memory", new_callable=AsyncMock), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._create_backup({"type": "incremental"})

        assert result["success"] is True


# ---------------------------------------------------------------------------
# Filesystem check
# ---------------------------------------------------------------------------


class TestFilesystemCheck:
    @pytest.mark.asyncio
    async def test_clean_filesystem(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "storage.check_mounted":
                return {"success": True, "output": {"mounted": False}}
            if name == "storage.fsck":
                return {"success": True, "output": {"errors_found": 0, "errors_fixed": 0}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._filesystem_check({"device": "/dev/sda1"})

        assert result["success"] is True
        assert result["clean"] is True
        assert result["errors_found"] == 0

    @pytest.mark.asyncio
    async def test_no_device_specified(self, agent: StorageAgent):
        result = await agent._filesystem_check({})
        assert result["success"] is False
        assert "No device" in result["error"]

    @pytest.mark.asyncio
    async def test_mounted_device_rejected(self, agent: StorageAgent):
        async def _mounted(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {"mounted": True}}

        with patch.object(agent, "call_tool", side_effect=_mounted):
            result = await agent._filesystem_check({"device": "/dev/sda1"})

        assert result["success"] is False
        assert "mounted" in result["error"]

    @pytest.mark.asyncio
    async def test_errors_found_and_fixed(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "storage.check_mounted":
                return {"success": True, "output": {"mounted": False}}
            if name == "storage.fsck":
                return {"success": True, "output": {"errors_found": 5, "errors_fixed": 3}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "push_event", new_callable=AsyncMock):
            result = await agent._filesystem_check({"device": "/dev/sda1"})

        assert result["errors_found"] == 5
        assert result["errors_fixed"] == 3
        assert result["clean"] is False
        agent.push_event.assert_awaited_once()


# ---------------------------------------------------------------------------
# Capacity report
# ---------------------------------------------------------------------------


class TestCapacityReport:
    @pytest.mark.asyncio
    async def test_normal_usage(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "filesystems": [
                    {"filesystem": "/dev/sda1", "mount_point": "/",
                     "size_gb": 100, "used_gb": 50, "use_percent": 50.0},
                    {"filesystem": "/dev/sdb1", "mount_point": "/data",
                     "size_gb": 500, "used_gb": 200, "use_percent": 40.0},
                ]
            }}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock):
            result = await agent._capacity_report({})

        assert result["success"] is True
        assert result["filesystem_count"] == 2
        assert result["warnings"] == []

    @pytest.mark.asyncio
    async def test_critical_usage_generates_warnings(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "filesystems": [
                    {"filesystem": "/dev/sda1", "mount_point": "/",
                     "use_percent": 97.0},
                    {"filesystem": "/dev/sdb1", "mount_point": "/data",
                     "use_percent": 88.0},
                ]
            }}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "update_metric", new_callable=AsyncMock), \
             patch.object(agent, "think", new_callable=AsyncMock,
                          return_value="1. Clean /tmp\n2. Remove old logs"):
            result = await agent._capacity_report({})

        assert len(result["warnings"]) == 2
        critical_warns = [w for w in result["warnings"] if w["severity"] == "critical"]
        assert len(critical_warns) == 1
        assert result["recommendations"]


# ---------------------------------------------------------------------------
# Mount management
# ---------------------------------------------------------------------------


class TestManageMounts:
    @pytest.mark.asyncio
    async def test_list_mounts(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {
                "mounts": [{"device": "/dev/sda1", "mount_point": "/", "fs_type": "ext4"}]
            }}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._manage_mounts({"action": "list"})

        assert result["success"] is True
        assert len(result["mounts"]) == 1

    @pytest.mark.asyncio
    async def test_mount_device(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "store_memory", new_callable=AsyncMock):
            result = await agent._manage_mounts({
                "action": "mount",
                "device": "/dev/sdb1",
                "mount_point": "/mnt/data",
                "fs_type": "ext4",
            })

        assert result["success"] is True
        assert result["action"] == "mount"

    @pytest.mark.asyncio
    async def test_unmount(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._manage_mounts({
                "action": "unmount",
                "mount_point": "/mnt/data",
            })

        assert result["success"] is True
        assert result["action"] == "unmount"

    @pytest.mark.asyncio
    async def test_unknown_action(self, agent: StorageAgent):
        result = await agent._manage_mounts({"action": "format"})
        assert result["success"] is False


# ---------------------------------------------------------------------------
# Restore backup
# ---------------------------------------------------------------------------


class TestRestoreBackup:
    @pytest.mark.asyncio
    async def test_dry_run_restore(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "storage.verify_backup":
                return {"success": True, "output": {"integrity_ok": True}}
            if name == "storage.restore_backup":
                return {"success": True, "output": {"file_count": 150}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool), \
             patch.object(agent, "think", new_callable=AsyncMock, return_value="Safe to restore"):
            result = await agent._restore_backup({"backup_id": "bkp-001", "dry_run": True})

        assert result["success"] is True
        assert result["dry_run"] is True
        assert result["files_to_restore"] == 150

    @pytest.mark.asyncio
    async def test_no_backup_id(self, agent: StorageAgent):
        with patch.object(agent, "recall_memory", new_callable=AsyncMock, return_value=None):
            result = await agent._restore_backup({})

        assert result["success"] is False
        assert "No backup_id" in result["error"]

    @pytest.mark.asyncio
    async def test_integrity_failure(self, agent: StorageAgent):
        async def _fake_call_tool(name, input_json=None, *, reason="", task_id=None):
            if name == "storage.verify_backup":
                return {"success": True, "output": {"integrity_ok": False}}
            return {"success": True, "output": {}}

        with patch.object(agent, "call_tool", side_effect=_fake_call_tool):
            result = await agent._restore_backup({"backup_id": "bkp-corrupt"})

        assert result["success"] is False
        assert "integrity" in result["error"].lower()
