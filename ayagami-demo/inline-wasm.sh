#!/bin/bash

for wasm in "$TRUNK_STAGING_DIR"/*.wasm; do
    echo "Inlining $wasm..."
    (
        echo -n "const data = Uint8Array.fromBase64('"
        base64 -w 0 < "$wasm" | tr -d '\n'
        echo "');"
        echo "export default data;"
    ) > "$wasm.module.js"
done

# Block other requests via CSP
sed -i -e "s/connect-src 'self'/connect-src 'none'/" "$TRUNK_STAGING_DIR/index.html"
