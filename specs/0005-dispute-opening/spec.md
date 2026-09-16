# SPEC-0005: Apertura de disputas de milestone

Status: DRAFT

Created: 2026-09-16

Last updated: 2026-09-16

## Problem

SPEC-0003 permite que el cliente o el coordinador marque un milestone como
disputado y congele sus fondos. Esa operación no modela el rechazo previo del
cliente ni crea un expediente consultable. Tampoco conserva una evidencia
inmutable, admite la respuesta de la contraparte, crea un canal o publica una
vista saneada.

La prueba de concepto debe demostrar la apertura documentada de una disputa a
partir del primer rechazo. Resolverla pertenece a una iteración posterior.

## Goals

- Registrar el rechazo de una solicitud de finalización con un motivo obligatorio.
- Permitir que el cliente o el coordinador asignado abra voluntariamente una
  disputa después de un rechazo del mismo milestone.
- Crear atómicamente el expediente, argumento inicial, evidencia disponible,
  canal, congelación y eventos de apertura.
- Permitir que la contraparte añada argumentos sin modificar los anteriores.
- Ofrecer una vista pública saneada cuando se aprueben sus reglas de visibilidad.
- Mantener el escrow sin pago ni devolución mientras no exista una resolución
  aprobada en otra especificación.

## Non-goals

- Resolver, rechazar o cancelar una disputa.
- Elegir juez, árbitro o miembro de la DAO.
- Definir electorado, votación, quorum, plazo o ejecución de gobernanza.
- Distribuir, devolver o penalizar fondos disputados.
- Resolver disputas de planificación; el comportamiento vigente de SPEC-0003
  no se amplía aquí.
- Inventar entregables, versiones, criterios de aceptación o comunicaciones que
  la plataforma no haya registrado.
- Restaurar tablas PostgreSQL del documento de origen. El mock transaccional
  continúa siendo la fuente de verdad de la PoC.

## Actors

| Actor | Permissions in this scope |
|---|---|
| Client | Reject its milestone completion; open a dispute; answer when it is the counterparty |
| Assigned coordinator | Open a dispute; answer when it is the counterparty |
| Assigned worker | No mutation permission on a dispute |
| Public reader | Read only the fields approved for the public view; exact audience is unresolved |
| DAO or judge | No permission in this scope |
| System authority | No power to resolve or move disputed funds in this scope |

## Definitions

- **Completion request:** the coordinator's recorded request for the client to
  accept a milestone as completed.
- **Rejection:** the client's recorded refusal of one completion request,
  including its mandatory reason.
- **Dispute:** an append-only case associated with one project and milestone,
  opened after at least one rejection of that milestone.
- **Counterparty:** the other project principal: coordinator when the client
  opens, or client when the coordinator opens.
- **Evidence snapshot:** an immutable copy of the project, milestone, task and
  event data available to the provider when the dispute opens.
- **Current view:** live project data after opening. It is not evidence for the
  original opening unless appended through a future approved operation.

## Functional requirements

### REJ-001: Reject milestone completion with a reason

Only the project client may reject a milestone whose completion is awaiting
review. A rejection must contain a non-blank reason and identify the completion
request it rejects. The provider records author, milestone, reason and Unix
timestamp atomically and emits `MilestoneCompletionRejected`.

The rejection releases no funds and records no scores. A missing or invalid
reason commits no state.

### REJ-002: Rejection does not open a dispute

The first and subsequent rejections make dispute opening eligible but never
create a dispute automatically. The ordinary milestone state after rejection
and the resubmission rule are blocking product decisions.

### DSP-001: Authorized and justified opening

The project client or its assigned coordinator may open a milestone dispute
only when that milestone has a recorded rejection. The opening argument is
mandatory and non-blank. Workers, former/unassigned coordinators and unrelated
accounts cannot open it.

Whether more than one dispute may exist for the same milestone or rejection is
not approved. Existing operation-id replay protection still prevents one signed
operation from creating duplicate effects.

