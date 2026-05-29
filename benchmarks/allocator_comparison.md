# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 107.288 | 180.244 | 250.403 | 103.364 | N/A | 0.60x | 0.43x | 1.04x | N/A |
| allocator allocation latency/small_32 | 12.019 | 33.864 | 16.436 | 17.160 | N/A | 0.35x | 0.73x | 0.70x | N/A |
| allocator burst retention/large_8192 | 4081.387 | 8941.467 | 385364.062 | 19734.595 | N/A | 0.46x | 0.01x | 0.21x | N/A |
| allocator burst retention/medium_1024 | 1945.457 | 6729.333 | 75525.439 | 7453.418 | N/A | 0.29x | 0.03x | 0.26x | N/A |
| allocator burst retention/small_32 | 983.993 | 6234.924 | 879.601 | 4197.339 | N/A | 0.16x | 1.12x | 0.23x | N/A |
| allocator cycle latency/large_8192 | 3.610 | 20.153 | 16.817 | 17.354 | N/A | 0.18x | 0.21x | 0.21x | N/A |
| allocator cycle latency/medium_1024 | 3.605 | 21.843 | 5.661 | 16.604 | N/A | 0.17x | 0.64x | 0.22x | N/A |
| allocator cycle latency/small_32 | 3.621 | 20.020 | 2.743 | 14.952 | N/A | 0.18x | 1.32x | 0.24x | N/A |
| allocator deallocation latency/medium_1024 | 86.967 | 95.322 | 98.264 | 75.529 | N/A | 0.91x | 0.89x | 1.15x | N/A |
| allocator deallocation latency/small_32 | 5.409 | 17.698 | 5.324 | 9.402 | N/A | 0.31x | 1.02x | 0.58x | N/A |
| cross-thread free handoff/medium_1024 | 20445.459 | 32249.414 | 140051.758 | 36220.166 | N/A | 0.63x | 0.15x | 0.56x | N/A |
| cross-thread free handoff/small_32 | 16271.240 | 28006.079 | 16272.952 | 19903.564 | N/A | 0.58x | 1.00x | 0.82x | N/A |
| realloc latency/cross_class_32_to_64 | 10.725 | 42.947 | 7.420 | 32.947 | N/A | 0.25x | 1.45x | 0.33x | N/A |
| realloc latency/within_class_24_to_32 | 4.458 | 42.743 | 4.443 | 17.126 | N/A | 0.10x | 1.00x | 0.26x | N/A |
| segment cache eviction | 66283.545 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 256832.227 | 359015.234 | 59753.113 | 271019.336 | N/A | 0.72x | 4.30x | 0.95x | N/A |
| threaded small allocation cycles | 25453.247 | 36117.065 | 4506.808 | 25096.484 | N/A | 0.70x | 5.65x | 1.01x | N/A |
| usable size latency/medium_1024 | 3.871 | N/A | 6.063 | 16.790 | N/A | N/A | 0.64x | 0.23x | N/A |
| usable size latency/small_32 | 3.860 | N/A | 2.867 | 16.387 | N/A | N/A | 1.35x | 0.24x | N/A |
| usable size query latency/medium_1024 | 0.340 | N/A | 0.528 | 0.455 | N/A | N/A | 0.64x | 0.75x | N/A |
| usable size query latency/small_32 | 0.346 | N/A | 0.529 | 0.461 | N/A | N/A | 0.65x | 0.75x | N/A |
