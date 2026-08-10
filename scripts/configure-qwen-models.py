#!/usr/bin/env python3
"""Merge verified model providers into project-local Qwen Code settings.

The script never writes API keys. It stores only environment-variable names
and provider endpoints. Run it again after a model registry update.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent.parent
TEMPLATE = ROOT / "templates" / "qwen" / "model-providers.json.tmpl"
SETTINGS = ROOT / ".qwen" / "settings.json"

REGIONS = {
    "global": "https://coding-intl.dashscope.aliyuncs.com/v1",
    "china": "https://coding.dashscope.aliyuncs.com/v1",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Configure verified Qwen Code model providers without storing API keys."
    )
    parser.add_argument(
        "--region",
        choices=sorted(REGIONS),
        help="Alibaba Cloud Coding Plan region. Asked interactively when omitted.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=SETTINGS,
        help="Settings file to update. Defaults to .qwen/settings.json.",
    )
    return parser.parse_args()


def choose_region(explicit: str | None) -> str:
    if explicit:
        return explicit
    if not sys.stdin.isatty():
        return "global"
    answer = input("Alibaba Cloud Coding Plan region [global/china] (global): ").strip().lower()
    return answer or "global"


def load_json(path: Path) -> dict:
    if not path.exists():
        return {}
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise SystemExit(f"Refusing to modify invalid JSON in {path}: {error}") from error
    if not isinstance(value, dict):
        raise SystemExit(f"Refusing to modify {path}: root JSON value must be an object.")
    return value


def load_catalog(region: str) -> dict:
    raw = TEMPLATE.read_text(encoding="utf-8")
    raw = raw.replace("{{CODING_PLAN_BASE_URL}}", REGIONS[region])
    return json.loads(raw)


def model_key(model: dict) -> tuple[str, str]:
    return str(model.get("id", "")), str(model.get("baseUrl", ""))


def merge_models(existing: list[dict], wanted: list[dict]) -> list[dict]:
    result = list(existing)
    positions = {model_key(model): index for index, model in enumerate(result)}
    for model in wanted:
        key = model_key(model)
        if key in positions:
            result[positions[key]] = model
        else:
            positions[key] = len(result)
            result.append(model)
    return result


def main() -> None:
    args = parse_args()
    region = choose_region(args.region)
    if region not in REGIONS:
        raise SystemExit("Region must be 'global' or 'china'.")

    output = args.output if args.output.is_absolute() else ROOT / args.output
    current = load_json(output)
    catalog = load_catalog(region)

    providers = current.setdefault("modelProviders", {})
    openai = providers.setdefault("openai", {})
    openai["protocol"] = "openai"
    current_models = openai.get("models", [])
    if not isinstance(current_models, list):
        raise SystemExit("modelProviders.openai.models must be an array.")
    wanted = catalog["modelProviders"]["openai"]["models"]
    openai["models"] = merge_models(current_models, wanted)

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(current, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    print(f"Updated {output.relative_to(ROOT) if output.is_relative_to(ROOT) else output}")
    print("No API keys were written.")
    print("Set the relevant environment variable before starting Qwen Code:")
    print("  DEEPSEEK_API_KEY")
    print("  ZAI_API_KEY")
    print("  BAILIAN_CODING_PLAN_API_KEY")
    print("Then start qwen and use /model to select a configured model.")


if __name__ == "__main__":
    main()
