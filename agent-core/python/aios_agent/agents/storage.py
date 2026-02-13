"""
StorageAgent — Filesystem management, disk health, and backup automation.

Capabilities:
  - Disk health monitoring (SMART data, I/O stats)
  - Backup creation and restoration
  - Mount point management
  - Filesystem checks and repair
  - Storage capacity planning
"""

from __future__ import annotations

import asyncio
import logging
import time
from typing import Any

from aios_agent.base import BaseAgent, IntelligenceLevel

logger = logging.getLogger("aios.agent.storage")

DISK_CHECK_INTERVAL_S = 300.0  # 5 minutes
DISK_WARN_THRESHOLD = 85.0
DISK_CRIT_THRESHOLD = 95.0


class StorageAgent(BaseAgent):
    """Agent responsible for storage and filesystem management."""

    def get_agent_type(self) -> str:
        return "storage"

    def get_capabilities(self) -> list[str]:
        return [
            "monitor.disk",
            "fs.list",
            "fs.stat",
            "fs.disk_usage",
            "fs.read",
            "fs.write",
        ]

    # ------------------------------------------------------------------
    # Task dispatcher
    # ------------------------------------------------------------------

    async def handle_task(self, task: dict[str, Any]) -> dict[str, Any]:
        description = task.get("description", "").lower()
        input_data = task.get("input_json", {}) if isinstance(task.get("input_json"), dict) else {}

        if "health" in description or "smart" in description or "disk" in description and "check" in description:
            return await self._check_disk_health(input_data)
        if "backup" in description and ("create" in description or "make" in description or "run" in description):
            return await self._create_backup(input_data)
        if "restore" in description:
            return await self._restore_backup(input_data)
        if "mount" in description or "unmount" in description or "umount" in description:
            return await self._manage_mounts(input_data)
        if "fsck" in description or "filesystem" in description and "check" in description:
            return await self._filesystem_check(input_data)
        if "capacity" in description or "space" in description or "usage" in description:
            return await self._capacity_report(input_data)

        decision = await self.think(
            f"Storage task: '{task.get('description', '')}'. "
            f"Options: check_disk_health, create_backup, restore_backup, manage_mounts, "
            f"filesystem_check, capacity_report. Which action? Reply with ONLY the action name.",
            level=IntelligenceLevel.OPERATIONAL,
        )
        action = decision.strip().lower()
        if "health" in action or "smart" in action or "disk" in action:
            return await self._check_disk_health(input_data)
        if "backup" in action and "restore" not in action:
            return await self._create_backup(input_data)
        if "restore" in action:
            return await self._restore_backup(input_data)
        if "mount" in action:
            return await self._manage_mounts(input_data)
        if "fsck" in action or "filesystem" in action:
            return await self._filesystem_check(input_data)
        return await self._capacity_report(input_data)

    # ------------------------------------------------------------------
    # Disk health
    # ------------------------------------------------------------------

    async def _check_disk_health(self, params: dict[str, Any]) -> dict[str, Any]:
        """Check disk health using SMART data and I/O statistics."""
        devices = params.get("devices", [])

        # List block devices if none specified
        if not devices:
            list_result = await self.call_tool(
                "fs.list", {},
                reason="Listing block devices for health check",
            )
            if list_result.get("success"):
                devices = [
                    d.get("name", "") for d in list_result.get("output", {}).get("devices", [])
                    if d.get("type") == "disk" and d.get("name")
                ]

        if not devices:
            return {"success": False, "error": "No disk devices found"}

        disk_reports: list[dict[str, Any]] = []
        unhealthy_disks: list[str] = []

        for device in devices:
            # Get SMART data
            smart_result = await self.call_tool(
                "fs.stat",
                {"device": device},
                reason=f"Reading SMART data for {device}",
            )

            smart_data: dict[str, Any] = {}
            health_status = "unknown"
            if smart_result.get("success"):
                smart_data = smart_result.get("output", {})
                health_status = smart_data.get("overall_health", "unknown")
                if health_status.lower() not in ("passed", "ok", "good"):
                    unhealthy_disks.append(device)

            # Get I/O stats
            io_result = await self.call_tool(
                "monitor.disk",
                {"device": device},
                reason=f"Reading I/O stats for {device}",
            )
            io_stats: dict[str, Any] = {}
            if io_result.get("success"):
                io_stats = io_result.get("output", {})

            # Temperature check
            temperature = smart_data.get("temperature_celsius", 0)
            temp_warning = temperature > 50
            temp_critical = temperature > 60

            report = {
                "device": device,
                "health_status": health_status,
                "temperature_c": temperature,
                "temp_warning": temp_warning,
                "temp_critical": temp_critical,
                "power_on_hours": smart_data.get("power_on_hours", 0),
                "reallocated_sectors": smart_data.get("reallocated_sectors", 0),
                "pending_sectors": smart_data.get("pending_sectors", 0),
                "read_iops": io_stats.get("read_iops", 0),
                "write_iops": io_stats.get("write_iops", 0),
                "utilization_percent": io_stats.get("utilization_percent", 0.0),
            }
            disk_reports.append(report)

            # Store metrics
            await self.update_metric(f"storage.{device}.temperature", float(temperature))
            await self.update_metric(
                f"storage.{device}.utilization",
                io_stats.get("utilization_percent", 0.0),
            )

        # AI analysis for unhealthy disks
        warnings: list[str] = []
        if unhealthy_disks:
            analysis = await self.think(
                f"Disk health check found unhealthy disks: {unhealthy_disks}.\n"
                f"SMART reports:\n"
                + "\n".join(
                    f"- {r['device']}: status={r['health_status']}, temp={r['temperature_c']}C, "
                    f"reallocated={r['reallocated_sectors']}, pending={r['pending_sectors']}"
                    for r in disk_reports if r["device"] in unhealthy_disks
                )
                + "\n\nWhat actions should be taken? Is data at risk?",
                level=IntelligenceLevel.TACTICAL,
            )
            warnings = [
                line.strip() for line in analysis.strip().split("\n") if line.strip()
            ]

            await self.push_event(
                "storage.disk_unhealthy",
                {"devices": unhealthy_disks, "reports": disk_reports},
                critical=True,
            )

        await self.store_memory("disk_health", {
            "timestamp": int(time.time()),
            "devices_checked": len(devices),
            "unhealthy": unhealthy_disks,
        })

        return {
            "success": True,
            "devices_checked": len(devices),
            "all_healthy": len(unhealthy_disks) == 0,
            "unhealthy_disks": unhealthy_disks,
            "reports": disk_reports,
            "warnings": warnings,
        }

    # ------------------------------------------------------------------
    # Backup creation
    # ------------------------------------------------------------------

    async def _create_backup(self, params: dict[str, Any]) -> dict[str, Any]:
        """Create a backup of specified paths or the entire system."""
        source_paths = params.get("paths", ["/etc", "/home", "/var/lib"])
        destination = params.get("destination", "/backup")
        backup_type = params.get("type", "incremental")  # full | incremental | differential
        compression = params.get("compression", "zstd")
        exclude_patterns = params.get("exclude", ["*.tmp", "*.cache", "__pycache__", ".git"])

        # Check destination space
        space_result = await self.call_tool(
            "fs.disk_usage",
            {"path": destination},
            reason=f"Checking backup destination space: {destination}",
        )

        available_gb = 0.0
        if space_result.get("success"):
            available_gb = space_result.get("output", {}).get("available_gb", 0.0)

        if available_gb < 1.0:
            return {
                "success": False,
                "error": f"Insufficient space at {destination}: {available_gb:.1f}GB available",
                "destination": destination,
            }

        # Estimate backup size
        estimate_result = await self.call_tool(
            "fs.disk_usage",
            {"paths": source_paths, "exclude": exclude_patterns},
            reason="Estimating backup size",
        )
        estimated_gb = 0.0
        if estimate_result.get("success"):
            estimated_gb = estimate_result.get("output", {}).get("estimated_gb", 0.0)

        if estimated_gb > available_gb * 0.9:
            # Use AI to decide on compression level or path trimming
            decision = await self.think(
                f"Backup estimated at {estimated_gb:.1f}GB but only {available_gb:.1f}GB available. "
                f"Source paths: {source_paths}. Type: {backup_type}. "
                f"Should I proceed with higher compression, skip some paths, or abort?",
                level=IntelligenceLevel.OPERATIONAL,
            )
            if "abort" in decision.lower():
                return {
                    "success": False,
                    "error": f"Aborted: estimated {estimated_gb:.1f}GB exceeds available {available_gb:.1f}GB",
                    "ai_decision": decision.strip(),
                }

        # Check for previous backup (for incremental)
        last_backup = await self.recall_memory("last_backup")
        reference_backup = ""
        if backup_type == "incremental" and isinstance(last_backup, dict):
            reference_backup = last_backup.get("backup_id", "")

        # Execute backup
        backup_result = await self.call_tool(
            "fs.copy",
            {
                "source_paths": source_paths,
                "destination": destination,
                "type": backup_type,
                "compression": compression,
                "exclude": exclude_patterns,
                "reference_backup": reference_backup,
            },
            reason=f"Creating {backup_type} backup of {source_paths} -> {destination}",
        )

        if not backup_result.get("success"):
            await self.push_event(
                "storage.backup_failed",
                {"error": backup_result.get("error", ""), "destination": destination},
                critical=True,
            )
            return {
                "success": False,
                "error": backup_result.get("error", "Backup failed"),
                "destination": destination,
            }

        output = backup_result.get("output", {})
        backup_id = output.get("backup_id", "")
        size_gb = output.get("size_gb", 0.0)

        await self.store_memory("last_backup", {
            "backup_id": backup_id,
            "timestamp": int(time.time()),
            "type": backup_type,
            "source_paths": source_paths,
            "destination": destination,
            "size_gb": size_gb,
        })

        await self.push_event(
            "storage.backup_created",
            {"backup_id": backup_id, "type": backup_type, "size_gb": size_gb},
        )

        return {
            "success": True,
            "backup_id": backup_id,
            "type": backup_type,
            "source_paths": source_paths,
            "destination": destination,
            "size_gb": size_gb,
            "compression": compression,
            "execution_id": backup_result.get("execution_id", ""),
        }

    # ------------------------------------------------------------------
    # Backup restoration
    # ------------------------------------------------------------------

    async def _restore_backup(self, params: dict[str, Any]) -> dict[str, Any]:
        """Restore from a backup."""
        backup_id = params.get("backup_id", "")
        target_path = params.get("target_path", "/")
        dry_run = params.get("dry_run", True)

        if not backup_id:
            # Use latest backup
            last_backup = await self.recall_memory("last_backup")
            if isinstance(last_backup, dict):
                backup_id = last_backup.get("backup_id", "")
            if not backup_id:
                return {"success": False, "error": "No backup_id specified and no recent backup found"}

        # Verify backup integrity first
        verify_result = await self.call_tool(
            "fs.stat",
            {"backup_id": backup_id},
            reason=f"Verifying backup {backup_id} before restore",
        )

        if not verify_result.get("success"):
            return {
                "success": False,
                "error": f"Backup verification failed: {verify_result.get('error', '')}",
                "backup_id": backup_id,
            }

        integrity_ok = verify_result.get("output", {}).get("integrity_ok", False)
        if not integrity_ok:
            return {
                "success": False,
                "error": "Backup integrity check failed — backup may be corrupted",
                "backup_id": backup_id,
            }

        # Safety check
        safety = await self.think(
            f"About to restore backup {backup_id} to {target_path}. "
            f"dry_run={dry_run}. Is this safe? What could go wrong?",
            level=IntelligenceLevel.TACTICAL,
        )

        if dry_run:
            restore_result = await self.call_tool(
                "fs.copy",
                {"backup_id": backup_id, "target_path": target_path, "dry_run": True},
                reason=f"Dry-run restore of backup {backup_id}",
            )
            return {
                "success": restore_result.get("success", False),
                "dry_run": True,
                "backup_id": backup_id,
                "files_to_restore": restore_result.get("output", {}).get("file_count", 0),
                "safety_analysis": safety.strip(),
            }

        # Actual restore
        restore_result = await self.call_tool(
            "fs.copy",
            {"backup_id": backup_id, "target_path": target_path, "dry_run": False},
            reason=f"Restoring backup {backup_id} to {target_path}",
        )

        await self.push_event(
            "storage.backup_restored",
            {
                "backup_id": backup_id,
                "target_path": target_path,
                "success": restore_result.get("success", False),
            },
        )

        return {
            "success": restore_result.get("success", False),
            "dry_run": False,
            "backup_id": backup_id,
            "target_path": target_path,
            "files_restored": restore_result.get("output", {}).get("file_count", 0),
            "error": restore_result.get("error", ""),
        }

    # ------------------------------------------------------------------
    # Mount management
    # ------------------------------------------------------------------

    async def _manage_mounts(self, params: dict[str, Any]) -> dict[str, Any]:
        """Manage filesystem mount points."""
        action = params.get("action", "list")  # list | mount | unmount | remount
        device = params.get("device", "")
        mount_point = params.get("mount_point", "")
        fs_type = params.get("fs_type", "ext4")
        options = params.get("options", "defaults")

        if action == "list":
            result = await self.call_tool(
                "fs.list", {},
                reason="Listing mount points",
            )
            mounts = result.get("output", {}).get("mounts", []) if result.get("success") else []
            return {"success": result.get("success", False), "mounts": mounts}

        if action == "mount" and device and mount_point:
            result = await self.call_tool(
                "process.spawn",
                {
                    "device": device,
                    "mount_point": mount_point,
                    "fs_type": fs_type,
                    "options": options,
                },
                reason=f"Mounting {device} at {mount_point} ({fs_type})",
            )
            if result.get("success"):
                await self.store_memory(f"mount:{mount_point}", {
                    "device": device,
                    "fs_type": fs_type,
                    "mounted_at": int(time.time()),
                })
            return {
                "success": result.get("success", False),
                "action": "mount",
                "device": device,
                "mount_point": mount_point,
                "error": result.get("error", ""),
            }

        if action == "unmount" and mount_point:
            result = await self.call_tool(
                "process.spawn",
                {"mount_point": mount_point, "force": params.get("force", False)},
                reason=f"Unmounting {mount_point}",
            )
            return {
                "success": result.get("success", False),
                "action": "unmount",
                "mount_point": mount_point,
                "error": result.get("error", ""),
            }

        if action == "remount" and mount_point:
            result = await self.call_tool(
                "process.spawn",
                {"mount_point": mount_point, "options": options},
                reason=f"Remounting {mount_point} with options: {options}",
            )
            return {
                "success": result.get("success", False),
                "action": "remount",
                "mount_point": mount_point,
                "error": result.get("error", ""),
            }

        return {"success": False, "error": f"Unknown mount action: {action}"}

    # ------------------------------------------------------------------
    # Filesystem check
    # ------------------------------------------------------------------

    async def _filesystem_check(self, params: dict[str, Any]) -> dict[str, Any]:
        """Run filesystem consistency check (fsck)."""
        device = params.get("device", "")
        auto_fix = params.get("auto_fix", False)

        if not device:
            return {"success": False, "error": "No device specified for filesystem check"}

        # Warn if mounted
        mount_result = await self.call_tool(
            "fs.stat",
            {"device": device},
            reason=f"Checking if {device} is mounted before fsck",
        )
        is_mounted = mount_result.get("output", {}).get("mounted", False) if mount_result.get("success") else False

        if is_mounted and not params.get("force", False):
            return {
                "success": False,
                "error": f"Device {device} is currently mounted. Unmount first or use force=True.",
                "device": device,
            }

        result = await self.call_tool(
            "process.spawn",
            {"device": device, "auto_fix": auto_fix},
            reason=f"Running fsck on {device} (auto_fix={auto_fix})",
        )

        output = result.get("output", {}) if result.get("success") else {}
        errors_found = output.get("errors_found", 0)
        errors_fixed = output.get("errors_fixed", 0)

        if errors_found > 0:
            await self.push_event(
                "storage.fsck_errors",
                {"device": device, "found": errors_found, "fixed": errors_fixed},
                critical=(errors_found > errors_fixed),
            )

        return {
            "success": result.get("success", False),
            "device": device,
            "errors_found": errors_found,
            "errors_fixed": errors_fixed,
            "clean": errors_found == 0,
            "error": result.get("error", ""),
        }

    # ------------------------------------------------------------------
    # Capacity report
    # ------------------------------------------------------------------

    async def _capacity_report(self, params: dict[str, Any]) -> dict[str, Any]:
        """Generate a storage capacity report."""
        result = await self.call_tool(
            "fs.disk_usage", {},
            reason="Generating storage capacity report",
        )

        if not result.get("success"):
            return {"success": False, "error": result.get("error", "Failed to generate report")}

        output = result.get("output", {})
        filesystems = output.get("filesystems", [])

        # Identify critical volumes
        warnings: list[dict[str, Any]] = []
        for fs in filesystems:
            usage_pct = fs.get("use_percent", 0.0)
            if usage_pct >= DISK_CRIT_THRESHOLD:
                warnings.append({
                    "filesystem": fs.get("filesystem", ""),
                    "mount_point": fs.get("mount_point", ""),
                    "usage_percent": usage_pct,
                    "severity": "critical",
                })
            elif usage_pct >= DISK_WARN_THRESHOLD:
                warnings.append({
                    "filesystem": fs.get("filesystem", ""),
                    "mount_point": fs.get("mount_point", ""),
                    "usage_percent": usage_pct,
                    "severity": "warning",
                })

        # Growth prediction
        recommendations: list[str] = []
        if warnings:
            analysis = await self.think(
                f"Storage capacity warnings:\n"
                + "\n".join(
                    f"- {w['mount_point']}: {w['usage_percent']}% ({w['severity']})"
                    for w in warnings
                )
                + "\n\nRecommend actions to free space or plan expansion.",
                level=IntelligenceLevel.OPERATIONAL,
            )
            recommendations = [
                line.strip().lstrip("- 0123456789.)")
                for line in analysis.strip().split("\n")
                if line.strip()
            ][:5]

        for fs in filesystems:
            mp = fs.get("mount_point", "root").replace("/", "_") or "root"
            await self.update_metric(f"storage.usage.{mp}", fs.get("use_percent", 0.0))

        return {
            "success": True,
            "filesystem_count": len(filesystems),
            "filesystems": filesystems,
            "warnings": warnings,
            "recommendations": recommendations,
        }

    # ------------------------------------------------------------------
    # Background disk monitoring loop
    # ------------------------------------------------------------------

    async def _disk_monitor_loop(self) -> None:
        while not self._shutdown_event.is_set():
            try:
                report = await self._capacity_report({})
                if report.get("warnings"):
                    critical = [w for w in report["warnings"] if w["severity"] == "critical"]
                    if critical:
                        logger.critical("CRITICAL disk usage: %s", critical)
            except Exception as exc:
                logger.error("Disk monitor error: %s", exc)
            try:
                await asyncio.wait_for(
                    self._shutdown_event.wait(),
                    timeout=DISK_CHECK_INTERVAL_S,
                )
            except asyncio.TimeoutError:
                pass

    async def run(self) -> None:
        self._running = True
        try:
            await self.register_with_orchestrator()
            await asyncio.gather(
                self.heartbeat_loop(),
                self.task_poll_loop(),
                self._disk_monitor_loop(),
                self._shutdown_event.wait(),
            )
        finally:
            await self.unregister_from_orchestrator()
            await self._close_channels()
            self._running = False


if __name__ == "__main__":
    import os
    agent = StorageAgent(agent_id=os.getenv("AIOS_AGENT_NAME", "storage-agent"))
    asyncio.run(agent.run())
