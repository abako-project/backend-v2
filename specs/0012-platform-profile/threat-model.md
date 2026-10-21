# Threat model

All returned fields are public. The profile must contain no credentials,
wallet material, personal contact data or internal service configuration.
Typed deserialization with unknown-field rejection prevents accidentally
publishing additional operator configuration.

Only the deployment operator controls the file. No public write route exists.
Bound file size, strings and lists. Reject invalid explicit configuration at
startup. Do not echo file contents in errors or logs.

Frontend consumers render text as text, map icon identifiers to their icon
library and do not execute HTML or scripts from the profile. Existing exact
Origin/CORS behavior stays in force.

Test anonymous access, forbidden Origin, field whitelist and malformed input.
