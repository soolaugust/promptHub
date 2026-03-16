#!/bin/sh
# Start registry in background, wait for it to be ready, then seed layers.
set -e

REGISTRY_URL="http://localhost:8080"
ADMIN_TOKEN="${ADMIN_TOKEN:-phrt_demo_readonly_changeme}"
LAYERS_DIR="/layers"

ph-registry /etc/prompthub/registry.yaml &
REGISTRY_PID=$!

# Wait for registry to be ready
echo "Waiting for registry to start..."
for i in $(seq 1 30); do
    if wget -q -O /dev/null "$REGISTRY_URL/layers" 2>/dev/null; then
        echo "Registry is ready."
        break
    fi
    sleep 1
done

# Seed official layers if layers directory exists
if [ -d "$LAYERS_DIR" ]; then
    echo "Seeding layers from $LAYERS_DIR..."
    for layer_yaml in "$LAYERS_DIR"/*/*/layer.yaml; do
        # Extract namespace/name/version from path: /layers/base/code-reviewer/layer.yaml
        dir=$(dirname "$layer_yaml")
        name=$(basename "$dir")
        ns_dir=$(dirname "$dir")
        namespace=$(basename "$ns_dir")
        # Read version from layer.yaml
        version=$(grep '^version:' "$layer_yaml" | head -1 | sed 's/version: *//;s/"//g;s/ *$//')
        prompt_md="$dir/prompt.md"

        if [ -z "$version" ] || [ ! -f "$prompt_md" ]; then
            continue
        fi

        http_code=$(wget -q -O /dev/null -S \
            --method=PUT \
            --header="Authorization: Bearer $ADMIN_TOKEN" \
            --body-file=/dev/null \
            "$REGISTRY_URL/layers/$namespace/$name/$version" 2>&1 | grep "HTTP/" | tail -1 | awk '{print $2}' || true)

        # Use curl if available, otherwise skip multipart (wget can't do multipart)
        if command -v curl >/dev/null 2>&1; then
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
        fi
    done
    echo "Seeding done."
fi

# Wait for registry process
wait $REGISTRY_PID
