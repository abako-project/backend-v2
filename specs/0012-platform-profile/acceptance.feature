Feature: Public platform profile
  Visitors can learn the deployment's branding and vocabulary before login.

  Scenario: Read the default software profile
    Given the adapter uses the bundled software profile
    When an anonymous visitor requests GET /api/platform
    Then the response is 200 application/json
    And its object has the agreed platform profile shape
    And its name is Abako
    And no signing or provider execution is performed

  Scenario: Configure another domain
    Given the operator supplies a valid alternative platform JSON file
    When the adapter starts and a visitor requests GET /api/platform
    Then the response contains that profile's branding and terminology
    And existing balances and reservations are unchanged

  Scenario: Refuse invalid explicit configuration
    Given PLATFORM_CONFIG_PATH selects an invalid or oversized file
    When the adapter starts
    Then startup fails without serving a fallback profile
    And the error does not include the file's contents

  Scenario: Preserve Origin restrictions
    Given a browser sends an Origin outside the configured allowlist
    When it requests GET /api/platform
    Then the response is 403

  Scenario: Preserve current mock amount units
    When a visitor reads the bundled platform profile
    Then currency decimals is 0
    And existing integer money responses are unchanged

  Scenario: Save configured and legacy project types
    Given an authorized client has a project
    Then its brief accepts each configured project type and each legacy value
    And an unconfigured project type is rejected
    And previously saved types remain readable after configuration changes
