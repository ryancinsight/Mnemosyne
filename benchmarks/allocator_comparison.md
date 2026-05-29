# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 120.417 | 256.688 | 303.398 | 144.837 | N/A | 0.47x | 0.40x | 0.83x | N/A |
| allocator allocation latency/small_32 | 14.033 | 38.497 | 21.684 | 16.356 | N/A | 0.36x | 0.65x | 0.86x | N/A |
| allocator burst retention/large_8192 | 4234.872 | 9244.934 | 506240.625 | 20908.826 | N/A | 0.46x | 0.01x | 0.20x | N/A |
| allocator burst retention/medium_1024 | 2210.498 | 8375.049 | 107603.076 | 7496.033 | N/A | 0.26x | 0.02x | 0.29x | N/A |
| allocator burst retention/small_32 | 1447.364 | 7462.744 | 1141.941 | 4185.477 | N/A | 0.19x | 1.27x | 0.35x | N/A |
| allocator cycle latency/large_8192 | 5.714 | 22.524 | 14.576 | 17.055 | N/A | 0.25x | 0.39x | 0.34x | N/A |
| allocator cycle latency/medium_1024 | 5.495 | 23.371 | 6.454 | 16.379 | N/A | 0.24x | 0.85x | 0.34x | N/A |
| allocator cycle latency/small_32 | 5.283 | 28.034 | 2.799 | 14.965 | N/A | 0.19x | 1.89x | 0.35x | N/A |
| allocator deallocation latency/medium_1024 | 134.372 | 99.613 | 128.419 | 92.091 | N/A | 1.35x | 1.05x | 1.46x | N/A |
| allocator deallocation latency/small_32 | 6.149 | 19.657 | 7.546 | 10.000 | N/A | 0.31x | 0.81x | 0.61x | N/A |
| cross-thread free handoff/medium_1024 | 13897.217 | 40906.921 | 211294.531 | 46530.847 | N/A | 0.34x | 0.07x | 0.30x | N/A |
| cross-thread free handoff/small_32 | 10825.743 | 34451.477 | 7261.301 | 20058.313 | N/A | 0.31x | 1.49x | 0.54x | N/A |
| realloc latency/cross_class_32_to_64 | 14.264 | 63.810 | 9.769 | 33.186 | N/A | 0.22x | 1.46x | 0.43x | N/A |
| realloc latency/within_class_24_to_32 | 6.158 | 62.922 | 7.780 | 16.960 | N/A | 0.10x | 0.79x | 0.36x | N/A |
| segment cache eviction | 73013.379 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 108965.137 | 442175.391 | 83797.705 | 274071.484 | N/A | 0.25x | 1.30x | 0.40x | N/A |
| threaded small allocation cycles | 11757.156 | 41892.969 | 7434.432 | 29268.652 | N/A | 0.28x | 1.58x | 0.40x | N/A |
| usable size latency/medium_1024 | 5.660 | N/A | 6.907 | 16.528 | N/A | N/A | 0.82x | 0.34x | N/A |
| usable size latency/small_32 | 7.114 | N/A | 4.108 | 16.016 | N/A | N/A | 1.73x | 0.44x | N/A |
| usable size query latency/medium_1024 | 0.460 | N/A | 0.743 | 0.556 | N/A | N/A | 0.62x | 0.83x | N/A |
| usable size query latency/small_32 | 0.515 | N/A | 1.036 | 0.588 | N/A | N/A | 0.50x | 0.88x | N/A |
