#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ask() {
  local prompt="$1" default="$2" value
  read -r -p "$prompt [$default]: " value || true
  printf '%s' "${value:-$default}"
}

slugify() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9]+/-/g; s/^-+|-+$//g'
}

PROJECT_NAME="$(ask 'Project name' 'Example Rust Product')"
PROJECT_SLUG="$(slugify "$PROJECT_NAME")"
PRODUCT_PROBLEM="$(ask 'One-sentence product problem' 'Describe the real product problem before implementation')"
ARCHITECTURE_PROFILE="$(ask 'Architecture profile (modular-monolith, coarse-microservices, undecided)' 'undecided')"
BOUNDED_CONTEXTS="$(ask 'Candidate bounded contexts, comma separated' 'identity,core-domain')"
FRONTEND="$(ask 'Frontend (leptos, external, none, undecided)' 'leptos')"
BROWSER_API="$(ask 'Browser API (graphql-bff, rest, undecided)' 'graphql-bff')"
INTERNAL_API="$(ask 'Internal synchronous API (http, grpc, mixed, undecided)' 'mixed')"
REALTIME="$(ask 'Realtime (sse, websocket, both, none, undecided)' 'undecided')"
DATABASE="$(ask 'Database' 'postgresql')"
BROKER="$(ask 'Message broker' 'rabbitmq')"
AUTH="$(ask 'Authentication approach' 'undecided')"
DEPLOYMENT="$(ask 'Deployment target' 'undecided')"
REPOSITORY_URL="$(ask 'Repository URL' 'https://example.invalid/replace-me')"
LICENSE="$(ask 'SPDX license expression' 'Apache-2.0 OR MIT')"
MSRV="$(ask 'Minimum supported Rust version' '1.85.0')"
RUST_VERSION="$(ask 'Rust toolchain channel or version' 'stable')"
AGENT_HARNESS="$(ask 'Primary coding harness (codex, opencode, claude, qwen, undecided)' 'undecided')"
MODEL_PROFILE="$(ask 'Preferred model profile (auto, kimi, deepseek, glm, qwen)' 'auto')"

cat > .project-template.env <<EOF
PROJECT_NAME=$(printf '%q' "$PROJECT_NAME")
PROJECT_SLUG=$(printf '%q' "$PROJECT_SLUG")
ARCHITECTURE_PROFILE=$(printf '%q' "$ARCHITECTURE_PROFILE")
BOUNDED_CONTEXTS=$(printf '%q' "$BOUNDED_CONTEXTS")
FRONTEND=$(printf '%q' "$FRONTEND")
BROWSER_API=$(printf '%q' "$BROWSER_API")
INTERNAL_API=$(printf '%q' "$INTERNAL_API")
REALTIME=$(printf '%q' "$REALTIME")
DATABASE=$(printf '%q' "$DATABASE")
BROKER=$(printf '%q' "$BROKER")
AUTH=$(printf '%q' "$AUTH")
DEPLOYMENT=$(printf '%q' "$DEPLOYMENT")
REPOSITORY_URL=$(printf '%q' "$REPOSITORY_URL")
LICENSE=$(printf '%q' "$LICENSE")
MSRV=$(printf '%q' "$MSRV")
RUST_VERSION=$(printf '%q' "$RUST_VERSION")
AGENT_HARNESS=$(printf '%q' "$AGENT_HARNESS")
MODEL_PROFILE=$(printf '%q' "$MODEL_PROFILE")
EOF

mkdir -p docs/project
cat > docs/project/project-context.md <<EOF
# Project Context

## Identity

Project name: $PROJECT_NAME  
Project slug: \`$PROJECT_SLUG\`

## Product Problem

$PRODUCT_PROBLEM

## Initial Technical Preferences

| Dimension | Current decision |
|---|---|
| Architecture profile | $ARCHITECTURE_PROFILE |
| Candidate bounded contexts | $BOUNDED_CONTEXTS |
| Frontend | $FRONTEND |
| Browser API | $BROWSER_API |
| Internal synchronous API | $INTERNAL_API |
| Realtime | $REALTIME |
| Database | $DATABASE |
| Broker | $BROKER |
| Authentication | $AUTH |
| Deployment | $DEPLOYMENT |
| Repository | $REPOSITORY_URL |
| License | $LICENSE |
| MSRV | $MSRV |
| Rust toolchain | $RUST_VERSION |
| Primary coding harness | $AGENT_HARNESS |
| Preferred model profile | $MODEL_PROFILE |

## Required Discovery Before Architecture Approval

- Actors and permissions.
- Critical user journeys.
- Explicit non-goals.
- Data sensitivity and retention.
- Expected load and latency.
- Availability and recovery objectives.
- Compliance and regional constraints.
- Operational ownership and deployment environment.

Unknown values remain unknown until the owner answers them. Agents must not infer them from this template.
EOF

render() {
  local src="$1" dst="$2"
  sed \
    -e "s|{{PROJECT_SLUG}}|$PROJECT_SLUG|g" \
    -e "s|{{RUST_VERSION}}|$RUST_VERSION|g" \
    -e "s|{{MSRV}}|$MSRV|g" \
    -e "s|{{LICENSE}}|$LICENSE|g" \
    -e "s|{{REPOSITORY_URL}}|$REPOSITORY_URL|g" \
    "$src" > "$dst"
}

render templates/rust-toolchain.toml.tmpl rust-toolchain.toml
render templates/env.example.tmpl .env.example

printf '\nConfigured %s. Review docs/project/project-context.md before workspace initialization.\n' "$PROJECT_NAME"
