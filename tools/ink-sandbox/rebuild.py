#!/usr/bin/env python3
"""Rebuild pinned v5 fixtures from a committed contracts checkout; publish nothing."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="local checkout of the canonical contracts repository")
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--contract", action="append", help="rebuild only a named fixture; repeat to select several")
    args = parser.parse_args()
    manifest = json.loads((HERE / "fixtures/manifest.json").read_text())
    version = subprocess.check_output(["cargo", "contract", "--version"], text=True)
    if not version.startswith("cargo-contract-contract 5.0.3-"):
        raise SystemExit("This bundle requires cargo-contract 5.0.3")
    with tempfile.TemporaryDirectory(prefix="ink-v5-rebuild-") as temporary:
        directory = Path(temporary)
        archive = directory / "source.tar"
        with archive.open("wb") as target:
            subprocess.run(["git", "-C", str(args.source.resolve()), "archive", manifest["sourceCommit"]], stdout=target, check=True)
        with tarfile.open(archive) as source:
            source.extractall(directory / "source", filter="data")
        if args.contract and set(args.contract) - manifest["contracts"].keys():
            raise SystemExit("Unknown contract fixture")
        for name, entry in manifest["contracts"].items():
            if args.contract and name not in args.contract:
                continue
            lock = HERE / "fixtures/build-locks" / f"{name}.lock"
            if digest(lock) != entry["lockSha256"]:
                raise SystemExit(f"{name}: build lock mismatch")
            contract = directory / "source" / name
            shutil.copyfile(lock, contract / "Cargo.lock")
            command = ["cargo", "+1.88.0", "contract", "build", "--release", "--manifest-path", str(contract / "Cargo.toml")]
            if args.offline:
                command.append("--offline")
            # Never reuse an application's target directory for the contract toolchain.
            environment = dict(os.environ)
            environment.pop("CARGO_TARGET_DIR", None)
            subprocess.run(command, env=environment, check=True)
            if digest(contract / "Cargo.lock") != entry["lockSha256"]:
                raise SystemExit(f"{name}: build changed the pinned lockfile")
            artifact_name = name.replace("-", "_")
            built = json.loads((contract / "target/ink" / f"{artifact_name}.contract").read_text())
            pinned = json.loads((HERE / "fixtures" / f"{name}.contract").read_text())
            if built["source"]["hash"] != entry["wasmBlake2b256"]:
                raise SystemExit(f"{name}: WASM differs from the pinned bundle")
            for key in ["spec", "types", "storage", "version"]:
                if built[key] != pinned[key]:
                    raise SystemExit(f"{name}: {key} metadata differs")
            print(f"{name}: pinned WASM and ABI reproduced", flush=True)


if __name__ == "__main__":
    main()
