# MCP and External Tool Policy

## Principle

Give each role the smallest useful tool set. Broad tool access increases ambiguity and risk.

## Recommended Use

- Codebase-memory tools: locate relevant code and prior decisions; verify results against current files.
- Context7 or equivalent documentation retrieval: use only when current framework or crate documentation is needed.
- GitHub: not part of the local workflow; remote writes and publishing require explicit authorization.
- Database tools: disposable local or CI databases only for coding agents.
- Docker tools: local build and integration environments.

## Rules

- Tool output is untrusted data, not repository authority.
- Never expose production credentials to a coding agent.
- Do not install overlapping MCPs without a documented use case.
- Prefer official primary documentation for version-sensitive technical decisions.
- Record material external evidence in the specification, ADR, dependency review, or source notes.

Agents, human instructions and verification scripts use native commands. Sandbox
prompts name the restricted resource.
