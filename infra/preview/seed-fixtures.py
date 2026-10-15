#!/usr/bin/env python3
"""Reuse the existing disposable actor seeder with the deployed origin."""

import argparse
import importlib.util
from pathlib import Path
import urllib.parse


def seed(base, origin, password_file, internal_http=False):
    parsed = urllib.parse.urlparse(base)
    if internal_http:
        if parsed.scheme != "http" or parsed.hostname != "127.0.0.1":
            raise ValueError("internal seeding requires loopback")
    elif parsed.scheme != "https":
        raise ValueError("public seeding requires HTTPS")
    helper = Path(__file__).with_name("seed-dev-users.py")
    if not helper.exists():
        helper = Path(__file__).resolve().parents[2] / "scripts/seed-dev-users.py"
    spec = importlib.util.spec_from_file_location("preview_seed", helper)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    original_client = module.poc.Client

    class PreviewClient(original_client):
        def __init__(self, address):
            super().__init__(address, origin)

        def request(self, *args, **kwargs):
            result = super().request(*args, **kwargs)
            if internal_http:
                for cookie in self.cookies:
                    cookie.secure = False
            return result

    module.poc.Client = PreviewClient
    module.seed(base, Path(password_file).read_text().strip())


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--origin", required=True)
    parser.add_argument("--admin-password-file", type=Path, required=True)
    parser.add_argument("--internal-http", action="store_true")
    arguments = parser.parse_args()
    seed(arguments.base_url, arguments.origin, arguments.admin_password_file, arguments.internal_http)
