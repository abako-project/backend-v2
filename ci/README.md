# CI and Delivery Gates

Pull requests normally run formatting, check, Clippy, Nextest, documentation tests, contract compatibility, migration checks, dependency policy, and container validation relevant to changed paths.

Nightly or scheduled jobs may run Miri subsets, Loom models, fuzzing, slow integration suites, and broader feature matrices.

Release jobs produce immutable artifacts, SBOM, scans, provenance, and signatures where supported. Deployment jobs promote the exact reviewed artifact through protected environments.
