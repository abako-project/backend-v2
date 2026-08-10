#!/usr/bin/env python3
"""Generate tool-specific agent adapters from the canonical registry."""
from __future__ import annotations

import json
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REGISTRY = json.loads((ROOT / "agents/registry.json").read_text(encoding="utf-8"))


def clean(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True)
    for child in path.iterdir():
        if child.is_file() or child.is_symlink():
            child.unlink()
        elif child.is_dir():
            shutil.rmtree(child)


def common_body(agent: dict) -> str:
    skills = ", ".join(f"`{name}`" for name in agent["skills"])
    return (
        "Read `AGENTS.md` first.\n\n"
        f"Follow `{agent['role']}` as the canonical role contract. "
        "Do not copy or reinterpret that role here.\n\n"
        f"Relevant shared skills: {skills}. Load only the skills needed for the assigned task.\n\n"
        "Respect the assigned write scope and worktree. If the role, specification, or scope is unclear, "
        "stop the affected branch and report the blocker.\n"
    )


def render_codex(agent: dict) -> str:
    sandbox = "read-only" if agent["access"] == "read-only" else "workspace-write"
    body = common_body(agent).replace('"""', '\\\"\\\"\\\"')
    return (
        f'name = "{agent["id"]}"\n'
        f'description = {json.dumps(agent["description"])}\n'
        f'sandbox_mode = "{sandbox}"\n\n'
        'developer_instructions = """\n'
        f'{body}'
        '"""\n'
    )


def render_opencode(agent: dict) -> str:
    edit = "deny" if agent["access"] == "read-only" else "ask"
    return (
        "---\n"
        f"description: {agent['description']}\n"
        "mode: subagent\n"
        "permission:\n"
        "  read: allow\n"
        "  glob: allow\n"
        "  grep: allow\n"
        "  list: allow\n"
        "  skill: allow\n"
        f"  edit: {edit}\n"
        "  bash: ask\n"
        "  external_directory: deny\n"
        "---\n\n"
        f"{common_body(agent)}"
    )


def render_claude(agent: dict) -> str:
    mode = "plan" if agent["access"] == "read-only" else "default"
    return (
        "---\n"
        f"name: {agent['id']}\n"
        f"description: {agent['description']}\n"
        "model: inherit\n"
        f"permissionMode: {mode}\n"
        "---\n\n"
        f"{common_body(agent)}"
    )


def render_qwen(agent: dict) -> str:
    mode = "plan" if agent["access"] == "read-only" else "default"
    return (
        "---\n"
        f"name: {agent['id']}\n"
        f"description: {agent['description']}\n"
        "model: inherit\n"
        f"approvalMode: {mode}\n"
        "---\n\n"
        f"{common_body(agent)}"
    )


OUTPUTS = {
    ".codex/agents": (".toml", render_codex),
    ".opencode/agents": (".md", render_opencode),
    ".claude/agents": (".md", render_claude),
    ".qwen/agents": (".md", render_qwen),
}

for directory in OUTPUTS:
    clean(ROOT / directory)

for agent in REGISTRY["agents"]:
    role = ROOT / agent["role"]
    if not role.is_file():
        raise SystemExit(f"Missing canonical role: {agent['role']}")
    for directory, (suffix, renderer) in OUTPUTS.items():
        target = ROOT / directory / f"{agent['id']}{suffix}"
        target.write_text(renderer(agent), encoding="utf-8")

print(f"Generated {len(REGISTRY['agents'])} agents for {len(OUTPUTS)} harnesses.")
