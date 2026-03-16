#!/bin/sh
# Start registry in background, wait for it to be ready, then seed layers.
set -e

REGISTRY_URL="http://localhost:8080"
ADMIN_TOKEN="${ADMIN_TOKEN:-phrt_demo_readonly_changeme}"
LAYERS_DIR="/layers"

ph-registry /etc/prompthub/registry.yaml &
REGISTRY_PID=$!

# Wait for registry to be ready (up to 30s)
echo "Waiting for registry to start..."
i=0
while [ $i -lt 30 ]; do
    if curl -sf "$REGISTRY_URL/layers" > /dev/null 2>&1; then
        echo "Registry is ready."
        break
    fi
    i=$((i + 1))
    sleep 1
done

# Seed official layers
if [ -d "$LAYERS_DIR" ]; then
    echo "Seeding layers from $LAYERS_DIR..."
    for layer_yaml in "$LAYERS_DIR"/*/*/layer.yaml; do
        [ -f "$layer_yaml" ] || continue
        dir=$(dirname "$layer_yaml")
        name=$(basename "$dir")
        namespace=$(basename "$(dirname "$dir")")
        version=$(grep '^version:' "$layer_yaml" | head -1 | sed 's/version:[[:space:]]*//' | tr -d '"')
        prompt_md="$dir/prompt.md"

        [ -n "$version" ] || continue
        [ -f "$prompt_md" ] || continue

        http_code=$(curl -s -o /dev/null -w "%{http_code}" \
            -X PUT "$REGISTRY_URL/layers/$namespace/$name/$version" \
            -H "Authorization: Bearer $ADMIN_TOKEN" \
            -F "layer.yaml=@$layer_yaml;type=application/octet-stream" \
            -F "prompt.md=@$prompt_md;type=text/plain")

        case "$http_code" in
            201) echo "  OK    $namespace/$name:$version" ;;
            409) echo "  SKIP  $namespace/$name:$version (already exists)" ;;
            *)   echo "  FAIL  $namespace/$name:$version (HTTP $http_code)" ;;
        esac
    done
    echo "Seeding done."
else
    echo "No layers directory found at $LAYERS_DIR, skipping seed."
fi

# Wait for registry process
wait $REGISTRY_PID
