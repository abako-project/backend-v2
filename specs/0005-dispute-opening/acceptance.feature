@spec_SPEC_0005
Feature: Reject a milestone and open a public dispute
  The mock owns business state and executes authenticated signed commands.
  This PoC ends with an Open dispute and a frozen project, without resolution.

  Background:
    Given an approved project has a client and an assigned coordinator
    And its milestones have funded execution escrow and reserved work minutes

  Rule: Deliveries are versioned and reviewed explicitly

    @SUB_001 @REF_001
    Scenario: Coordinator submits a delivery
      Given the milestone is InProgress
      When its coordinator submits an evidence URL and SHA-256 with every worker rating
      Then a new submission ID and version are recorded with PendingReview
      And the milestone becomes CompletionRequested
      And balances reputation and reservations remain unchanged

    @SUB_002
    Scenario: Client accepts the exact current submission
      Given the current submission is PendingReview
      When the client accepts that submission with the existing client ratings
      Then that submission becomes Accepted and the milestone becomes Completed
      And the existing payouts and reputation contributions commit exactly once

    @REJ_001
    Scenario: Rejection immediately requests changes without opening a dispute
      Given the current submission is PendingReview
      When the client rejects it with a valid reason reference
      Then it becomes Rejected with the client and timestamp
      And the milestone becomes ChangesRequested
      And no dispute payment reputation change or reservation change occurs

    @REJ_002 @SUB_002 @DSP_001
    Scenario: Resubmission prevents stale review and stale dispute opening
      Given the client rejected submission version 1
      When the coordinator submits version 2
      Then version 2 is PendingReview and version 1 remains Rejected
      And the milestone becomes CompletionRequested
      And reviewing version 1 or opening from its rejection is rejected

    @SUB_001 @SUB_002 @REJ_001 @authorization
    Scenario Outline: Only the designated actor can submit or review
      When the <actor> attempts to <action>
      Then the request is forbidden and domain state is unchanged
      Examples:
        | actor                | action          |
        | client               | submit delivery |
        | assigned worker      | submit delivery |
        | assigned coordinator | reject delivery |
        | unrelated account    | accept delivery |

    @REF_001
    Scenario Outline: Malformed references cannot enter recorded evidence
      When a command supplies <reference>
      Then validation fails without committing domain effects
      Examples:
        | reference                    |
        | a blank URL                  |
        | a URL with credentials       |
        | a URL with control characters |
        | a URL above the byte limit   |
        | a digest of the wrong length |
        | a non-hexadecimal digest     |

    @REF_001
    Scenario: Reference registration does not fetch external content
      Given an evidence URL is structurally valid but unavailable
      When an otherwise valid submission records that reference and its declared digest
      Then the reference is recorded without a backend fetch
      And the system does not claim the content was verified or retained

  Rule: A current rejection enables a voluntary dispute

    @DSP_001 @DSP_002
    Scenario Outline: Either principal opens from the current rejection
      Given the milestone is ChangesRequested with its latest submission Rejected
      When the <opener> opens a dispute against that submission with evidence
      Then one Open dispute is created with the other principal as counterparty
      And the project links that dispute and the milestone becomes Disputed
      And the pre-opening event cursor and existing proposal revision are recorded
      Examples:
        | opener               |
        | client               |
        | assigned coordinator |

    @DSP_001
    Scenario Outline: Other milestone states do not enable opening
      Given the milestone is <state>
      When a principal attempts to open a dispute
      Then no dispute or freeze is committed
      Examples:
        | state               |
        | InProgress          |
        | CompletionRequested |
        | Completed           |

    @DSP_001 @authorization
    Scenario Outline: Non-principals cannot open a case
      Given a current rejected submission
      When the <actor> opens a dispute against it
      Then the request is forbidden and domain state is unchanged
      Examples:
        | actor                  |
        | assigned worker        |
        | unrelated account      |
        | system authority       |
        | unassigned coordinator |

    @DSP_001 @SUB_002
    Scenario: A submission from another milestone cannot be used
      Given two milestones each have a rejected submission
      When an opening or review pairs one milestone with the other's submission ID
      Then the provider rejects the inconsistent resource references

  Rule: Opening freezes the whole project atomically

    @DSP_002 @DSP_003
    Scenario: Freeze preserves all funds and reservations without snapshots
      Given a project has multiple milestones and task storages
      When a valid dispute opens for one milestone
      Then all project mutations are blocked by its active dispute
      And balances escrows assignments scores and reservations are unchanged
      And opening context reads the frozen records without storing a snapshot

    @DSP_003
    Scenario Outline: Existing project operations cannot bypass the freeze
      Given the project has an active dispute
      When an otherwise authorized caller attempts <mutation>
      Then the command fails and frozen project state is unchanged
      And no payment score reservation change or business event occurs
      Examples:
        | mutation                    |
        | create a task               |
        | edit a task                 |
        | report task progress        |
        | edit or delete a proposal   |
        | submit or approve a proposal |
        | submit completion           |
        | accept or reject completion |
        | change planning             |
        | open a planning dispute     |
        | cancel the project          |
        | mutate another milestone    |

    @DSP_002 @DSP_006
    Scenario: Concurrent openings cannot create two active cases
      Given two valid opening commands target different rejected milestones in one project
      When they execute concurrently with distinct operation IDs
      Then exactly one opening succeeds
      And the other observes the active dispute without partial effects

    @DSP_002 @rollback
    Scenario: A storage failure cannot commit part of an opening
      Given a valid opening cannot commit its transaction
      When the command executes
      Then no partial dispute project link milestone freeze or event is persisted

    @DSP_003
    Scenario: Freeze is scoped to its project
      Given one project is disputed and another is not
      When an authorized caller edits the other project
      Then that edit succeeds
      And frozen project reservations remain unchanged

  Rule: The case is public and the response is unique

    @DSP_005
    Scenario: Anonymous reader sees the public case
      Given an Open dispute
      When a reader without a session requests the public case
      Then the case references and affected frozen context are returned
      And no keys credentials signing data receipts notification data or unrelated project is returned
      And no notification is marked read

    @DSP_005 @authorization
    Scenario: Public reading does not authorize writing
      Given an Open dispute is readable without login
      When a browser without a valid session and CSRF token posts a response
      Then no signed command is enqueued

    @DSP_004
    Scenario: Counterparty responds once
      Given an Open dispute with no response
      When its counterparty submits valid response evidence
      Then one immutable response is recorded with author and timestamp
      And the case remains Open and the project unchanged
      And a second response with another operation ID is rejected

    @DSP_004 @authorization
    Scenario Outline: No one can answer for the counterparty
      Given an Open dispute with no response
      When the <actor> attempts to respond
      Then the response is forbidden
      Examples:
        | actor             |
        | opener            |
        | assigned worker   |
        | system authority  |
        | unrelated account |

    @DSP_005 @DSP_007
    Scenario: The PoC ends without chat or resolution
      Given an Open dispute with a counterparty response
      When a principal or administrator tries to resolve unlock or modify it
      Then no supported operation can resolve or unlock it
      And no communication channel is created
      And the project and its pending funds remain frozen

  Rule: Real signed delivery and retained state preserve the result

    @DSP_006 @realtime
    Scenario: Counterparty reconnects to notifications
      Given opening and response committed while the counterparty was disconnected
      When it reconnects through authenticated SSE using its last cursor
      Then the committed events can be received without losing notification state

    @DSP_006 @idempotency
    Scenario Outline: Lost responses do not duplicate business effects
      Given a signed <command> committed but its response was lost
      When the exact operation is retried
      Then the original receipt is returned without duplicate effects
      Examples:
        | command   |
        | rejection |
        | opening   |
        | response  |
        | acceptance |

    @DSP_006 @storage
    Scenario Outline: Supported backends preserve the same business semantics
      Given the mock uses <backend>
      When signed submission rejection opening and response complete
      Then the case freeze balances and reservations satisfy the same invariants
      Examples:
        | backend |
        | memory  |
        | sqlite  |

    @DSP_002 @DSP_003 @DSP_004 @storage
    Scenario: Retained state restores the frozen case
      Given SQLite committed an opening and response
      When the mock restarts with the same retained compatible state
      Then the case response project freeze and receipts remain valid
      And another response or project mutation remains rejected

    @SUB_001 @SUB_002 @REJ_001 @REJ_002 @DSP_001 @DSP_002 @DSP_003 @DSP_004 @DSP_005 @DSP_006 @DSP_007 @e2e
    Scenario: Complete public API dispute flow with real custody and mock
      Given disposable adapter custody and mock processes are running
      And public API setup created a funded project with two milestones and workers
      When the coordinator submits and the client rejects a delivery
      And resubmission and rejection create a new current rejected delivery
      And a principal opens a dispute and the counterparty responds
      Then the anonymous public case is available and authenticated SSE reports it
      And replay does not duplicate it and forbidden actors cannot mutate it
      And both milestone storages escrows balances and reservations remain frozen
      And the final state is Open without an unlock operation
