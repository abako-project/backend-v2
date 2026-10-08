#!/usr/bin/env python3
"""Create the disposable development actors through the current public API.

Run after starting the backend. Existing actors are checked and reused; balances
below 1000 KVN are topped up through simulated Bramp/operator confirmation.
"""
import argparse
import importlib.util
from pathlib import Path

spec = importlib.util.spec_from_file_location("poc_e2e", Path(__file__).with_name("poc-e2e.py"))
poc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(poc)


def seed(base, admin_password):
    admin = poc.Client(base)
    admin.authenticate("/api/auth/login", {"username": "admin", "password": admin_password})
    poc.require(admin.session["isAdmin"], "administrator required")
    catalog = admin.request("GET", "/api/catalog")
    roles = [role["id"] for role in catalog["roles"] if not role["fixed"]]
    skills = [skill["id"] for skill in catalog["skills"]]
    poc.require(roles and skills, "catalog seed is missing")
    workers = {worker["account"]: worker for worker in admin.request("GET", "/api/workers")}
    accounts = []
    for kind, count in (("worker", 10), ("coordinator", 3), ("client", 3)):
        for index in range(1, count + 1):
            username, name = f"{kind}{index}", f"{kind.title()} {index}"
            credentials = {"username": username, "password": f"{name} 1234!"}
            actor = poc.Client(base)
            session = actor.request("POST", "/api/auth/register", {**credentials, "displayName": name}, (201, 409))
            if "principalId" in session:
                actor.session = session
            else:
                actor.authenticate("/api/auth/login", credentials)
            account = actor.session["accountId"]
            poc.require(not actor.session["isAdmin"], "fixture unexpectedly has administrator authority")
            if kind == "client":
                profile = {"name": name, "company": f"Company {index}", "department": None,
                           "website": None, "description": "Development test client", "location": "Spain", "languages": ["eng", "spa"]}
                section = "client"
            else:
                profile = {"name": name, "contactEmail": f"{username}@example.test", "githubUsername": None,
                           "portfolioUrl": None, "biography": "Development test worker", "background": None,
                           "proficiency": "senior", "location": "Spain", "languages": ["eng", "spa"]}
                section = "worker"
            saved = actor.request("PUT", "/api/profiles/me", {"section": section, "profile": profile})
            poc.require(saved[section]["name"] == name, "profile was not persisted")
            if kind != "client":
                if account not in workers:
                    actor.command("POST", "/api/workers", {"displayName": name,
                        "qualifications": {"roleIds": roles, "skillIds": skills},
                        "calendar": {"defaultWeeklyMinutes": 2400, "overrides": []}})
                if kind == "coordinator":
                    if not workers.get(account, {}).get("coordinatorEligible"):
                        admin.command("POST", "/api/admin/coordinators", {"account": account})
                    if workers.get(account, {}).get("mode") != "Coordinator":
                        actor.command("PUT", "/api/workers/me/mode", {"mode": "Coordinator"})
            available = int(actor.request("GET", "/api/balance")["available"])
            if available < 1000:
                deposit = actor.command("POST", "/api/bramp/deposits", {"amount": str(1000 - available)})
                admin.command("POST", "/api/admin/bramp/deposits/" + deposit["receipt"]["createdEntityId"] + "/confirm")
            poc.require(int(actor.request("GET", "/api/balance")["available"]) >= 1000, "fixture balance missing")
            accounts.append(account)
            print(f"Ready: {username} ({name})")
    directory = admin.request("GET", "/api/workers")
    fixtures = [worker for worker in directory if worker["account"] in accounts]
    poc.require(len(set(accounts)) == 16 and len(fixtures) == 13, "wrong fixture actor count")
    poc.require(sum(worker["mode"] == "Coordinator" and worker["coordinatorEligible"] for worker in fixtures) == 3,
                "wrong coordinator count")
    print("Verified: 10 workers, 3 coordinators, 3 clients; at least 1000 mock KVN each.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", default="http://localhost:8088")
    parser.add_argument("--admin-password-file", type=Path, required=True)
    args = parser.parse_args()
    seed(args.base_url, args.admin_password_file.read_text().strip())
