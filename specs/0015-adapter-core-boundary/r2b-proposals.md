# R2b1: proposal content ownership

Implementation slice of approved SPEC-0015 R2. Catalog names/moderation and
cancellation/dispute reason text are the following R2b2 slice; task ownership is
R3. This slice must finish as a working native/WASM/API cohort, with exact-state
rollback, before acceptance. No release, live migration or deployment is implied.

The adapter owns exact proposal titles/descriptions, milestone titles and the
latest change-request explanation in one immutable `proposal_documents` context.
Definition and change request have independent fixed references; editing a
proposal does not erase a previous request. Existing presentation/comments remain
in their present context tables. API staging and the queued compact command commit
in the existing operation transaction, retaining the original public request for
idempotency. No extra service, generic document store, dependency or public limit.

The signed document binds generation, Project, operation, verified author and
explicit target proposal (unknown until creation succeeds). It contains the exact
original definition, including numeric quoted inputs, or exact change-request
text. Migration identifies the source file hash and retained proposal without
inventing a signer. Core receives real numeric milestone terms and a fixed
operation/hash commitment, never descriptive text. The core still checks keys,
qualifications, amounts, windows, permissions, lifecycle and captured allocation.

Create assigns revision 0. Update retains its existing permission/order behavior
and captures the compensation policy during execution. The API never predicts
revision/policy or allocates payouts from its snapshot. Definition refs and actual
terms/policy are committed atomically. SetDelivery, Submit and Withdraw may advance
revision while retaining the selected immutable document. Reads verify context,
digest, definition keys and input terms reallocated under the captured policy;
migrated terms are already allocated and compare directly. Mutable tasks,
assignments, submissions and milestone status are not part of that comparison.
Public views continue to use the core's current facts and revision.

Legacy full-text calls lower only after replay admission and retain their exact
recorded bytes, signatures, inputs and outcomes. A working V2 predecessor bridge
must decode/replay compact commands 51–53 but reject fresh execution without a
receipt/nonce change. Freeze that executable before the V3 cutover. Conversion is
explicit from that immediate predecessor, through owner exports and fresh stores;
current-state rollback rehydrates V2 through API-owned content, including successful
compact writes. Keep selected and historical content, and reject missing,
conflicting, foreign or corrupt records. Reconcile pending/ambiguous work before
migration; never rewrite signed commands. Core money/lifecycle operations and
standalone task reads must not depend on unrelated descriptive content.

Final acceptance: unchanged public DTOs/REST/SSE and IDs, exact native/WASM
financial and history parity, revision/policy races, authorization and missing or
corrupt content, PostgreSQL transactions/recovery, sealed imports, restart,
failed-commit recovery, and real frozen-predecessor rollback after successful
compact create/update/change request and further ordinary work. Run the app/SDK
and unchanged SDK policy gates; rebuild changed v5 source artifacts. Astra xhigh
reviews the exact milestone. Do not stop at the decoder bridge as the deliverable.
