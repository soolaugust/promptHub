#!/usr/bin/env bash
# seed.sh — Preload official layers into the demo registry
#
# Usage:
#   REGISTRY_URL=https://prompthub-demo.onrender.com \
#   ADMIN_TOKEN=phrt_demo_readonly_changeme \
#   ./seed.sh [layers_dir]
#
# layers_dir defaults to ./prompthub/layers (relative to repo root)

set -euo pipefail

REGISTRY_URL="${REGISTRY_URL:-http://localhost:8080}"
ADMIN_TOKEN="${ADMIN_TOKEN:-phrt_demo_readonly_changeme}"
LAYERS_DIR="${1:-$(dirname "$0")/../prompthub/layers}"
LAYERS_DIR="$(realpath "$LAYERS_DIR")"

if [[ ! -d "$LAYERS_DIR" ]]; then
  echo "ERROR: layers directory not found: $LAYERS_DIR" >&2
  exit 1
fi

echo "Seeding registry at $REGISTRY_URL"
echo "Layers dir: $LAYERS_DIR"
echo ""

push_layer() {
  local namespace="$1"
  local name="$2"
  local version="$3"
  local layer_dir="$LAYERS_DIR/$namespace/$name"

  if [[ ! -f "$layer_dir/layer.yaml" ]] || [[ ! -f "$layer_dir/prompt.md" ]]; then
    echo "  SKIP  $namespace/$name:$version (files missing)"
    return
  fi

  local http_code
  http_code=$(curl -s -o /dev/null -w "%{http_code}" \
    -X PUT "$REGISTRY_URL/layers/$namespace/$name/$version" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -F "layer.yaml=@$layer_dir/layer.yaml;type=application/octet-stream" \
    -F "prompt.md=@$layer_dir/prompt.md;type=text/plain")

  case "$http_code" in
    201) echo "  OK    $namespace/$name:$version" ;;
    409) echo "  SKIP  $namespace/$name:$version (already exists)" ;;
    *)   echo "  FAIL  $namespace/$name:$version (HTTP $http_code)" ;;
  esac
}

# Base layers
push_layer base code-reviewer v1.0
push_layer base translator    v1.0
push_layer base writer        v1.0
push_layer base analyst       v1.0
push_layer base frontend-builder v1.0
push_layer base office-doc    v1.0

# Style layers
push_layer style concise  v1.0
push_layer style verbose  v1.0
push_layer style academic v1.0

# Guard layers
push_layer guard no-secrets  v1.0
push_layer guard safe-output v1.0
push_layer guard fact-check  v1.0

# Lang layers
push_layer lang chinese-markdown  v1.0
push_layer lang english-academic  v1.0
push_layer lang structured-output v1.0

echo ""
echo "Done. Visit $REGISTRY_URL to browse layers."
