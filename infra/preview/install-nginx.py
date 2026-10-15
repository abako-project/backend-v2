#!/usr/bin/env python3
"""Validate and install only the Digesta route in the existing HTTPS site."""

import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import time


ROOT = Path("/opt/digesta/preview")
SITE = Path("/etc/nginx/sites-enabled/web.dev.abako.xyz")
MARKER = "# DEPLOY-001 Digesta API"


def block_end(text, opening):
    depth = 0
    quote = None
    comment = False
    escaped = False
    for index in range(opening, len(text)):
        char = text[index]
        if comment:
            comment = char != "\n"
            continue
        if escaped:
            escaped = False
            continue
        if char == "\\":
            escaped = True
            continue
        if quote:
            if char == quote:
                quote = None
            continue
        if char in "\"'":
            quote = char
        elif char == "#":
            comment = True
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return index
    raise ValueError("unbalanced Nginx configuration")


def install():
    target = SITE.resolve(strict=True)
    if target.parent != Path("/etc/nginx/sites-available"):
        raise ValueError("unexpected existing site location")
    original = target.read_text()
    if MARKER in original:
        raise ValueError("Digesta route already installed; use its recorded rollback/verification")
    candidates = []
    for match in re.finditer(r"(?m)^\s*server\s*\{", original):
        opening = original.index("{", match.start())
        closing = block_end(original, opening)
        body = original[opening + 1:closing]
        if re.search(r"\blisten\s+[^;]*\b443\b[^;]*;", body) and re.search(r"\bserver_name\s+web\.dev\.abako\.xyz\s*;", body):
            candidates.append((opening, closing, body))
    if len(candidates) != 1:
        raise ValueError("cannot identify exactly one approved HTTPS server")
    opening, closing, body = candidates[0]
    if re.search(r"\blocation\s+(?:\^~\s+)?/api/\s*\{", body):
        raise ValueError("existing API prefix would conflict")
    rpc = re.search(r"\blocation\s*=\s*/api/kreivo-rpc\s*\{", body)
    if not rpc:
        raise ValueError("expected existing RPC route missing")
    rpc_open = body.index("{", rpc.start())
    rpc_close = block_end(body, rpc_open)
    rpc_body = body[rpc_open + 1:rpc_close]
    if re.search(r"\bproxy_set_header\s+Cookie\b", rpc_body):
        if not re.search(r'\bproxy_set_header\s+Cookie\s+""\s*;', rpc_body):
            raise ValueError("existing RPC cookie policy needs review")
    else:
        # This stateless third-party RPC must not receive the new application session.
        body = body[:rpc_close] + '        proxy_set_header Cookie "";\n    ' + body[rpc_close:]
    snippet = (ROOT / "nginx-api.conf").read_text()
    body += "\n    " + MARKER + "\n" + "\n".join("    " + line for line in snippet.splitlines()) + "\n"
    candidate = original[:opening + 1] + body + original[closing:]
    candidate = "log_format digesta_api '$remote_addr $request_method $uri $status';\n" + candidate
    check = ROOT / "nginx-check"
    check.mkdir(mode=0o700, exist_ok=True)
    sites = check / "sites-enabled"
    sites.mkdir(mode=0o700, exist_ok=True)
    candidate_file = check / "candidate-site.conf"
    candidate_file.write_text(candidate)
    os.chmod(candidate_file, 0o600)
    for source in SITE.parent.iterdir():
        destination = sites / source.name
        if destination.exists() or destination.is_symlink():
            destination.unlink()
        destination.symlink_to(candidate_file if source == SITE else source)
    main = Path("/etc/nginx/nginx.conf").read_text()
    directive = "include /etc/nginx/sites-enabled/*;"
    if main.count(directive) != 1:
        raise ValueError("unexpected main Nginx include layout")
    main = main.replace(directive, f"include {sites}/*;")
    validation = check / "nginx.conf"
    validation.write_text(main)
    subprocess.run(["nginx", "-t", "-c", str(validation)], check=True, timeout=30)
    if target.read_text() != original:
        raise ValueError("site changed during validation")
    backup = Path("/srv/digesta/preview/backups/nginx")
    backup.mkdir(parents=True, mode=0o700, exist_ok=True)
    backup_file = backup / ("web.dev.abako.xyz-" + str(int(time.time())) + ".conf")
    shutil.copy2(target, backup_file)
    os.chmod(backup_file, 0o600)
    replacement = target.with_name(target.name + ".digesta-new")
    replacement.write_text(candidate)
    metadata = target.stat()
    os.chown(replacement, metadata.st_uid, metadata.st_gid)
    os.chmod(replacement, metadata.st_mode & 0o777)
    os.replace(replacement, target)
    try:
        subprocess.run(["nginx", "-t"], check=True, timeout=30)
        subprocess.run(["systemctl", "reload", "nginx"], check=True, timeout=30)
    except subprocess.SubprocessError:
        shutil.copy2(backup_file, target)
        subprocess.run(["nginx", "-t"], check=True, timeout=30)
        subprocess.run(["systemctl", "reload", "nginx"], check=True, timeout=30)
        raise
    print("Digesta API route installed; original site backup:", backup_file)
    print("Installed config SHA256:", hashlib.sha256(candidate.encode()).hexdigest())


if __name__ == "__main__":
    install()
