#!/bin/bash
set -e

PLUGINS=(
    "hello-wasm-plugin"
    "weather-plugin"
    "json-formatter-plugin"
    "markdown-plugin"
    "snippet-store-plugin"
)

echo "Building all WASM plugins..."
echo "============================="

TOTAL_SIZE=0
SUCCESS_COUNT=0
FAIL_COUNT=0

for plugin in "${PLUGINS[@]}"; do
    echo ""
    echo "Building $plugin..."
    cd "$plugin"
    
    if cargo build --target wasm32-wasip1 --release 2>&1; then
        # 显示文件大小
        WASM_FILE="target/wasm32-wasip1/release/${plugin//-/_}.wasm"
        if [ -f "$WASM_FILE" ]; then
            SIZE=$(wc -c < "$WASM_FILE")
            SIZE_KB=$((SIZE / 1024))
            TOTAL_SIZE=$((TOTAL_SIZE + SIZE))
            echo "✓ Built: $WASM_FILE ($SIZE_KB KB)"
            SUCCESS_COUNT=$((SUCCESS_COUNT + 1))
        else
            echo "✗ Failed to find output file for $plugin"
            FAIL_COUNT=$((FAIL_COUNT + 1))
        fi
    else
        echo "✗ Failed to build $plugin"
        FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    
    cd ..
done

echo ""
echo "============================="
echo "Build Summary:"
echo "  Success: $SUCCESS_COUNT/${#PLUGINS[@]}"
echo "  Failed: $FAIL_COUNT"
TOTAL_SIZE_KB=$((TOTAL_SIZE / 1024))
echo "  Total Size: $TOTAL_SIZE_KB KB"
echo ""

if [ $FAIL_COUNT -eq 0 ]; then
    echo "✓ All plugins built successfully!"
    exit 0
else
    echo "✗ Some plugins failed to build"
    exit 1
fi
