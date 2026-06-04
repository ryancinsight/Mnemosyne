# mimalloc-bench Mapping

Source: <https://github.com/daanx/mimalloc-bench>, inspected at commit `941c265d972fa83579e141b33ed6bbbde6d1487a`.

This repository does not vendor or execute the upstream C/C++ benchmark suite. The upstream `bench/` tree contains separately licensed benchmark programs and Unix-oriented shell orchestration. The local integration maps benchmark shapes into the Rust Criterion harness so allocator calls, comparators, and reports remain under the repository's Rust benchmark path.

## Implemented

- [patch] `bench/glibc-bench/bench-malloc-simple.c` → `Mimalloc-bench glibc simple`
  - Sizes: `16`, `32`, `64` bytes.
  - Allocation counts: `25`, `100`, `400`, `1600`.
  - Workload: allocate all blocks, write the requested payload, free the first half FIFO, then free the second half LIFO.
  - Scenarios:
    - `Mimalloc-bench glibc simple`: main-thread run before explicit thread creation.
    - `Mimalloc-bench glibc simple main-after-thread`: main-thread run after creating and joining a warmup thread, matching the upstream `SINGLE_THREAD_P == false` pass at the benchmark-surface level.
    - `Mimalloc-bench glibc simple thread`: the same workload executed by a persistent worker thread, matching the upstream thread-arena pass without measuring per-iteration thread creation.
  - Memory bound: pointer scratch is heap-allocated once per Criterion case and reused inside measured iterations; no stack-resident pointer array is used. The broader benchmark harness also keeps burst, handoff, and segment-eviction live-set scratch on the heap.

## Deferred

- [patch] `bench/malloc-large/malloc-large.cpp` needs an explicit process memory ceiling before inclusion because upstream keeps 20 live buffers in the 5-25 MiB range.
- [patch] `bench/rptest/rptest.c` needs a bounded Rust model for distribution selection, working-set size, and cross-thread deallocation rate.
- [patch] `bench/mstress/mstress.c` remains audit-only because upstream states it should not be used as a benchmark.
