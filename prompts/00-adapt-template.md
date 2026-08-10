# Template Adaptation Prompt

Act as the planner and architecture facilitator for a newly configured repository. Read `AGENTS.md`, `docs/project/project-context.md`, `docs/agentic/operating-model.md`, the canonical role contracts, and the available skills.

Do not implement product code. Produce a proposed adaptation plan that:

1. Identifies missing concrete project information and asks for it without guessing.
2. Recommends which candidate bounded contexts should remain modules, become deployable services, or remain undecided.
3. Selects only the communication mechanisms justified by approved requirements.
4. Identifies irrelevant agents or skills that should remain unused rather than deleting reusable source material prematurely.
5. Proposes scoped `AGENTS.md` files for approved services and applications.
6. Proposes the first vertical-slice specification and its deterministic acceptance evidence.
7. Lists dependencies that require current registry research and dependency review.
8. Records all proposed changes before editing repository governance.

Wait for human approval before applying the adaptation. After approval, update project-specific documentation and create only the approved scoped files.
