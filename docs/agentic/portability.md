# Agent Harness Portability

The repository keeps shared intent separate from tool syntax.

## Canonical Layer

| Purpose | Source of truth |
|---|---|
| Project rules | `AGENTS.md` |
| Agent responsibilities | `agents/*.md` |
| Agent metadata | `agents/registry.json` |
| Reusable procedures | `.agents/skills/*` |
| Model aliases | `models/registry.json` |

## Adapter Layer

`python3 scripts/generate-agent-adapters.py` renders small adapters for:

- Codex: `.codex/agents/*.toml`
- OpenCode: `.opencode/agents/*.md`
- Claude Code: `.claude/agents/*.md`
- Qwen Code: `.qwen/agents/*.md`

Adapters contain only syntax, permissions, and references needed by that harness. Shared engineering rules stay canonical.

## Skills

`.agents/skills/` is the canonical skill tree.

OpenCode discovers that path directly. Claude Code and Qwen Code use their native skill paths through directory symlinks:

```text
.claude/skills -> ../.agents/skills
.qwen/skills   -> ../.agents/skills
```

Run `./scripts/setup-tool-links.sh` after copying or extracting the template.

## Why Agents Are Generated Instead of Symlinked

Codex uses TOML. OpenCode, Claude Code, and Qwen Code use Markdown frontmatter, but their permission fields and supported metadata differ.

Sharing those adapter files would couple unrelated schemas. Generating them from one registry removes drift without pretending the formats are identical.

## Rule for New Harnesses

Use a symlink when the format and discovery contract are identical. Otherwise add a renderer to `scripts/generate-agent-adapters.py`.
