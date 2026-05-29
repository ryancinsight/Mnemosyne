# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 22.954 | 61.521 | 345.379 | 116.685 | N/A | 0.37x | 0.07x | 0.20x | N/A |
| allocator allocation latency/small_32 | 11.312 | 32.470 | 20.514 | 14.152 | N/A | 0.35x | 0.55x | 0.80x | N/A |
| allocator burst retention/large_8192 | 7246.274 | 12175.098 | 558314.062 | 21621.436 | N/A | 0.60x | 0.01x | 0.34x | N/A |
| allocator burst retention/medium_1024 | 2211.653 | 7992.914 | 101986.865 | 7684.222 | N/A | 0.28x | 0.02x | 0.29x | N/A |
| allocator burst retention/small_32 | 2195.262 | 7644.434 | 1293.334 | 3978.256 | N/A | 0.29x | 1.70x | 0.55x | N/A |
| allocator cycle latency/large_8192 | 5.375 | 20.401 | 16.873 | 17.303 | N/A | 0.26x | 0.32x | 0.31x | N/A |
| allocator cycle latency/medium_1024 | 5.378 | 20.720 | 5.751 | 16.846 | N/A | 0.26x | 0.94x | 0.32x | N/A |
| allocator cycle latency/small_32 | 5.330 | 20.504 | 2.756 | 16.204 | N/A | 0.26x | 1.93x | 0.33x | N/A |
| allocator deallocation latency/medium_1024 | 24.641 | 39.080 | 89.026 | 53.692 | N/A | 0.63x | 0.28x | 0.46x | N/A |
| allocator deallocation latency/small_32 | 4.331 | 15.448 | 6.300 | 9.962 | N/A | 0.28x | 0.69x | 0.43x | N/A |
| cross-thread free handoff/medium_1024 | 22508.052 | 36564.356 | 162421.160 | 38560.589 | N/A | 0.62x | 0.14x | 0.58x | N/A |
| cross-thread free handoff/small_32 | 16815.333 | 32166.366 | 14144.503 | 21502.299 | N/A | 0.52x | 1.19x | 0.78x | N/A |
| realloc latency/cross_class_32_to_64 | 12.932 | 45.621 | 7.856 | 44.618 | N/A | 0.28x | 1.65x | 0.29x | N/A |
| realloc latency/within_class_24_to_32 | 5.523 | 43.012 | 4.452 | 17.174 | N/A | 0.13x | 1.24x | 0.32x | N/A |
| segment cache eviction | 65683.398 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 112238.315 | 409409.758 | 81850.338 | 272029.599 | N/A | 0.27x | 1.37x | 0.41x | N/A |
| threaded small allocation cycles | 11666.019 | 42682.467 | 9670.874 | 32402.309 | N/A | 0.27x | 1.21x | 0.36x | N/A |
| usable size latency/medium_1024 | 6.294 | N/A | 7.011 | 17.121 | N/A | N/A | 0.90x | 0.37x | N/A |
| usable size latency/small_32 | 6.381 | N/A | 4.219 | 16.387 | N/A | N/A | 1.51x | 0.39x | N/A |
| usable size query latency/medium_1024 | 0.510 | N/A | 0.897 | 0.687 | N/A | N/A | 0.57x | 0.74x | N/A |
| usable size query latency/small_32 | 0.509 | N/A | 0.941 | 0.701 | N/A | N/A | 0.54x | 0.73x | N/A |
