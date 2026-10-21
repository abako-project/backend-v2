# Data ownership and model

The adapter owns one public profile per deployment. Operator configuration is
the source of truth. There are no database tables, migrations or tenant IDs
on existing records.

The profile contains identity/branding strings; a currency object containing
symbol, name and integer decimals; an ordered list of project-type strings;
and the complete terminology object supplied by the owner. External-profile
hints contain id, label, placeholder, icon and helpText.

The `id` identifies the configured profile. It does not grant access or select
a database partition. This first release does not claim multi-tenant support.
No currency conversion or amount migration follows from loading this object.

Project briefs already store JSONB, so projectType becomes a bounded string
without a table migration. HTTP writes check the configured types plus legacy
values. Reads validate structure without consulting the current allowlist,
preserving historical records. Older enum-only binaries cannot read newly
saved custom labels; review rollback compatibility before deploying.
