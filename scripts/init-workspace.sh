#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

command -v cargo >/dev/null || { echo "cargo is required. Install Rust with rustup before running this script." >&2; exit 1; }

if [[ ! -f .project-template.env ]]; then
  echo "Run ./scripts/configure-project.sh first." >&2
  exit 1
fi
# shellcheck disable=SC1091
source .project-template.env

if [[ -f Cargo.toml ]]; then
  echo "Cargo.toml already exists. Refusing to overwrite it." >&2
  exit 1
fi

sed \
  -e "s|{{MSRV}}|$MSRV|g" \
  -e "s|{{LICENSE}}|$LICENSE|g" \
  -e "s|{{REPOSITORY_URL}}|$REPOSITORY_URL|g" \
  templates/cargo/workspace.toml.tmpl > Cargo.toml

mkdir -p apps services workers crates

create_lib() {
  local path="$1"
  [[ -e "$path/Cargo.toml" ]] || cargo new --lib "$path" --vcs none
}
create_bin() {
  local path="$1"
  [[ -e "$path/Cargo.toml" ]] || cargo new --bin "$path" --vcs none
}
render_service_contract() {
  local path="$1" name="$2" crate_name="$3"
  sed \
    -e "s|{{SERVICE_NAME}}|$name|g" \
    -e "s|{{SERVICE_RESPONSIBILITY}}|Defined by approved specifications for this bounded context.|g" \
    -e "s|{{CRATE_NAME}}|$crate_name|g" \
    templates/scoped/AGENTS.service.md.tmpl > "$path/AGENTS.md"
}

for crate in domain-primitives auth-context telemetry messaging-envelope test-support generated-contracts; do
  create_lib "crates/$crate"
done

IFS=',' read -ra contexts <<< "$BOUNDED_CONTEXTS"
for raw in "${contexts[@]}"; do
  context="$(printf '%s' "$raw" | xargs | tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9]+/-/g; s/^-+|-+$//g')"
  [[ -z "$context" ]] && continue
  create_bin "services/$context"
  render_service_contract "services/$context" "$context service" "$context"
done

if [[ "$BROKER" == "rabbitmq" ]]; then
  create_bin workers/notification-worker
  render_service_contract workers/notification-worker "notification worker" "notification-worker"
fi

if [[ "$FRONTEND" == "leptos" ]]; then
  cat > apps/README.md <<'EOF'
# Applications

The selected frontend is Leptos. Create the concrete application only after the first approved vertical-slice specification defines SSR, hydration, routing, browser contract, asset pipeline, and deployment requirements.
EOF
fi

cargo fmt --all
cargo metadata --no-deps --format-version 1 >/dev/null

echo "Workspace initialized. Dependencies were intentionally not added globally."
echo "Use an approved specification and the rust-dependency-selection skill before adding crates."
