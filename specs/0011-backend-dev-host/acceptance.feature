Feature: Correct backend preview hostname
  Scenario: Existing users use the corrected HTTPS API
    Given the existing Digesta preview has persisted accounts and operations
    When its public route is moved to dev.abako.xyz/api
    Then the same user can log in through the new HTTPS host
    And a new signed operation completes
    And secure cookies, CSRF checks and SSE replay work

  Scenario: Preserve unrelated services and data
    Given both existing HTTPS sites and legacy containers are running
    When the Digesta routing and adapter origin are updated
    Then the frontend and unrelated legacy routes remain available
    And the prior web site no longer proxies the added Digesta API
    And no secret, persistent store or volume is replaced
