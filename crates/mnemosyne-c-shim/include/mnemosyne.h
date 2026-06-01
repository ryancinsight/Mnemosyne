/*
 * mnemosyne.h — C declarations for the Mnemosyne allocator shim.
 *
 * Link against the `mnemosyne-c-shim` cdylib (or interpose it via
 * LD_PRELOAD on Unix / DLL injection on Windows) to route the C
 * allocator family through Mnemosyne.
 *
 * Every function matches the standard C / POSIX contract:
 *
 *   - malloc(0) returns a unique, freeable pointer (not NULL).
 *   - free(NULL) is a no-op.
 *   - calloc detects nmemb*size overflow and returns NULL.
 *   - realloc(NULL, n) == malloc(n); realloc(p, 0) frees p and returns NULL.
 *   - aligned_alloc requires `alignment` to be a power of two and `size`
 *     to be a multiple of `alignment` (C11), else returns NULL.
 *   - posix_memalign requires `alignment` to be a power of two and at
 *     least sizeof(void*), returning 0 / EINVAL / ENOMEM.
 *   - malloc_usable_size(NULL) == 0; otherwise returns the block's usable
 *     capacity, which may exceed the original request because Mnemosyne
 *     rounds small requests up to the next size class.
 *
 * All pointers returned here must be released with free() (or realloc to
 * size 0); they must NOT be passed to the system free().
 */

#ifndef MNEMOSYNE_H
#define MNEMOSYNE_H

#include <stddef.h> /* size_t */

#ifdef __cplusplus
extern "C" {
#endif

void *malloc(size_t size);
void free(void *ptr);
void *calloc(size_t nmemb, size_t size);
void *realloc(void *ptr, size_t new_size);
void *aligned_alloc(size_t alignment, size_t size);
int posix_memalign(void **memptr, size_t alignment, size_t size);
size_t malloc_usable_size(void *ptr);

/* Mnemosyne Custom Tracing and Profiling APIs */
void mnemosyne_register_alloc_hook(void (*hook)(void *ptr, size_t size));
void mnemosyne_register_free_hook(void (*hook)(void *ptr, size_t size));
void mnemosyne_enable_profiling(size_t sample_interval);
void mnemosyne_disable_profiling(void);
int mnemosyne_is_profiling_enabled(void);
int mnemosyne_dump_profile(const char *path);
void mnemosyne_reset_profiler_for_testing(void);

void mnemosyne_enable_leak_detector(void);
void mnemosyne_disable_leak_detector(void);
int mnemosyne_is_leak_detector_enabled(void);
int mnemosyne_dump_leaks(const char *path);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* MNEMOSYNE_H */
