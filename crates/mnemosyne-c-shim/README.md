# Mnemosyne C ABI Shim (`mnemosyne-c-shim`)

This crate exposes the Mnemosyne allocator through standard C / POSIX entry points, allowing C/C++ programs to use Mnemosyne or interpose it dynamically via `LD_PRELOAD` (on Unix) or DLL loading/linking (on Windows).

## Exposed Symbols

The shim exports the following standard C allocator family functions (from `<stdlib.h>`, `<malloc.h>`, and POSIX):

- `void *malloc(size_t size)`
- `void free(void *ptr)`
- `void *calloc(size_t nmemb, size_t size)`
- `void *realloc(void *ptr, size_t new_size)`
- `void *aligned_alloc(size_t alignment, size_t size)`
- `int posix_memalign(void **memptr, size_t alignment, size_t size)`
- `size_t malloc_usable_size(void *ptr)`

## Semantics: C vs. Rust `realloc`

A critical semantic difference exists between Rust and C allocator behaviors:
- **Rust `GlobalAlloc::realloc`**: Rust passes the original layout (`Layout`). The copy length is bounded by `min(layout.size(), new_size)` because the Rust compiler tracks the exact requested size.
- **C `realloc`**: C does not track the requested size per block; it only tracks the *usable block size* (which may be larger than the original request due to size-class rounding). Since a C program may write data into the entire usable region returned by `malloc`, the C `realloc` implementation must copy the lesser of the **old usable size** and the new size: `min(usable_size(old), new_size)`.

The shim handles this correctly by invoking `mnemosyne_local::usable_size` to determine the old usable size before performing the copy and deallocation.

## Building the C Shim

The crate is configured as both a `lib` and a `cdylib` in its `Cargo.toml`. Building in release mode:

```bash
cargo build --release -p mnemosyne-c-shim
```

This generates:
- **Linux**: `target/release/libmnemosyne_c_shim.so`
- **macOS**: `target/release/libmnemosyne_c_shim.dylib`
- **Windows**: `target/release/mnemosyne_c_shim.dll` (with `libmnemosyne_c_shim.dll.a` or `.lib` import library)

---

## Interposition and ABI Demonstration

An example is provided under `examples/` to prove dynamic interposition, size-class rounding via `malloc_usable_size`, and C alignment/correctness guarantees.

### 1. Source files
- [interpose_demo.c](file:///d:/Mnemosyne/crates/mnemosyne-c-shim/examples/interpose_demo.c): A standard C program allocating, writing, querying usable size, reallocating, and freeing.
- [mnemosyne.h](file:///d:/Mnemosyne/crates/mnemosyne-c-shim/include/mnemosyne.h): The header file providing standard declarations.

### 2. Running on Unix (Linux/macOS)
The demo can compile *without* linking the shim crate directly, proving symbol interposition via `LD_PRELOAD`:

```bash
# Automate build, compilation, and preloading execution
./examples/run_demo.sh
```

Or manually:
```bash
# 1. Compile the demo
gcc -O2 -o interpose_demo examples/interpose_demo.c

# 2. Run with preloaded Mnemosyne shim
LD_PRELOAD=../../target/release/libmnemosyne_c_shim.so ./interpose_demo
```

### 3. Running on Windows
Dynamic linking can be verified using the automated script:

```powershell
# Run the automated build and dynamic link test
.\examples\run_demo.ps1
```
