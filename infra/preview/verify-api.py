#!/usr/bin/env python3
"""Verify a deployed preview without printing credentials or session tokens."""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import secrets
import sys
import urllib.parse


def load_client():
    helper = Path(__file__).with_name("poc-e2e.py")
    if not helper.exists():
        helper = Path(__file__).resolve().parents[2] / "scripts/poc-e2e.py"
    spec = importlib.util.spec_from_file_location("preview_poc", helper)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def verify(base, origin, identity_file, internal_http=False):
    parsed = urllib.parse.urlparse(base)
    if internal_http:
        if parsed.scheme != "http" or parsed.hostname != "127.0.0.1":
            raise ValueError("internal HTTP checks require loopback")
    elif parsed.scheme != "https":
        raise ValueError("public verification requires HTTPS")
    poc = load_client()
    anonymous = poc.Client(base, origin)
    schema = anonymous.request("GET", "/api/openapi.json")
    poc.require(schema["info"]["title"] == "Digesta public REST API", "wrong schema")
    poc.require(schema["components"]["securitySchemes"]["sessionCookie"]["name"] == "digesta_session", "wrong cookie contract")
    anonymous.request("GET", "/api/auth/session", expected=401)
    anonymous.request("GET", "/api/catalog", expected=403,
                      headers={"Origin": "https://untrusted.invalid"})
    anonymous.request("OPTIONS", "/api/workers", expected=204,
                      headers={"Access-Control-Request-Method": "POST",
                               "Access-Control-Request-Headers": "content-type,x-csrf-token,idempotency-key"})
    if not internal_http:
        for path in ["/api/internal", "/api/internal/signing-jobs", "/api/health",
                     "/api/ready", "/api/%69nternal/signing-jobs"]:
            anonymous.request("GET", path, expected=404)
    print("PASS: OpenAPI, anonymous authentication denial, origin and private API boundaries")

    actor = poc.Client(base, origin)
    identity_file = Path(identity_file)
    if identity_file.exists():
        identity = json.loads(identity_file.read_text())
        actor.authenticate("/api/auth/login", {
            "username": identity["username"], "password": identity["password"],
        })
        poc.require(actor.session["accountId"] == identity["accountId"], "restored identity changed")
    else:
        identity = {"username": "preview_smoke_" + secrets.token_hex(6),
                    "password": secrets.token_urlsafe(32)}
        actor.authenticate("/api/auth/register", {
            **identity, "displayName": "Deployment verification",
        })
        identity["accountId"] = actor.session["accountId"]
    poc.require(not actor.session["isAdmin"], "ordinary tester became administrator")
    cookie = next(cookie for cookie in actor.cookies if cookie.name == "digesta_session")
    poc.require(cookie.secure and cookie.path == "/api", "cookie security attributes changed")
    poc.require(cookie.has_nonstandard_attr("HttpOnly") and
                cookie.get_nonstandard_attr("SameSite") == "Lax", "cookie scope changed")
    if internal_http:
        # Loopback restore checks exercise persistence; public HTTPS tests retain Secure.
        cookie.secure = False
    actor.request("GET", "/api/auth/session")
    print("PASS: registration/login, stable account, secure HttpOnly SameSite=Lax session")

    catalog = actor.request("GET", "/api/catalog")
    roles = [role["id"] for role in catalog["roles"] if not role["fixed"]]
    skills = [skill["id"] for skill in catalog["skills"]]
    poc.require(roles and skills, "catalog seed missing")
    payload = {"displayName": "Deployment verification",
               "qualifications": {"roleIds": [roles[0]], "skillIds": [skills[0]]},
               "calendar": {"defaultWeeklyMinutes": 600, "overrides": []}}
    actor.request("POST", "/api/workers", payload, expected=403,
                  headers={"X-CSRF-Token": "", "Idempotency-Key": "0x" + secrets.token_hex(16)})
    if "operationId" not in identity:
        operation = actor.command("POST", "/api/workers", payload)
        identity["operationId"] = operation["operationId"]
    else:
        operation = actor.request("GET", "/api/operations/" + identity["operationId"])
        poc.require(operation["status"] == "Finalized" and
                    operation["receipt"]["outcome"]["type"] == "Success",
                    "restored operation missing")
    workers = actor.request("GET", "/api/workers")
    poc.require(any(worker["account"] == identity["accountId"] for worker in workers),
                "signed worker state missing")
    fresh = actor.command("PUT", "/api/workers/me/qualifications",
                          {"qualifications": payload["qualifications"]})
    identity["lastVerifiedOperationId"] = fresh["operationId"]
    print("PASS: CSRF denial, signed provider operation completion and persisted worker")

    first = actor.first_sse_after(0)
    replay = actor.first_sse_after(0)
    poc.require(first["notificationId"] == replay["notificationId"] and
                first["readAt"] == replay["readAt"], "SSE replay changed notification state")
    print("PASS: authenticated SSE reconnect/replay through the public proxy")
    temporary = identity_file.with_name(identity_file.name + ".new")
    with os.fdopen(os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w") as output:
        json.dump(identity, output)
    os.replace(temporary, identity_file)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--origin", required=True)
    parser.add_argument("--identity-file", type=Path, required=True)
    parser.add_argument("--internal-http", action="store_true")
    args = parser.parse_args()
    try:
        verify(args.base_url.rstrip("/"), args.origin, args.identity_file, args.internal_http)
    except Exception as error:
        # Known assertion messages contain route/status codes, never response bodies.
        detail = str(error) if isinstance(error, AssertionError) else type(error).__name__
        sys.exit("Preview verification failed: " + detail)