### DSP-002: Atomic dispute case

A successful opening commits one transaction containing:

- a provider-generated dispute ID;
- the project and milestone references;
- the opener, counterparty, opening time and opening argument;
- an immutable snapshot of available evidence;
- one linked communication channel;
- the milestone and escrow freeze required by SPEC-0003;
- durable opening, channel and publication events required by the approved
  status model.

If any part fails, none of those effects commits. The current `DisputeMilestone`
operation does not satisfy this requirement because it can run without a prior
rejection and stores no dispute entity.

### DSP-003: Immutable available evidence

Opening captures the data already recorded for the project and milestone,
including the proposal and milestone definition, contractual assignments, task
storage and tasks, completion and rejection history, and related domain events
up to a fixed cursor. The snapshot records its capture time and schema version.

Later task, project or event changes do not mutate that snapshot. Data absent
from the platform at opening is listed as unavailable; it is not reconstructed
from user claims. The mock stores this versioned snapshot in its provider
aggregate. Canonical chain encoding and any off-chain integrity commitment are
deferred until the real-chain design; neither is needed to demonstrate opening.

### DSP-004: Append-only counterparty arguments

The counterparty may append an authenticated argument containing author, type,
content and Unix timestamp. Existing arguments cannot be updated, reordered or
deleted. The public representation of each argument follows the approved
redaction policy.

Whether the opener may append later arguments, and whether repeated counterparty
responses are allowed, remain product decisions.

### DSP-005: Linked communication channel

Opening creates exactly one channel linked to the dispute and makes it available
to the client and assigned coordinator. Channel creation must not depend on a
best-effort webhook. Message protocol, retention and whether messages become
formal dispute arguments are blocking decisions.

### DSP-006: Public sanitized view

The system exposes a dedicated dispute view rather than the provider's internal
aggregate. At minimum the product document requests project and milestone
identity, dispute state, opening time, opener, opening argument, relevant
milestone history and later arguments.

The exact audience, identifiers, field-level redaction and treatment of task and
event contents require approval before this endpoint is implemented. Custody
data, credentials, private keys, session data, internal receipts and unrelated
project data are never public.

### DSP-007: Funds remain frozen

Opening freezes the disputed milestone and its unreleased portion of execution
escrow. It cannot reverse settled payments, release reservations, pay, refund or
change reputation. No command in this scope unfreezes or disposes of funds.

### DSP-008: Durable events and notification

The provider records the committed dispute events in its ordered event stream.
At minimum the counterparty receives an authenticated notification that the
dispute opened. Public listing does not expose private notification state.

### DSP-009: Storage and retry equivalence

Memory and SQLite storage commit the same dispute state, evidence, funds and
events. Duplicate delivery, a lost HTTP response or an operation replay cannot
create another argument, channel, snapshot or freeze effect for the same
operation ID.

## Invariants

- INV-001: Every dispute references an existing project, milestone and recorded
  rejection of that milestone.
- INV-002: The opener is the project client or assigned coordinator; the
  counterparty is the other one.
- INV-003: Every dispute has exactly one opening argument, evidence snapshot and
  communication channel.
- INV-004: Rejections and arguments are append-only.
- INV-005: Snapshot content and its event-cursor boundary never change.
- INV-006: Dispute opening conserves total token supply and keeps unreleased
  milestone funds in execution escrow.
- INV-007: A disputed milestone cannot complete or pay through the normal
  acceptance command.
- INV-008: Failed commands commit no partial dispute state, business event or
  balance change.
- INV-009: Public output is produced from an allowlisted DTO, never by serializing
  internal provider state.

## Failure behavior

| Condition | Observable result | State effect |
|---|---|---|
| Blank rejection reason | Validation failure | None |
| Rejection by a non-client | Forbidden | None |
| Rejection outside the approved review state | Domain rejection | None |
| Opening without a recorded rejection | Domain rejection | None |
| Opening by a non-party | Forbidden | None |
| Blank opening argument | Validation failure | None |
| Argument from anyone other than the approved counterparty | Forbidden | None |
| Evidence/channel creation failure | Operation failure | Full rollback |
| Normal completion acceptance after dispute | Domain rejection | Funds remain frozen |
| Replayed operation ID | Original receipt | No duplicate effect |

