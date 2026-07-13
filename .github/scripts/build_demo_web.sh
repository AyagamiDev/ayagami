#!/usr/bin/env bash
set -eu
script_path=$( cd "$(dirname "${BASH_SOURCE[0]}")" ; pwd -P )
cd "$script_path/../.."

CRATE_NAME=ayagami-demo

OPTIMIZE=false
BUILD=debug
BUILD_FLAGS="--config Trunk.release.toml"
GLOW=false

while test $# -gt 0; do
  case "$1" in
    -h|--help)
      echo "build_demo_web.sh [--release] [--glow] [--open]"
      echo ""
      echo "  -g:        Keep debug symbols even with --release."
      echo "             These are useful profiling and size trimming."
      echo ""
      echo "  --release: Build with --release, and then run wasm-opt."
      echo "             NOTE: --release also removes debug symbols, unless you also use -g."
      exit 0
      ;;

    -g)
      shift
      WASM_OPT_FLAGS="${WASM_OPT_FLAGS} -g"
      ;;

    --release)
      shift
      OPTIMIZE=true
      BUILD="release"
      BUILD_FLAGS="$BUILD_FLAGS --release"
      ;;

    *)
      echo "Unknown option: $1"
      exit 1
      ;;
  esac
done

OUT_FILE_NAME="ayagami-demo"

echo "Building with trunk…"

(cd $CRATE_NAME &&
  trunk build \
    ${BUILD_FLAGS} &&
  touch dist/.nojekyll
)
