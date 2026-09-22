# Specification status

State: DRAFT

Updated: 2026-09-22

Product decisions: CONFIRMED in conversation. Implementation: NOT STARTED.
The old unanswered business-question list is superseded by those answers.

## Confirmed decisions

- Submission: PendingReview, Rejected or Accepted. Rejection immediately moves
  milestone to ChangesRequested.
- Only a current rejection enables opening. No timeout or extension now.
- Public URL + SHA-256 for deliverable, rejection and both formal arguments.
- Separate Open dispute; one active per project, project-wide freeze and one
  immutable counterparty response.
- Public getter without login; no chat, snapshots, resolution or unlock.
- Provider owns business truth; adapter handles REST/auth/transport/SSE and public
  projection; custody signs. Existing scoring rules remain.
- Real-service signed E2E ends with an open case and frozen project.

## Pending technical review

Review spec.md, design.md, data-model.md, threat-model.md, acceptance.feature
and tasks.md. The last technical proposal still needs an answer on old-route
retirement, signed-payload compatibility/fresh test state and keeping the
PostgreSQL migration separate. See spec.md for the exact scope.

Do not set APPROVED or claim implementation because consolidation is complete.

## Observed baseline

DisputeMilestone records a reason in the provider's internal aggregate, marks
only the target milestone Disputed/frozen and emits an event. The reason is not
encrypted; it is absent from public read DTOs. The command accepts InProgress
or CompletionRequested and has no formal case. Adapter forwards that command.

Submission history, rejection, formal case, response and public getter are not
implemented. Existing tests do not verify this specification.
Evidence: ../../progress/handoffs/DSP-00-reanalysis.md.

## Next step

Approve the consolidated technical scope, then execute DSP-01 through DSP-07.
Keep approval, implementation and observed verification as separate statuses.
