@spec_SPEC_0005
Feature: Open a documented milestone dispute
  A client or assigned coordinator can preserve a dispute case after a rejected
  milestone without resolving it or moving its frozen funds.

  Background:
    Given an approved project has a milestone funded in execution escrow
    And the project has one client and one assigned coordinator

  Rule: Rejection is explicit and does not imply a dispute

    @REJ_001 @authorization
    Scenario: Client rejects a completion request with feedback
      Given the coordinator requested completion of the milestone
      When the project client rejects that completion with a non-blank reason
      Then one immutable rejection is recorded for that milestone and completion request
      And no milestone payout or reputation update occurs
      And a milestone completion rejected event is committed

    @REJ_001 @validation
    Scenario: Empty rejection reason changes nothing
      Given the coordinator requested completion of the milestone
      When the project client rejects that completion with a blank reason
      Then the command is rejected
      And no rejection, event, score or balance change is committed

    @REJ_001 @authorization
    Scenario Outline: Only the project client can reject completion
      Given the coordinator requested completion of the milestone
      When the <actor> attempts to reject that completion with a reason
      Then the request is <result>

      Examples:
        | actor                | result   |
        | project client       | accepted |
        | assigned coordinator | rejected |
        | assigned worker      | rejected |
        | unrelated account    | rejected |

    @REJ_002
    Scenario: Rejection does not automatically open a dispute
      Given the client has rejected the milestone completion
      When neither project party opens a dispute
      Then no dispute case or communication channel exists
      And the milestone funds remain unreleased in execution escrow

  Rule: Opening requires a rejection and a project principal

    @DSP_001 @happy_path
    Scenario Outline: A project principal opens from the first rejection
      Given the milestone has one recorded rejection
      When the <actor> opens a dispute with a non-blank argument
      Then one dispute is created for that project and milestone
      And the other project principal is recorded as the counterparty

      Examples:
        | actor                |
        | project client       |
        | assigned coordinator |

    @DSP_001 @rejection
    Scenario: Opening before any rejection changes nothing
      Given the milestone has no recorded rejection
      When the project client attempts to open a dispute
      Then the command is rejected
      And no dispute, channel, evidence, event or freeze is committed

    @DSP_001 @authorization
    Scenario Outline: Non-principals cannot open a dispute
      Given the milestone has one recorded rejection
      When the <actor> attempts to open a dispute with an argument
      Then the request is forbidden
      And no dispute state is committed

      Examples:
        | actor               |
        | assigned worker     |
        | unrelated account   |
        | unassigned coordinator |

    @DSP_001 @validation
    Scenario: Empty opening argument changes nothing
      Given the milestone has one recorded rejection
      When the assigned coordinator opens a dispute with a blank argument
      Then the command is rejected
      And no dispute state is committed

  Rule: The case is atomic, immutable and unresolved

    @DSP_002 @DSP_003 @DSP_005 @DSP_007
    Scenario: Opening creates one complete frozen case
      Given the milestone has one recorded rejection
      When the assigned coordinator opens a dispute with an argument
      Then the committed dispute has one opening argument
      And it has one immutable evidence snapshot ending at a fixed event cursor
      And it has one linked communication channel
      And the milestone and its unreleased funds are frozen
      And no payment, refund, score or reservation release occurs

    @DSP_002 @rollback
    Scenario: A failed evidence or channel operation rolls back opening
      Given the milestone has one recorded rejection
      And dispute opening cannot create every required case record
      When the project client attempts to open the dispute
      Then the operation fails
      And no dispute, argument, evidence, channel, event or freeze is committed

    @DSP_003 @immutability
    Scenario: Later project changes do not rewrite opening evidence
      Given a dispute captured the available project milestone task and event history
      When permitted live project data changes later
      Then the opening evidence remains byte-for-byte unchanged
      And the live project view may show the later state separately

    @DSP_004 @authorization
    Scenario: Counterparty appends an immutable response
      Given the project client opened a dispute against the assigned coordinator
      When the assigned coordinator adds a non-blank response
      Then one argument is appended with its verified author and timestamp
      And no operation can update or delete that argument

    @DSP_004 @authorization
    Scenario: Another account cannot impersonate the counterparty
      Given the project client opened a dispute against the assigned coordinator
      When an assigned worker submits a response naming the coordinator
      Then the request is forbidden
      And no argument or event is appended

    @DSP_007 @non_goal
    Scenario: The PoC cannot resolve the dispute
      Given a dispute is open and its milestone funds are frozen
      When a user inspects the available dispute operations
      Then no operation can resolve vote arbitrate pay refund penalize or unfreeze it

  Rule: Public reads and delivery preserve privacy and idempotency

    @DSP_006 @privacy
    Scenario: Public output is an allowlisted projection
      Given a dispute has private internal evidence and arguments
      When an allowed reader requests its public view
      Then only fields approved by the public visibility policy are returned
      And no credential key session receipt or unrelated project data is returned

    @DSP_008 @realtime
    Scenario: The counterparty receives the opening notification
      Given the project client opens a dispute successfully
      When the assigned coordinator resumes authenticated notifications
      Then the dispute opening event is available without changing its read state

    @DSP_009 @idempotency
    Scenario: A lost response cannot duplicate the dispute
      Given a valid signed opening commits but its HTTP response is lost
      When the same operation is submitted again
      Then the original receipt is returned
      And no second dispute argument evidence channel freeze or event is created

    @DSP_009 @storage
    Scenario Outline: Storage backends expose the same dispute semantics
      Given the mock uses <backend> storage
      When rejection opening and counterparty response succeed
      Then the committed dispute and frozen escrow are equivalent

      Examples:
        | backend |
        | memory  |
        | sqlite  |
