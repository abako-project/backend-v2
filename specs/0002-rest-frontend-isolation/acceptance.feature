@SPEC_0002
Feature: Independent frontends share the REST backend

  @REST_001 @REST_002
  Scenario: Use the backend from a non-Leptos client
    Given a client implements the documented REST contract
    When it authenticates and requests an allowed resource
    Then it receives the same response contract as Leptos
    And no Leptos-specific protocol is required

  @REST_004
  Scenario: Track a command without a live event stream
    Given an authenticated client has submitted a command
    When it queries the returned operation reference
    Then it receives the recorded execution status
    And an unknown outcome is not reported as a rejection

  @REST_005
  Scenario: Reject an unapproved browser origin
    Given a browser origin is not in the configured allowlist
    When it attempts a credentialed mutation
    Then the mutation is rejected

  @REST_006
  Scenario: Replay notifications without marking them read
    Given an authenticated client reconnects with its previous cursor
    When persisted notifications are replayed
    Then the client receives only its authorized notifications
    And notification read state is unchanged

  @REST_008
  Scenario: Keep internal services behind the deployment boundary
    Given Nginx exposes the frontend and public API
    When a browser requests an internal custody or mock endpoint
    Then Nginx does not forward the request to an internal service
