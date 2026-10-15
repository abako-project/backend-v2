Feature: Deployed backend for frontend integration
  Scenario: HTTPS API without a frontend
    Given the existing stack is running
    When the Digesta backend preview is deployed
    Then its OpenAPI schema is available through the selected existing HTTPS site
    And the existing stack remains available
    And no frontend deployment is required

  Scenario: Credentialed authenticated operations
    Given a tester has a secure Digesta session
    When an authenticated mutation supplies its CSRF token and idempotency key
    Then its provider operation can complete
    And missing CSRF or disallowed origins are rejected

  Scenario: Private services and recoverable state
    Given the preview has persistent adapter, wallet and mock state
    When a coherent backup is restored to isolated stores
    Then the restored preview preserves its identities and operations
    And custody, mock and PostgreSQL have no public host ports
