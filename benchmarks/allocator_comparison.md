# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 27.116 | 54.648 | 285.593 | 78.944 | N/A | 0.50x | 0.09x | 0.34x | N/A |
| allocator allocation latency/small_32 | 12.430 | 30.536 | 17.426 | 14.605 | N/A | 0.41x | 0.71x | 0.85x | N/A |
| allocator burst retention/large_8192 | 7246.274 | 12175.098 | 558314.062 | 21621.436 | N/A | 0.60x | 0.01x | 0.34x | N/A |
| allocator burst retention/medium_1024 | 2211.653 | 7992.914 | 101986.865 | 7684.222 | N/A | 0.28x | 0.02x | 0.29x | N/A |
| allocator burst retention/small_32 | 2195.262 | 7644.434 | 1293.334 | 3978.256 | N/A | 0.29x | 1.70x | 0.55x | N/A |
| allocator cycle latency/large_8192 | 5.547 | 21.263 | 16.672 | 17.470 | N/A | 0.26x | 0.33x | 0.32x | N/A |
| allocator cycle latency/medium_1024 | 5.383 | 20.306 | 5.697 | 16.623 | N/A | 0.27x | 0.94x | 0.32x | N/A |
| allocator cycle latency/small_32 | 5.293 | 20.173 | 2.790 | 16.287 | N/A | 0.26x | 1.90x | 0.32x | N/A |
| allocator deallocation latency/medium_1024 | 22.458 | 31.095 | 129.086 | 64.022 | N/A | 0.72x | 0.17x | 0.35x | N/A |
| allocator deallocation latency/small_32 | 4.231 | 19.284 | 7.548 | 9.845 | N/A | 0.22x | 0.56x | 0.43x | N/A |
| cross-thread free handoff/medium_1024 | 20828.455 | 35299.738 | 148006.360 | 37724.310 | N/A | 0.59x | 0.14x | 0.55x | N/A |
| cross-thread free handoff/small_32 | 17147.614 | 28440.618 | 15835.265 | 20958.640 | N/A | 0.60x | 1.08x | 0.82x | N/A |
| realloc latency/cross_class_32_to_64 | 13.005 | 43.543 | 7.982 | 33.143 | N/A | 0.30x | 1.63x | 0.39x | N/A |
| realloc latency/within_class_24_to_32 | 5.607 | 43.682 | 4.484 | 17.230 | N/A | 0.13x | 1.25x | 0.33x | N/A |
| segment cache eviction | 65683.398 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 76681.749 | 348585.439 | 61717.527 | 262230.297 | N/A | 0.22x | 1.24x | 0.29x | N/A |
| threaded small allocation cycles | 8161.640 | 31794.351 | 4710.414 | 25070.076 | N/A | 0.26x | 1.73x | 0.33x | N/A |
| usable size latency/medium_1024 | 5.697 | N/A | 6.131 | 16.923 | N/A | N/A | 0.93x | 0.34x | N/A |
| usable size latency/small_32 | 5.586 | N/A | 3.121 | 16.355 | N/A | N/A | 1.79x | 0.34x | N/A |
| usable size query latency/medium_1024 | 0.427 | N/A | 0.729 | 0.683 | N/A | N/A | 0.59x | 0.63x | N/A |
| usable size query latency/small_32 | 0.419 | N/A | 0.744 | 0.449 | N/A | N/A | 0.56x | 0.93x | N/A |
