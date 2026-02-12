"""
CreatorAgent — Project creation and code generation agent.

Capabilities:
  - Scaffold new projects from templates (Rust, Python, Node, generic)
  - Generate code files using AI reasoning
  - Initialize git repositories for new projects
  - End-to-end project creation: scaffold -> generate -> git init -> commit
"""

from __future__ import annotations

import json
import logging
import time
import uuid
from typing import Any

from aios_agent.base import BaseAgent, IntelligenceLevel

logger = logging.getLogger("aios.agent.creator")


class CreatorAgent(BaseAgent):
    """Agent that creates projects, generates code, and manages repositories."""

    def get_agent_type(self) -> str:
        return "creator"

    def get_capabilities(self) -> list[str]:
        return [
            "creator.scaffold",
            "creator.generate_code",
            "creator.init_repo",
            "creator.full_project",
            "code_gen",
            "git_write",
            "fs_write",
        ]

    # ------------------------------------------------------------------
    # Task dispatcher
    # ------------------------------------------------------------------

    async def handle_task(self, task: dict[str, Any]) -> dict[str, Any]:
        description = task.get("description", "").lower()
        input_data = task.get("input_json", {}) if isinstance(task.get("input_json"), dict) else {}

        if "scaffold" in description:
            return await self._scaffold_project(input_data, task)
        if "generate" in description and "code" in description:
            return await self._generate_code(input_data, task)
        if "init" in description and ("repo" in description or "git" in description):
            return await self._init_repo(input_data, task)
        if "project" in description or "create" in description:
            return await self._full_project(input_data, task)

        # Default: try to interpret as a project creation request
        return await self._full_project(input_data, task)

    # ------------------------------------------------------------------
    # Scaffold a project
    # ------------------------------------------------------------------

    async def _scaffold_project(
        self,
        params: dict[str, Any],
        task: dict[str, Any],
    ) -> dict[str, Any]:
        """Create a project directory structure from a template."""
        name = params.get("name", task.get("description", "new-project").split()[-1])
        project_type = params.get("project_type", "generic")
        path = params.get("path", "/tmp/aios-projects")
        description = params.get("description", task.get("description", ""))

        result = await self.call_tool(
            "code.scaffold",
            {
                "name": name,
                "project_type": project_type,
                "path": path,
                "description": description,
            },
            reason=f"Scaffolding {project_type} project: {name}",
        )

        if result.get("success"):
            await self.push_event(
                "creator.project_scaffolded",
                {
                    "name": name,
                    "project_type": project_type,
                    "path": result.get("output", {}).get("path", ""),
                    "files_created": len(result.get("output", {}).get("files_created", [])),
                },
            )

        return result

    # ------------------------------------------------------------------
    # Generate code
    # ------------------------------------------------------------------

    async def _generate_code(
        self,
        params: dict[str, Any],
        task: dict[str, Any],
    ) -> dict[str, Any]:
        """Generate a code file using AI reasoning."""
        file_path = params.get("file_path", "")
        description = params.get("description", task.get("description", ""))
        language = params.get("language", "")

        if not file_path:
            return {"success": False, "error": "file_path is required for code generation"}

        # Use AI to plan the code structure
        plan_prompt = (
            f"Plan the implementation for the following code:\n"
            f"File: {file_path}\n"
            f"Language: {language or 'infer from extension'}\n"
            f"Description: {description}\n\n"
            f"Return a JSON object with:\n"
            f"- \"outline\": brief structural outline of the code\n"
            f"- \"key_functions\": list of main functions/classes to implement\n"
            f"- \"dependencies\": list of required imports/packages"
        )

        plan_text = await self.think(
            plan_prompt,
            level=IntelligenceLevel.TACTICAL,
            task_id=task.get("id"),
        )

        # Search memory for similar past code generation
        past_code = await self.semantic_search(
            f"code generation: {description}",
            collections=["procedures"],
            n_results=3,
        )

        # Generate the code file
        result = await self.call_tool(
            "code.generate",
            {
                "file_path": file_path,
                "description": description,
                "language": language,
            },
            reason=f"Generating code: {description}",
        )

        if result.get("success"):
            await self.store_decision(
                context=f"Generated code: {file_path}",
                options=["template_generation", "ai_generation"],
                chosen="template_generation",
                reasoning=f"Generated {language or 'auto'} code for: {description}",
                intelligence_level="tactical",
            )

        return result

    # ------------------------------------------------------------------
    # Initialize git repo
    # ------------------------------------------------------------------

    async def _init_repo(
        self,
        params: dict[str, Any],
        task: dict[str, Any],
    ) -> dict[str, Any]:
        """Initialize a git repository and make an initial commit."""
        repo_path = params.get("path", params.get("repo_path", ""))
        if not repo_path:
            return {"success": False, "error": "path is required for git init"}

        # Init repo
        init_result = await self.call_tool(
            "git.init",
            {"path": repo_path},
            reason="Initialize git repository",
        )

        if not init_result.get("success"):
            return init_result

        # Stage all files
        add_result = await self.call_tool(
            "git.add",
            {"repo_path": repo_path, "all": True},
            reason="Stage all files for initial commit",
        )

        if not add_result.get("success"):
            return add_result

        # Initial commit
        commit_result = await self.call_tool(
            "git.commit",
            {
                "repo_path": repo_path,
                "message": "Initial commit — scaffolded by aiOS CreatorAgent",
                "author": "aiOS CreatorAgent <aios@localhost>",
            },
            reason="Create initial commit",
        )

        return {
            "success": commit_result.get("success", False),
            "repo_path": repo_path,
            "commit_hash": commit_result.get("output", {}).get("commit_hash", ""),
            "steps": ["git.init", "git.add", "git.commit"],
        }

    # ------------------------------------------------------------------
    # Full project creation
    # ------------------------------------------------------------------

    async def _full_project(
        self,
        params: dict[str, Any],
        task: dict[str, Any],
    ) -> dict[str, Any]:
        """End-to-end project creation: scaffold -> generate -> git init -> commit."""
        description = task.get("description", "")

        # Use AI to plan the project
        plan_prompt = (
            f"Plan a new software project based on this description:\n"
            f"{description}\n\n"
            f"Return a JSON object with:\n"
            f"- \"name\": project name (lowercase, hyphenated)\n"
            f"- \"project_type\": one of rust, python, node, generic\n"
            f"- \"description\": one-line project description\n"
            f"- \"files\": list of files to generate beyond the scaffold, "
            f"each with {{\"path\": \"relative/path\", \"description\": \"what it does\"}}"
        )

        plan_text = await self.think(
            plan_prompt,
            level=IntelligenceLevel.STRATEGIC,
            task_id=task.get("id"),
        )

        # Parse the plan
        try:
            plan = json.loads(plan_text.strip().strip("`").removeprefix("json").strip())
        except json.JSONDecodeError:
            # Fallback plan
            plan = {
                "name": params.get("name", f"project-{uuid.uuid4().hex[:6]}"),
                "project_type": params.get("project_type", "generic"),
                "description": description,
                "files": [],
            }

        name = plan.get("name", f"project-{uuid.uuid4().hex[:6]}")
        project_type = plan.get("project_type", "generic")
        proj_description = plan.get("description", description)
        path = params.get("path", "/tmp/aios-projects")

        results: dict[str, Any] = {"steps": []}

        # Step 1: Scaffold
        scaffold_result = await self._scaffold_project(
            {
                "name": name,
                "project_type": project_type,
                "path": path,
                "description": proj_description,
            },
            task,
        )
        results["steps"].append({"step": "scaffold", "result": scaffold_result})

        project_path = scaffold_result.get("output", {}).get("path", f"{path}/{name}")

        # Step 2: Generate additional files
        additional_files = plan.get("files", [])
        for file_spec in additional_files[:10]:  # Limit to 10 files
            file_path = f"{project_path}/{file_spec.get('path', '')}"
            if not file_path or file_path == f"{project_path}/":
                continue

            gen_result = await self._generate_code(
                {
                    "file_path": file_path,
                    "description": file_spec.get("description", ""),
                },
                task,
            )
            results["steps"].append({
                "step": "generate",
                "file": file_spec.get("path", ""),
                "result": gen_result,
            })

        # Step 3: Initialize git
        git_result = await self._init_repo({"path": project_path}, task)
        results["steps"].append({"step": "git_init", "result": git_result})

        # Store the project creation as a learned pattern
        await self.store_pattern(
            trigger=f"create {project_type} project",
            action=f"scaffold({project_type}) + generate({len(additional_files)} files) + git_init",
            success_rate=1.0 if git_result.get("success") else 0.5,
            created_from="creator-agent",
        )

        results["success"] = git_result.get("success", False)
        results["project_path"] = project_path
        results["project_type"] = project_type
        results["name"] = name

        return results
