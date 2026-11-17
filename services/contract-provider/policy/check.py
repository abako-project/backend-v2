#!/usr/bin/env python3
"""Check the owner-approved SDK policy against only its reviewed locked graph."""
import datetime
import hashlib
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
EXPECTED_LOCK = "494304d6d83c21e93c05fa1f7f8ee14dc98738d0e07716a9011da4af7493ab67"
REVIEW_BY = datetime.date(2026, 12, 1)
if hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest() != EXPECTED_LOCK:
    sys.exit("SDK policy needs a new review: Cargo.lock changed.")
if datetime.date.today() >= REVIEW_BY:
    sys.exit("SDK policy expired; review the current graph before renewing it.")
sys.exit(subprocess.run([
    "cargo", "deny", "--manifest-path", str(ROOT / "Cargo.toml"), "--locked",
    "--config", str(ROOT / "policy/deny.sdk.toml"),
    "check", "--hide-inclusion-graph",
], cwd=ROOT.parents[1], check=False).returncode)
