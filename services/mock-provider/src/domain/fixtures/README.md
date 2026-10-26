# Signed regression fixtures

Files without the `-v5` suffix retain their original signed bytes and expected
history. The task-ownership migration tests use the V4 archives in
`migration-signed.json`; do not replace them with newly generated current state.
Here V5 names the private core archive version, not a newer ink! version.

The `-v5` project and financial traces exercise compact task creation and
assignment. Legacy progress requests are explicit rejection probes; successful
text/progress edits and their permission/freeze rules are tested by the adapter.
The native financial test also compares final balances, holds, worker calendars
and exact reputation with the original frozen financial outcome.

`migration-signed-v5.json` contains checkpoints and continuations from the same
V5 financial trace. Its step numbers must never be paired with a V4 archive.
`evidence-signed-v5.json` and `qualification-capacity-signed-v5.json` retain the
corresponding current signed evidence/capacity scenarios. Normal verification
only reads these files; it never regenerates expected results.

Authoring uses the existing explicit Rust tests, with output paths under an SSD
scratch directory. Generate project/financial traces first, then migration, then
qualification capacity and evidence. Review the resulting commands and financial
assertions before copying an output into this directory:

- `export_project_trace`: `PROJECT_SIGNED_TRACE_OUTPUT`
- `export_financial_trace`: `FINANCIAL_SIGNED_TRACE_OUTPUT`
- `export_migration_reference`: `MIGRATION_REFERENCE_OUTPUT`
- `export_ordinary_qualification_capacity`: `QUALIFICATION_CAPACITY_REFERENCE`
- `committed_evidence_preserves_receipts_money_replay_and_current_archives`:
  `EVIDENCE_FIXTURE_OUTPUT`

The four export tests require `--ignored`. The evidence test runs normally and
only signs replacements when its explicit output variable is set. Run authoring
filters separately; unrelated generators require their own output variables.
