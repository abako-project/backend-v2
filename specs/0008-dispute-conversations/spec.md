# Dispute arguments and case conversation
Status: APPROVED
Approval: user instruction, 2026-10-10.

Recover the legacy written-opening modal, public case history, multiple public
Response/Additional arguments and a private client/coordinator conversation.
Published entries are append-only; no edit/delete endpoint exists. Author and
timestamp come from the authenticated session/server, never the request.

The provider remains authoritative for rejection, current submission, parties,
case creation and project freeze. It references an opening argument stored in
adapter PostgreSQL. The adapter persists descriptive texts and conversation,
scoped by provider instance. Opening text must exist before the signed operation
is enqueued. Unconfirmed opening records are never public. Idempotent retries
reuse IDs; a changed payload under an existing ID conflicts.

Public readers see the opening, formal arguments, milestone deliveries and
confirmed rejection reasons, names and public avatars. They never receive
emails, session data or private conversation messages. Only client/coordinator
may append public arguments. By default client/coordinator read the conversation,
and only the coordinator writes. DISPUTE_CHANNEL_ALLOW_PARTICIPANTS=true lets
the client, coordinator and assigned workers of the approved project read/write
the conversation. Outsiders remain excluded. GET messages returns canWrite for
the session; the frontend uses it without a second policy flag. Existing messages
remain immutable and readable after disabling expanded access.

Public history includes DisputeArgumentAdded activity from persisted public
entries, with server author/type/date. Private messages are never included.
Text accepts ordinary prose and arbitrary reference protocols, 1–4000 UTF-8
bytes, no NUL. Cursor pages contain at most 20 entries.

Resolution authority and settlement after a formal dispute are outside this phase.
The legacy verdict form may be opened but its submission stays disconnected.
No ruling, unfreeze, refund or new payment action is implemented. Existing
happy path settles each accepted milestone and closes after the last acceptance.
