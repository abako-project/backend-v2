@spec_SPEC_0001
Feature: Custodial wallet signing
  A person using classic login can cause authentic provider operations without handling a private key.

  Rule: Each classic principal has one custodial identity

    @REQ_001 @happy_path
    Scenario: Provision a wallet for a new principal
      Given a registered classic principal without a wallet
      When wallet provisioning succeeds
      Then the principal has one active sr25519 wallet
      And the returned identity is its AccountId32
      And no private key material is returned

    @REQ_001 @idempotency
    Scenario: Repeat wallet provisioning
      Given a classic principal already has a custodial wallet
      When wallet provisioning is requested again for that principal
      Then the existing wallet and AccountId32 are returned
      And no additional wallet is created

    @REQ_014 @credentials
    Scenario: Change credentials without changing wallet identity
      Given a classic principal has an active custodial wallet
      When the principal's password is changed
      Then the principal keeps the same AccountId32
      And the custodial signing seed is not replaced

  Rule: An authenticated application action authorizes its provider signature

    @REQ_003 @happy_path
    Scenario: Authorize signing through a valid session
      Given an authenticated principal is allowed to perform an application action
      When the principal performs that action
      Then one immutable provider operation is accepted
      And no additional wallet confirmation is requested

    @REQ_003 @rejection
    Scenario: Reject an unauthorized action before signing
      Given a principal is not allowed to perform an application action
      When the principal attempts that action
      Then the action is rejected
      And no signing job is created

    @REQ_013 @security
    Scenario: Do not offer arbitrary signing to a browser
      Given an authenticated browser session
      When the browser attempts to submit arbitrary bytes for signing
      Then no browser operation accepts those bytes
      And no signing job is created

  Rule: Signing jobs are durable and idempotent

    @REQ_011 @security
    Scenario: Reject an unauthenticated custody caller
      Given an internal caller has no valid custody service credential
      When it attempts to create a signing job
      Then custody rejects the request
      And no signing job is created

    @REQ_004 @idempotency
    Scenario: Repeat an identical signing request
      Given a signing job exists for an operation and payload
      When the same operation and payload are requested again
      Then the existing signing job is returned
      And no second signing job is created

    @REQ_004 @rejection
    Scenario: Reject an operation identifier collision
      Given a signing job exists for an operation and payload
      When the same operation is requested with different content
      Then the request is rejected as an idempotency conflict
      And the original signing job remains unchanged

    @REQ_009 @recovery
    Scenario: Recover a signing job after a worker stops
      Given a worker lease expires before a pending signing job is completed
      When another custody worker polls eligible work
      Then that worker can claim the signing job
      And the job can reach one terminal outcome

  Rule: Only active custodial wallets sign

    @REQ_010 @rejection
    Scenario Outline: Reject signing by an inactive wallet
      Given a wallet is <lifecycle>
      And a pending signing job references that wallet
      When custody evaluates the signing job
      Then the signing job is rejected
      And no signature is returned

      Examples:
        | lifecycle |
        | Suspended |
        | Retired   |

  Rule: The mock executes only valid fresh calls

    @REQ_007 @REQ_008 @happy_path
    Scenario: Execute a valid signed call
      Given an active wallet signed a call with its next nonce and a future expiration
      When the provider submits the unchanged signed call to the mock
      Then the addressed contract message is executed once
      And the next nonce, receipt, and durable event are committed with the state change

    @REQ_007 @REQ_008 @security
    Scenario Outline: Reject a manipulated signed call
      Given a valid signed call
      When its <field> is changed without a new valid signature
      Then the mock rejects the call
      And no provider state, nonce, receipt, or event is changed

      Examples:
        | field             |
        | payload version   |
        | origin            |
        | contract instance |
        | message           |
        | nonce             |
        | expiration        |
        | payload           |

    @REQ_008 @rejection
    Scenario: Reject an expired call
      Given a correctly signed call whose expiration has passed
      When the provider submits the call
      Then the mock rejects it as expired
      And no provider state, nonce, receipt, or event is changed

    @REQ_008 @idempotency
    Scenario: Repeat an exactly finalized call
      Given a signed call has already been finalized by the mock
      When the identical signed bytes are submitted again
      Then the previous receipt is returned
      And the contract state changes only once

    @REQ_006 @concurrency
    Scenario: Preserve operation order for one wallet
      Given one wallet has two authorized operations in order
      When workers process both operations concurrently
      Then the second operation is not signed or submitted before the first leaves its critical section
      And each accepted call uses the next account nonce

    @REQ_006 @concurrency
    Scenario: Make progress for different wallets
      Given two wallets each have an authorized operation
      When workers process both operations
      Then neither wallet waits for the other wallet's critical section

  Rule: Execution remains safe across preparation failures and provider resets

    @REQ_007 @REQ_008 @instance_isolation
    Scenario: Reject a signature from a previous disposable provider instance
      Given a correctly signed call for a previous provider instance
      When the call is submitted to a freshly reset mock
      Then no contract state changes

    @REQ_008 @expired_replay
    Scenario: Recover a successful receipt after call expiration
      Given a signed call was executed and its reply was lost
      And the call has since expired
      When the identical call is authenticated and submitted again
      Then the original receipt is returned
      And no business effect is repeated

    @REQ_009 @unknown_outcome
    Scenario: Preserve uncertainty after submission retries are exhausted
      Given a provider may have executed an operation
      And its response cannot be retrieved
      When the submission retry budget is exhausted
      Then the operation is OutcomeUnknown
      And no new business attempt is created automatically

    @REQ_008 @side_effect_free_preparation
    Scenario: Preparing bytes does not execute a command
      Given an authorized application action
      When its signing payload is prepared but signing fails
      Then no provider business state changes

  Rule: Secrets do not cross the custody boundary

    @REQ_002 @REQ_012 @security
    Scenario: Complete a signed operation without disclosing secrets
      Given a seeded wallet and diagnostic capture are enabled for the test
      When an authorized operation is signed and finalized
      Then no seed, master key, or service token appears in an API response
      And no seed, master key, or service token appears in captured logs, traces, metrics, or errors
