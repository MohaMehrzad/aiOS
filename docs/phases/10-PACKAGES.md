# Phase 10: AI Package Manager

## Goal
Build an AI-driven package manager that can install, update, and remove software autonomously, with dependency resolution, vulnerability tracking, and self-healing.

## Prerequisites
- Phase 8 complete (networking for downloading packages)
- Phase 9 complete (security for package verification)

---

## Step-by-Step

### Step 10.1: Choose Package Backend

We use **Alpine's apk** as the underlying package manager (small, fast, musl-based) with an AI wrapper layer on top.

Alternative: Build a custom package manager using tar archives + dependency manifest. Simpler but less ecosystem access.

**Decision**: Start with apk for access to Alpine's package repository, wrap with AI layer.

### Step 10.2: Implement Package Tools

**Claude Code prompt**: "Implement all pkg.* tools — search, install, remove, update, list, info, check_vulnerabilities"

```
pkg.search           — Search repositories for a package
pkg.install          — Install a package (resolve deps, download, install)
pkg.remove           — Remove a package (check reverse deps first)
pkg.update           — Update a specific package or all packages
pkg.list             — List all installed packages
pkg.info             — Get detailed info about a package
pkg.check_vuln       — Check installed packages against CVE database
pkg.lock             — Lock a package at current version (prevent updates)
pkg.unlock           — Unlock a locked package
pkg.history          — Show package operation history
pkg.rollback         — Rollback last package operation
```

### Step 10.3: Implement AI Wrapper

**Claude Code prompt**: "Implement the AI package management wrapper — the Package Agent uses intelligence to decide WHAT to install, WHEN to update, and handles dependency conflicts"

```python
# pkg-manager/python/ai_wrapper.py

class AIPackageManager:
    """AI-enhanced package management."""

    async def smart_install(self, description: str) -> InstallResult:
        """Install software from a natural language description."""
        # Example: "I need a web server" → installs nginx
        # Example: "Set up Python 3.12 development environment" → installs python3, pip, venv, gcc

        # 1. Use AI to determine required packages
        analysis = await self.think(
            f"What packages should I install to satisfy: '{description}'?\n"
            f"Available package manager: apk (Alpine)\n"
            f"Currently installed: {await self.list_installed()}\n"
            f"Return a JSON list of package names.",
            level="tactical"
        )
        packages = parse_package_list(analysis)

        # 2. Check for conflicts
        for pkg in packages:
            conflicts = await self.check_conflicts(pkg)
            if conflicts:
                resolution = await self.resolve_conflicts(pkg, conflicts)
                packages = resolution.updated_packages

        # 3. Security check
        for pkg in packages:
            vulns = await self.check_vulnerabilities(pkg)
            if vulns.has_critical():
                # Escalate to Claude for security decision
                decision = await self.think(
                    f"Package {pkg} has critical vulnerabilities: {vulns}\n"
                    f"Should I still install it? Are there alternatives?",
                    level="strategic"
                )
                # Handle based on decision

        # 4. Install
        results = []
        for pkg in packages:
            result = await self.call_tool("pkg.install", package=pkg)
            results.append(result)

        return InstallResult(packages=packages, results=results)

    async def auto_update(self):
        """Automatically update packages with AI-driven risk assessment."""
        updates = await self.call_tool("pkg.list", filter="upgradable")

        for pkg in updates.data["packages"]:
            # Assess risk of updating
            risk = await self.assess_update_risk(pkg)

            if risk == "low":
                await self.call_tool("pkg.update", package=pkg["name"])
            elif risk == "medium":
                # Update in a sandbox first, test, then apply
                await self.sandboxed_update(pkg)
            elif risk == "high":
                # Log and skip — notify human
                await self.log_deferred_update(pkg, reason="high risk")
```

### Step 10.4: Implement Package Agent

**Claude Code prompt**: "Implement the Package Agent that manages all package operations, runs scheduled updates, and monitors for vulnerabilities"

### Step 10.5: Implement Vulnerability Monitoring

**Claude Code prompt**: "Implement CVE monitoring — periodically check installed packages against known vulnerability databases"

### Step 10.6: Integration Test

**Claude Code prompt**: "Test: ask the Package Agent to install nginx, verify it resolves dependencies, downloads, installs, and the binary is available"

---

## Deliverables Checklist

- [ ] Package backend (apk) integrated into rootfs
- [ ] All pkg.* tools implemented
- [ ] AI wrapper handles natural language install requests
- [ ] Dependency resolution works
- [ ] Vulnerability checking works
- [ ] Package Agent runs scheduled updates
- [ ] Package rollback works
- [ ] Integration test: install nginx end-to-end

---

## Next Phase
→ [Phase 11: API Gateway](./11-API-GATEWAY.md)
