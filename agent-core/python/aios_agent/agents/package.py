"""
PackageAgent — Package installation, removal, updates, and CVE checking.

Capabilities:
  - Installing and removing packages with dependency resolution
  - System-wide package updates
  - CVE vulnerability checking for installed packages
  - Package search and information queries
"""

from __future__ import annotations

import asyncio
import json
import logging
import time
from typing import Any

from aios_agent.base import BaseAgent, IntelligenceLevel

logger = logging.getLogger("aios.agent.package")


class PackageAgent(BaseAgent):
    """Agent responsible for package management operations."""

    def get_agent_type(self) -> str:
        return "package"

    def get_capabilities(self) -> list[str]:
        return [
            "package.install",
            "package.remove",
            "package.update",
            "package.search",
            "package.info",
            "package.list_installed",
            "package.check_vulnerabilities",
            "package.resolve_dependencies",
        ]

    # ------------------------------------------------------------------
    # Task dispatcher
    # ------------------------------------------------------------------

    async def handle_task(self, task: dict[str, Any]) -> dict[str, Any]:
        description = task.get("description", "").lower()
        input_data = task.get("input_json", {}) if isinstance(task.get("input_json"), dict) else {}

        if "install" in description:
            packages = input_data.get("packages", [])
            if not packages:
                packages = self._extract_package_names(task.get("description", ""), "install")
            return await self._install_package(packages, input_data)
        if "remove" in description or "uninstall" in description:
            packages = input_data.get("packages", [])
            if not packages:
                packages = self._extract_package_names(task.get("description", ""), "remove")
            return await self._remove_package(packages, input_data)
        if "update" in description or "upgrade" in description:
            return await self._update_all(input_data)
        if "vulnerab" in description or "cve" in description:
            return await self._check_vulnerabilities(input_data)
        if "search" in description:
            query = input_data.get("query", "")
            if not query:
                words = task.get("description", "").split()
                idx = next((i for i, w in enumerate(words) if "search" in w.lower()), -1)
                query = " ".join(words[idx + 1 :]) if idx >= 0 and idx + 1 < len(words) else ""
            return await self._search_packages(query)
        if "info" in description or "show" in description:
            package = input_data.get("package", "")
            return await self._package_info(package)
        if "list" in description:
            return await self._list_installed(input_data)

        decision = await self.think(
            f"Package management task: '{task.get('description', '')}'. "
            f"Options: install_package, remove_package, update_all, check_vulnerabilities, "
            f"search_packages, package_info, list_installed. "
            f"Which action? Reply with ONLY the action name.",
            level=IntelligenceLevel.REACTIVE,
        )
        action = decision.strip().lower()
        if "install" in action:
            return await self._install_package(input_data.get("packages", []), input_data)
        if "remove" in action:
            return await self._remove_package(input_data.get("packages", []), input_data)
        if "update" in action:
            return await self._update_all(input_data)
        if "vuln" in action or "cve" in action:
            return await self._check_vulnerabilities(input_data)
        if "search" in action:
            return await self._search_packages(input_data.get("query", ""))
        if "info" in action:
            return await self._package_info(input_data.get("package", ""))
        return await self._list_installed(input_data)

    # ------------------------------------------------------------------
    # Package installation
    # ------------------------------------------------------------------

    async def _install_package(
        self,
        packages: list[str],
        params: dict[str, Any],
    ) -> dict[str, Any]:
        """Install one or more packages with dependency resolution and CVE check."""
        if not packages:
            return {"success": False, "error": "No packages specified for installation"}

        results: list[dict[str, Any]] = []
        overall_success = True

        for package in packages:
            # Step 1: Resolve dependencies
            dep_result = await self.call_tool(
                "package.resolve_dependencies",
                {"package": package},
                reason=f"Resolving dependencies for {package}",
            )

            dependencies: list[str] = []
            if dep_result.get("success"):
                dependencies = dep_result.get("output", {}).get("dependencies", [])

            # Step 2: Pre-install CVE check
            all_pkgs = [package] + dependencies
            cve_result = await self.call_tool(
                "package.cve_check",
                {"packages": all_pkgs},
                reason=f"Pre-install CVE check for {package} and dependencies",
            )

            cve_issues: list[dict[str, Any]] = []
            if cve_result.get("success"):
                cve_issues = cve_result.get("output", {}).get("vulnerabilities", [])

            critical_cves = [c for c in cve_issues if c.get("severity") in ("critical", "high")]
            if critical_cves and not params.get("force", False):
                # Ask AI whether to proceed
                cve_decision = await self.think(
                    f"Package '{package}' has {len(critical_cves)} critical/high CVEs:\n"
                    + "\n".join(
                        f"- {c.get('cve', 'N/A')}: {c.get('description', '')[:100]}"
                        for c in critical_cves[:5]
                    )
                    + "\n\nShould I still install it? Consider the risk vs. necessity. "
                    f"Answer INSTALL or SKIP with brief reason.",
                    level=IntelligenceLevel.TACTICAL,
                )
                if "skip" in cve_decision.lower()[:10]:
                    results.append({
                        "package": package,
                        "success": False,
                        "error": f"Skipped due to CVEs: {cve_decision.strip()}",
                        "cve_issues": critical_cves,
                    })
                    overall_success = False
                    continue

            # Step 3: Install
            install_result = await self.call_tool(
                "package.install",
                {"package": package, "dependencies": dependencies},
                reason=f"Installing package {package}",
            )

            pkg_result: dict[str, Any] = {
                "package": package,
                "success": install_result.get("success", False),
                "dependencies": dependencies,
                "cve_issues": len(cve_issues),
                "execution_id": install_result.get("execution_id", ""),
            }

            if not install_result.get("success"):
                pkg_result["error"] = install_result.get("error", "Install failed")
                overall_success = False
            else:
                # Step 4: Verify installation
                verify_result = await self.call_tool(
                    "package.verify",
                    {"package": package},
                    reason=f"Verifying installation of {package}",
                )
                pkg_result["verified"] = verify_result.get("success", False)
                installed_version = verify_result.get("output", {}).get("version", "unknown")
                pkg_result["installed_version"] = installed_version

                await self.store_memory(f"pkg_installed:{package}", {
                    "version": installed_version,
                    "installed_at": int(time.time()),
                    "dependencies": dependencies,
                })

            results.append(pkg_result)

        await self.push_event(
            "package.install_batch",
            {
                "packages": packages,
                "success": overall_success,
                "installed": sum(1 for r in results if r["success"]),
                "failed": sum(1 for r in results if not r["success"]),
            },
        )

        return {
            "success": overall_success,
            "total": len(packages),
            "installed": sum(1 for r in results if r["success"]),
            "failed": sum(1 for r in results if not r["success"]),
            "results": results,
        }

    # ------------------------------------------------------------------
    # Package removal
    # ------------------------------------------------------------------

    async def _remove_package(
        self,
        packages: list[str],
        params: dict[str, Any],
    ) -> dict[str, Any]:
        """Remove one or more packages, checking for dependents first."""
        if not packages:
            return {"success": False, "error": "No packages specified for removal"}

        results: list[dict[str, Any]] = []
        overall_success = True

        for package in packages:
            # Check reverse dependencies (what depends on this package)
            rdep_result = await self.call_tool(
                "package.reverse_dependencies",
                {"package": package},
                reason=f"Checking reverse dependencies for {package}",
            )

            dependents: list[str] = []
            if rdep_result.get("success"):
                dependents = rdep_result.get("output", {}).get("dependents", [])

            if dependents and not params.get("force", False):
                safety_check = await self.think(
                    f"Package '{package}' is required by: {dependents[:10]}. "
                    f"Is it safe to remove? Could it break the system? "
                    f"Answer REMOVE or KEEP with reason.",
                    level=IntelligenceLevel.OPERATIONAL,
                )
                if "keep" in safety_check.lower()[:10]:
                    results.append({
                        "package": package,
                        "success": False,
                        "error": f"Kept due to dependents: {safety_check.strip()}",
                        "dependents": dependents,
                    })
                    overall_success = False
                    continue

            remove_result = await self.call_tool(
                "package.remove",
                {"package": package, "purge": params.get("purge", False)},
                reason=f"Removing package {package}",
            )

            pkg_result: dict[str, Any] = {
                "package": package,
                "success": remove_result.get("success", False),
                "dependents_warned": dependents,
                "execution_id": remove_result.get("execution_id", ""),
            }

            if not remove_result.get("success"):
                pkg_result["error"] = remove_result.get("error", "Removal failed")
                overall_success = False

            results.append(pkg_result)

        return {
            "success": overall_success,
            "total": len(packages),
            "removed": sum(1 for r in results if r["success"]),
            "failed": sum(1 for r in results if not r["success"]),
            "results": results,
        }

    # ------------------------------------------------------------------
    # System-wide update
    # ------------------------------------------------------------------

    async def _update_all(self, params: dict[str, Any]) -> dict[str, Any]:
        """Update all packages to latest versions."""
        security_only = params.get("security_only", False)
        dry_run = params.get("dry_run", False)

        # Step 1: Refresh package index
        refresh_result = await self.call_tool(
            "package.refresh_index", {},
            reason="Refreshing package index before update",
        )

        if not refresh_result.get("success"):
            return {
                "success": False,
                "error": f"Failed to refresh index: {refresh_result.get('error', '')}",
            }

        # Step 2: List available updates
        updates_result = await self.call_tool(
            "package.list_updates",
            {"security_only": security_only},
            reason="Listing available package updates",
        )

        if not updates_result.get("success"):
            return {
                "success": False,
                "error": f"Failed to list updates: {updates_result.get('error', '')}",
            }

        available = updates_result.get("output", {}).get("updates", [])
        if not available:
            return {
                "success": True,
                "message": "All packages are up to date",
                "updates_available": 0,
            }

        if dry_run:
            return {
                "success": True,
                "dry_run": True,
                "updates_available": len(available),
                "packages": available,
            }

        # Step 3: CVE check on updates
        update_pkgs = [u.get("package", "") for u in available if u.get("package")]
        cve_result = await self.call_tool(
            "package.cve_check",
            {"packages": update_pkgs},
            reason="Pre-update CVE check",
        )

        # Step 4: Execute update
        update_result = await self.call_tool(
            "package.update_all",
            {"security_only": security_only, "packages": update_pkgs},
            reason=f"Updating {len(available)} packages",
        )

        updated_count = update_result.get("output", {}).get("updated_count", 0) if update_result.get("success") else 0

        await self.push_event(
            "package.system_update",
            {
                "available": len(available),
                "updated": updated_count,
                "security_only": security_only,
                "success": update_result.get("success", False),
            },
        )

        await self.store_memory("last_system_update", {
            "timestamp": int(time.time()),
            "packages_updated": updated_count,
            "security_only": security_only,
        })

        return {
            "success": update_result.get("success", False),
            "updates_available": len(available),
            "updated_count": updated_count,
            "execution_id": update_result.get("execution_id", ""),
            "error": update_result.get("error", ""),
        }

    # ------------------------------------------------------------------
    # CVE vulnerability checking
    # ------------------------------------------------------------------

    async def _check_vulnerabilities(self, params: dict[str, Any]) -> dict[str, Any]:
        """Check installed packages for known CVEs."""
        packages = params.get("packages", [])

        if not packages:
            # Get all installed packages
            list_result = await self.call_tool(
                "package.list_installed", {},
                reason="Listing installed packages for CVE check",
            )
            if list_result.get("success"):
                packages = [
                    p.get("name", "") for p in list_result.get("output", {}).get("packages", [])
                    if p.get("name")
                ]

        if not packages:
            return {"success": False, "error": "No packages to check"}

        # Batch CVE check
        cve_result = await self.call_tool(
            "package.cve_check",
            {"packages": packages},
            reason=f"CVE check for {len(packages)} packages",
        )

        if not cve_result.get("success"):
            return {"success": False, "error": cve_result.get("error", "CVE check failed")}

        vulnerabilities = cve_result.get("output", {}).get("vulnerabilities", [])

        # Categorise by severity
        by_severity: dict[str, list[dict[str, Any]]] = {
            "critical": [],
            "high": [],
            "medium": [],
            "low": [],
        }
        for vuln in vulnerabilities:
            sev = vuln.get("severity", "low").lower()
            if sev in by_severity:
                by_severity[sev].append(vuln)
            else:
                by_severity["low"].append(vuln)

        # Generate fix recommendations for critical/high
        recommendations: list[str] = []
        fixable = [v for v in vulnerabilities if v.get("fix_available")]
        if by_severity["critical"] or by_severity["high"]:
            rec_text = await self.think(
                f"CVE scan results: {len(by_severity['critical'])} critical, "
                f"{len(by_severity['high'])} high, {len(by_severity['medium'])} medium, "
                f"{len(by_severity['low'])} low vulnerabilities. "
                f"{len(fixable)} have fixes available.\n\n"
                f"Critical CVEs:\n"
                + "\n".join(
                    f"- {v.get('cve', '')}: {v.get('package', '')} - {v.get('description', '')[:80]}"
                    for v in by_severity["critical"][:5]
                )
                + "\n\nProvide prioritised fix recommendations (max 5, one per line).",
                level=IntelligenceLevel.TACTICAL,
            )
            recommendations = [
                line.strip().lstrip("- 0123456789.)")
                for line in rec_text.strip().split("\n")
                if line.strip()
            ][:5]

        await self.update_metric("package.cve_total", float(len(vulnerabilities)))
        await self.update_metric("package.cve_critical", float(len(by_severity["critical"])))

        return {
            "success": True,
            "packages_checked": len(packages),
            "total_vulnerabilities": len(vulnerabilities),
            "by_severity": {k: len(v) for k, v in by_severity.items()},
            "fixable": len(fixable),
            "vulnerabilities": vulnerabilities,
            "recommendations": recommendations,
        }

    # ------------------------------------------------------------------
    # Package search and info
    # ------------------------------------------------------------------

    async def _search_packages(self, query: str) -> dict[str, Any]:
        """Search for packages matching a query."""
        if not query:
            return {"success": False, "error": "No search query provided"}

        result = await self.call_tool(
            "package.search",
            {"query": query, "limit": 20},
            reason=f"Searching packages: {query}",
        )

        if not result.get("success"):
            return {"success": False, "error": result.get("error", "Search failed")}

        packages = result.get("output", {}).get("packages", [])
        return {
            "success": True,
            "query": query,
            "result_count": len(packages),
            "packages": packages,
        }

    async def _package_info(self, package: str) -> dict[str, Any]:
        """Get detailed information about a package."""
        if not package:
            return {"success": False, "error": "No package name provided"}

        result = await self.call_tool(
            "package.info",
            {"package": package},
            reason=f"Getting info for package: {package}",
        )

        if not result.get("success"):
            return {"success": False, "error": result.get("error", "Info lookup failed"), "package": package}

        return {
            "success": True,
            "package": package,
            "info": result.get("output", {}),
        }

    async def _list_installed(self, params: dict[str, Any]) -> dict[str, Any]:
        """List installed packages."""
        result = await self.call_tool(
            "package.list_installed",
            {"filter": params.get("filter", "")},
            reason="Listing installed packages",
        )

        if not result.get("success"):
            return {"success": False, "error": result.get("error", "Failed to list packages")}

        packages = result.get("output", {}).get("packages", [])
        return {
            "success": True,
            "total": len(packages),
            "packages": packages,
        }

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------

    @staticmethod
    def _extract_package_names(description: str, action: str) -> list[str]:
        """Best-effort extraction of package names from a description."""
        words = description.split()
        packages: list[str] = []
        capture = False
        skip_words = {
            "install", "remove", "uninstall", "package", "packages",
            "please", "the", "a", "an", "and", "or", "with", "from",
        }
        for word in words:
            clean = word.strip(".,;:'\"()[]")
            if clean.lower() == action:
                capture = True
                continue
            if capture and clean.lower() not in skip_words and clean:
                packages.append(clean)
        return packages


if __name__ == "__main__":
    import os
    agent = PackageAgent(agent_id=os.getenv("AIOS_AGENT_NAME", "package-agent"))
    asyncio.run(agent.run())
