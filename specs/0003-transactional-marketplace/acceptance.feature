@SPEC_0003
Feature: Negotiate planning and execute a transactionally funded proposal

  @PLAN_001 @MONEY_002
  Scenario: Accept a planning quote
    Given a coordinator quoted planning and the client can fund it
    When the client accepts the quote
    Then the planning fee is locked
    And planning hours are reserved in the same transaction
    And no planning fee is paid yet

  @PLAN_003 @PLAN_004
  Scenario: Pay for delivered planning without hiring execution
    Given the coordinator delivered the agreed plan
    When the client accepts planning delivery
    Then the planning fee is paid once
    And execution remains unapproved

  @CAL_002
  Scenario: Preserve commitments when changing capacity
    Given a worker has committed hours in a week
    When the worker changes default capacity or a weekly override
    Then existing reservations remain
    And a change below committed capacity is rejected

  @MATCH_002 @DOM_002
  Scenario: Competing clients cannot overbook one worker
    Given two proposals compete for the same remaining weekly hours
    When both clients approve concurrently
    Then at most one incompatible reservation succeeds
    And a rejected approval changes no balance, assignment, or reservation

  @MATCH_001
  Scenario: Match all skills without requiring the role
    Given a worker has every required skill and enough capacity
    And the worker's role differs from the requirement's role
    When eligible workers are selected
    Then that role difference does not exclude the worker

  @MILE_001 @MONEY_002 @SCORE_002
  Scenario: Accept a milestone exactly once
    Given the coordinator requested milestone completion with individual scores
    When the client accepts completion and rates the team and coordinator
    Then quoted payouts and weighted scores commit together
    And repeating the same operation repeats neither payouts nor scores

  @MONEY_003
  Scenario: Freeze unresolved funds
    Given a project has unreleased escrow
    When it is cancelled or the relevant work is disputed
    Then unresolved funds remain frozen
    And no automatic refund or payout occurs

  @MONEY_003A
  Scenario Outline: Only the project parties may freeze disputed work
    Given a project has unreleased escrow
    When the <actor> requests cancellation or opens a planning or milestone dispute
    Then the request is <result>
    And no settled payment is reversed

    Examples:
      | actor                | result   |
      | client               | accepted |
      | assigned coordinator | accepted |
      | assigned worker      | rejected |
      | unrelated principal  | rejected |

  @TASK_002
  Scenario: Tracking is not contractual reassignment
    Given a task exists in a milestone storage
    When the coordinator changes its tracking assignees
    Then contractual assignments and reserved hours do not change

  @DOM_003
  Scenario Outline: Storage backends have identical transaction semantics
    Given the mock uses <backend> storage
    When execution fails after tentative business changes
    Then no partial business changes commit

    Examples:
      | backend |
      | memory  |
      | sqlite  |
