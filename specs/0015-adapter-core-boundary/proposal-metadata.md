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
text. Core receives real numeric milestone terms and a fixed
operation/hash commitment, never descriptive text. The core still checks keys,
qualifications, amounts, windows, permissions, lifecycle and captured allocation.

Create assigns revision 0. Update retains its existing permission/order behavior
and captures the compensation policy during execution. The API never predicts
revision/policy or allocates payouts from its snapshot. Definition refs and actual
terms/policy are committed atomically. SetDelivery, Submit and Withdraw may advance
revision while retaining the selected immutable document. Reads verify context,
digest, definition keys and input terms reallocated under the captured policy; Mutable tasks,
assignments, submissions and milestone status are not part of that comparison.
Public views continue to use the core's current facts and revision.

## Encoding

The SHA-256 preimage is literal `abako/proposal-document/v1`, one NUL byte, then
SCALE generation, Project ID, provenance and content. Operation provenance binds
operation ID, verified author and optional target proposal (None for creation).
Definition content contains the original quoted inputs and narrative; change
content contains the exact explanation. The data-only codec and independent signed
vectors are in `proposal-metadata-vectors.json`. Signed references are 49 bytes;
Ordinary execution receives only the fixed 48-byte commitment and numeric terms.
No URL parser, descriptive proposal text or API document store is added onchain.


## Fresh development and recovery

The owner-approved fresh-start amendment removes migration records and converters.
Contracts start through ordinary constructors. Keep matching API, wallet and
provider backups together; the core cannot reconstruct descriptive preimages.
Existing incompatible stores fail startup and are explicitly replaced for a new
development installation. Signed command indexes and current content hashing
remain unchanged.
