# Proposal metadata

Approved ownership and encoding rules under SPEC-0015.

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

## Encoding

The SHA-256 preimage is literal `abako/proposal-document/v1`, one NUL byte, then
SCALE generation, Project ID, provenance and content. Operation provenance binds
operation ID, verified author and optional target proposal (None for creation).
Migration provenance binds the exact source archive SHA-256 and retained proposal.
Definition content contains the original quoted inputs and narrative; change
content contains the exact explanation. The data-only codec and five independent
vectors are in `proposal-metadata-vectors.json`. Signed references are 49 bytes;
Migration references are 33 bytes. Ordinary execution receives only the fixed
48-byte commitment and numeric terms; migration refs enter through sealed import.
No URL parser, descriptive proposal text or API document store is added onchain.


## Recovery

Use the current [paired owner procedure](../../docs/operations/task-cutover.md) for
task ownership and preserve matching API/core backups. Earlier metadata converter
steps remain in [the frozen instructions](https://github.com/abako-project/backend-v2/blob/1964a56d88bc31f652af7151841023c6aa53e896/specs/0015-adapter-core-boundary/r2b-proposals.md).
Never reinterpret a signed format or substitute a stale checkpoint after writes.