Stable public error codes are frozen with the contracts task, not invented in
this draft.

## Contract impact

Proposed signed commands are `RejectMilestoneCompletion`,
`OpenMilestoneDispute` and `AddDisputeArgument`. Proposed query DTOs are
`DisputeView`, `DisputePublicView`, `DisputeArgumentView`,
`DisputeEvidenceView` and `DisputeChannelView`.

Proposed REST surface:

- `POST /api/projects/{projectId}/milestones/{milestoneId}/reject-completion`
- `POST /api/projects/{projectId}/milestones/{milestoneId}/disputes`
- `POST /api/disputes/{disputeId}/arguments`
- `GET /api/disputes/{disputeId}`
- `GET /api/disputes/{disputeId}/history`

Public listing and channel-message routes depend on the open visibility and
channel decisions. No generic provider command endpoint is exposed.

Published event candidates are `MilestoneCompletionRejected`, `DisputeOpened`,
`DisputeChannelCreated`, `DisputePublished` and `DisputeArgumentAdded`. The
meaning and order of publication-related events depend on the status decision.

Replacing the current `DisputeMilestone` contract is a compatibility change.
The contracts task must choose an explicit payload-version or transition policy;
the permissive old behavior cannot remain an alternate path.

## Data ownership and consistency

The mock provider owns rejection, dispute, evidence, arguments, channel identity,
freeze state and domain events. The adapter owns only authentication, transport
operations and notification read state. Custody owns signing keys. A future real
chain implementation must preserve this authority split.

Opening is one provider transaction under SPEC-0003 DOM-002. Reads may use a
separate sanitized projection but cannot become a second source of truth.

## Security and privacy

- The adapter checks session ownership; the provider checks the verified signing
  origin again.
- Actor IDs are derived from the signed origin, never request-body fields.
- Arguments and rejection reasons have bounded length and are treated as
  untrusted text. Limits must be frozen in the contract.
- Public evidence uses explicit allowlists and cannot include secrets or unrelated
  participants' private data.
- The immutable internal record remains distinct from its redacted public view.
- Logs record IDs, result and event cursors, not argument or evidence content.

## Test strategy

- Contract tests cover validation, serialization and compatibility in dedicated
  `tests.rs` files.
- Provider tests cover authorization, state transitions, rollback, escrow
  conservation, immutability and replay for memory and SQLite.
- Adapter tests cover session authorization, public redaction, operation polling
  and SSE delivery.
- End-to-end tests cover rejection, optional opening, evidence capture,
  counterparty response, notification and frozen funds through signed HTTP.
- An independent verifier maps every approved requirement to a Gherkin scenario.

## Open questions

| ID | Blocking question |
|---|---|
| Q-001 | After rejection, does the milestone return to `InProgress`, remain `CompletionRequested`, or gain a separate review state? How is a new completion version submitted? |
| Q-002 | Are `Open`, `Public` and `PendingDaoResolution` distinct stored states? Which atomic action triggers each transition? |
| Q-003 | Is the public view unauthenticated, member-only or authenticated-platform-only? Which identity, task, event and argument fields are redacted? |
| Q-004 | Are channel messages separate private communication, formal public arguments, or promotable evidence? What are their retention rules? |
| Q-006 | Can one milestone or one rejection have multiple disputes? If not, what conflict result is returned? |
| Q-007 | May the opener append later arguments, and may the counterparty add more than one response? |
| Q-008 | What recorded object identifies a completion submission/version? Current contracts record neither deliverable content nor a completion-request ID. |

## Approval

Product, architecture, security and data approval are pending. No implementation
task may start while this specification remains `DRAFT`.
