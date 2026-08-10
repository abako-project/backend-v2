@spec_SPEC_XXXX
Feature: Feature name
  Describe the user-visible capability and why it matters.

  Rule: A stable business rule

    @REQ_001 @happy_path
    Scenario: Successful behavior
      Given a valid initial business state
      And all required preconditions are satisfied
      When the actor performs the business action
      Then the observable result is correct
      And the relevant invariant remains true

    @REQ_001 @rejection
    Scenario: Rejected behavior
      Given a state that violates a business precondition
      When the actor attempts the business action
      Then the action is rejected with a stable public outcome
      And no partial state change is committed
