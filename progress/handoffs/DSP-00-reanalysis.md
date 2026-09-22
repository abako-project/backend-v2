# DSP-00: Dispute specification reanalysis

Date: 2026-09-22
Task: DSP-00, documentation consolidation and read-only implementation review.
Base: 792e4ce (master). Branch: spec/disputes-consolidation.
Worktree: existing repository checkout; no additional worktree created.
Writer: Codex integrator.

## Outcome

Consolidated the owner's confirmed delivery/rejection states, public URL/SHA-256
references, current-rejection eligibility, project-wide freeze, single response,
public getter and no-resolution endpoint. Removed superseded timeout, extension,
channel and snapshot requirements from the active dispute draft.

SPEC-0005 remains DRAFT for technical review, not unanswered business decisions.
No Rust code, endpoint or runnable dispute E2E was implemented in this task.

## Graph and source evidence

codebase-memory-mcp 0.10.8 is usable through its CLI outside the sandbox.
Inside, secure coordination fails on the containing-directory ownership check.
The first kunveno-rust graph was dated 2026-09-15 and referenced pre-refactor
symbols. Reindexing corrected it; coverage reported generation
2026-09-22T20:20:28Z. Relevant Rust/source paths report metadata_match and no
recorded parse gaps. Coverage is best-effort, not a completeness guarantee.

Commands used, from the repository root:

```sh
codebase-memory-mcp cli --json list_projects
codebase-memory-mcp cli --json index_status --project kunveno-rust
codebase-memory-mcp cli --json index_repository --repo-path . --name kunveno-rust --mode fast
codebase-memory-mcp cli --json search_graph --project kunveno-rust --name-pattern '.*(apply_project_command|milestone_command|lifecycle_command|authorize|boundary|execute).*' --limit 30
codebase-memory-mcp cli --json trace_path --project kunveno-rust --function-name apply_project_command --direction both --depth 2 --limit 45
codebase-memory-mcp cli --json get_code_snippet --project kunveno-rust --qualified-name kunveno-rust.services.mock-provider.src.domain.project.State.lifecycle_command
codebase-memory-mcp cli --json get_code_snippet --project kunveno-rust --qualified-name kunveno-rust.services.mock-provider.src.domain.commands.State.apply_project_command
```

Also traced enqueue in both directions and checked exact coverage paths for
provider project/commands/validation, adapter http/operations, generated contracts
and poc-e2e.py. Array arguments passed as flag strings were interpreted literally
in one coverage call; repeated with a structured JSON argument and inspected the
actual per-file results. Structural traces contain heuristic edges: exact source
reads, rather than every reported edge, substantiate the following findings.

| Finding | Source and consequence |
|---|---|
| Current dispute is a flag/reason | mock-provider domain/project.rs lifecycle_command: accepts InProgress/CompletionRequested, sets target Disputed/frozen, records reason; no case |
| Central freeze location exists | domain/commands.rs apply_project_command: checks cancellation then routes all existing project commands; add dispute guard here |
| Current acceptance lacks submission identity | generated-contracts/lib.rs completion DTOs and project.rs milestone_command: extend review with exact submission ID to prevent stale reviews |
| No public case auth exception | adapter-api/src/http.rs boundary: only health/auth/OpenAPI are public; add exact case GET/HEAD, keep POST protected |
| Existing signing/event infrastructure is reusable | adapter operations, provider storage/execute and notifications implement custody transport, atomic receipts and ordered SSE ingestion |
| Existing E2E covers marketplace, not formal disputes | scripts/poc-e2e.py exercise/exercise_multi_milestone; update payloads and retain 5/3/4/2 outcomes |
| New tracing needs executable setup | No tracing/info!/warn!/error! references found in scoped service Rust/manifests; include subscriber and log evidence in implementation |

## Corrections that matter for implementation

- URL hashing binds artifact bytes only when a reader verifies them. It does not
  validate delivery quality, host content, freeze a repository or recover a file.
- No snapshots means retained context is state at opening, not a historic copy
  of task contents at submission. Current events do not encode full old tasks.
- The response must bypass only the project mutation prohibition for its own
  append; it must not provide a generic mutation escape hatch.
- Exact replay must return before the active-dispute guard. New IDs obey it.
- A public dispute DTO must exclude internal event recipients, receipts and
  unrelated project/personal data, even though public references are intentional.

## Changed scope

Disputas.md adenda; SPEC-0005 spec/design/data-model/threat-model/acceptance/tasks/
status; README.md, SPECS.md, docs/project/porting-coverage.md, progress/README.md
and this handoff. Historical handoffs remain historical, not silently rewritten.

## Verification

- git diff --check passed.
- Programmatic comparison confirms Disputas.md's historical body matches HEAD,
  allowing only the existing introductory note and final adenda; terminal newline
  normalized for comparison.
- All 12 requirement identifiers have Gherkin tags across 26 scenarios/outlines.
  This is specification traceability, not executed acceptance coverage.
- No product code changed. No cargo tests, dispute E2E or cargo-deny was run.

## Exact pending decision

The owner has not answered the final technical proposal: retire old completion/
dispute routes, bump payload version with fresh disposable test state, and defer
the requested global PostgreSQL migration from this slice. The consolidated
specification and task plan are now concrete review artifacts. Once that scope
is accepted, record APPROVED and execute DSP-01 through DSP-07. Do not reopen
already confirmed business rules or infer production approval.
