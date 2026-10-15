Feature: Digesta project identity
  Scenario: Version-tagged backend images
    Given a version tag points to a commit on master
    When release verification succeeds
    Then backend images have digesta-prefixed names under the repository owner's GHCR namespace

  Scenario: Browser configuration and authentication
    Given the frontend uses DIGESTA_API_BASE
    When a user logs in
    Then the adapter sets digesta_session with the existing security attributes
    And authenticated API requests accept that cookie

  Scenario: Existing encrypted and signed state
    Given wallet ciphertext and signed calls created before the rename
    When the renamed services read them
    Then cryptographic format bytes remain unchanged
