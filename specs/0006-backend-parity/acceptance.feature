Feature: Backend parity without duplicate business truth

  @PAR-PROFILE-01
  Scenario: The owner edits a profile without exposing private data
    Given a client has an authenticated session
    When the client updates their descriptive profile
    Then their private read contains the updated fields
    And the public read excludes email, department and session data

  @PAR-MEDIA-01
  Scenario: A profile image is public but only its owner may replace it
    Given a client has a descriptive profile
    When the client uploads a valid PNG image
    Then the public image read returns the same bytes and PNG content type
    And an unrelated principal cannot replace that image
    And an unsupported or oversized image is rejected

  @PAR-CATALOG-01
  Scenario: A requested skill does not change qualifications until approval
    Given a worker requests a skill that is not in the catalog
    When the worker reads the catalog and qualifications
    Then the requested skill is absent from both
    When a system operator approves the request
    Then the skill and its role associations appear once in the catalog

  @PAR-STORAGE-01
  Scenario: Direct task-storage routes use existing ownership rules
    Given a coordinator owns a milestone task storage
    When the coordinator creates a task through the direct route
    Then the direct task read and project read show the same task
    And an unrelated principal cannot edit it

  @PAR-PG-01
  Scenario: Restarting an adapter preserves its operation and notification state
    Given an operation and unread notification exist in PostgreSQL
    When the adapter restarts
    Then the operation outcome and unread notification are still queryable

  @PAR-BRIEF-01
  Scenario: A client saves a project brief without changing business state
    Given an existing project and authenticated client, coordinator and assigned worker
    When the client saves its descriptive brief with expected revision zero
    Then the client and project participants can read the same ordered details
    And an outsider cannot read it or write it
    And neither the coordinator nor worker can change it
    And provider project state, balances and nonces are unchanged
    And restarting the adapter preserves the brief
    And a provider reset does not attach the brief to a reused project identifier

  @PAR-BRIEF-02
  Scenario: Concurrent brief writes cannot overwrite each other silently
    Given two adapter replicas and a saved project brief
    When different edits use the same expected revision concurrently
    Then exactly one edit succeeds and the other returns brief_revision_conflict
    And retrying the successful body with its original expected revision does not increment again
    And concurrent identical initial writes create only one saved revision
