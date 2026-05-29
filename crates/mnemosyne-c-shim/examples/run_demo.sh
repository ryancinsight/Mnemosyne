#!/usr/bin/env bash
#
# run_demo.sh — Compiles and runs the interposition demo on Linux/macOS.
#

set -euo pipefail

# Navigate to the shim crate directory
cd "$(dirname "$0")/.."

echo "=== Building mnemosyne-c-shim cdylib ==="
cargo build --release

echo "=== Compiling C interpose_demo ==="
# We compile the demo using standard gcc.
# To demonstrate dynamic preloading interposition (LD_PRELOAD), we do NOT link it directly.
gcc -O2 -o examples/interpose_demo examples/interpose_demo.c

OS_TYPE=$(uname)
echo "Detected OS: $OS_TYPE"

if [ "$OS_TYPE" = "Linux" ]; then
    echo "=== Running under LD_PRELOAD interposition ==="
    LD_PRELOAD=../../target/release/libmnemosyne_c_shim.so ./examples/interpose_demo
elif [ "$OS_TYPE" = "Darwin" ]; then
    echo "=== Running under DYLD_INSERT_LIBRARIES interposition ==="
    DYLD_INSERT_LIBRARIES=../../target/release/libmnemosyne_c_shim.dylib DYLD_FORCE_FLAT_NAMESPACE=1 ./examples/interpose_demo
else
    echo "Dynamic preloading (LD_PRELOAD) is not natively supported on $OS_TYPE."
    echo "Linking dynamically instead:"
    gcc -O2 -Iinclude -o examples/interpose_demo_linked examples/interpose_demo.c -L../../target/release -lmnemosyne_c_shim
    ./examples/interpose_demo_linked
fi
