Feature: Mock auxiliary interfaces

  @VIR-AUTH-01
  Scenario: A passkey signs in to the existing custodial account
    Given a principal has a password session and custodial wallet
    When the principal registers a verified passkey after password reauthentication
    And signs in with the passkey and email
    Then the new session identifies the same principal and wallet

  @VIR-AUTH-02
  Scenario: A replayed or foreign passkey proof is rejected
    Given a challenge has expired, been used, or belongs to another principal
    When a client submits an assertion for it
    Then no session is created and no wallet operation is signed

  @VIR-RAMP-01
  Scenario: An operator confirms one simulated deposit
    Given an owner requested a KVN deposit with fixed amount and destination
    When the system operator confirms it twice using different operations
    Then the owner's free balance increases exactly once by the requested amount

  @VIR-RAMP-02
  Scenario: A withdrawal cannot spend project escrow
    Given a client has free KVN and reserved project escrow
    When the client requests a withdrawal above the free balance
    Then the withdrawal is rejected without changing either balance

  @VIR-RAMP-03
  Scenario: A pending withdrawal is cancelled
    Given a pending withdrawal holds KVN from free balance
    When its owner cancels it
    Then the held KVN returns once to free balance
