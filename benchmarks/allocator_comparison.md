# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 379.834 | 3624.730 | 5200.259 | N/A | 2655.861 | 0.10x | 0.07x | N/A | 0.14x |
| allocator allocation latency/large_8192 | 26.563 | 364.739 | 2062.193 | 385.792 | 155.300 | 0.07x | 0.01x | 0.07x | 0.17x |
| allocator allocation latency/medium_1024 | 12.355 | 74.676 | 330.878 | 93.525 | 37.146 | 0.17x | 0.04x | 0.13x | 0.33x |
| allocator allocation latency/small_32 | 11.370 | 28.547 | 21.125 | 17.666 | 13.235 | 0.40x | 0.54x | 0.64x | 0.86x |
| allocator burst retention/large_8192 | 2412.571 | 13119.539 | 635126.379 | 35526.144 | 33959.304 | 0.18x | 0.00x | 0.07x | 0.07x |
| allocator burst retention/medium_1024 | 1340.506 | 8897.911 | 110977.195 | 9792.135 | 10694.303 | 0.15x | 0.01x | 0.14x | 0.13x |
| allocator burst retention/small_32 | 1142.103 | 9486.168 | 1571.433 | 4621.079 | 3558.152 | 0.12x | 0.73x | 0.25x | 0.32x |
| allocator cycle latency/huge_2m | 24.560 | 11522.336 | 11705.238 | N/A | 156.610 | 0.00x | 0.00x | N/A | 0.16x |
| allocator cycle latency/large_8192 | 3.740 | 27.179 | 19.717 | 19.218 | 25.481 | 0.14x | 0.19x | 0.19x | 0.15x |
| allocator cycle latency/medium_1024 | 2.479 | 23.581 | 6.418 | 18.737 | 13.181 | 0.11x | 0.39x | 0.13x | 0.19x |
| allocator cycle latency/small_32 | 2.376 | 25.008 | 3.883 | 17.832 | 7.916 | 0.10x | 0.61x | 0.13x | 0.30x |
| allocator deallocation latency/huge_2m | 351.557 | 5619.407 | 7177.988 | N/A | 4717.281 | 0.06x | 0.05x | N/A | 0.07x |
| allocator deallocation latency/large_8192 | 24.585 | 167.363 | 787.255 | 206.952 | 65.194 | 0.15x | 0.03x | 0.12x | 0.38x |
| allocator deallocation latency/medium_1024 | 10.593 | 36.719 | 135.332 | 74.350 | 19.419 | 0.29x | 0.08x | 0.14x | 0.55x |
| allocator deallocation latency/small_32 | 3.420 | 13.788 | 6.723 | 12.145 | 8.779 | 0.25x | 0.51x | 0.28x | 0.39x |
| cross-thread free handoff/huge_2m | 862.378 | 111598.205 | 123689.042 | N/A | 4395.753 | 0.01x | 0.01x | N/A | 0.20x |
| cross-thread free handoff/large_8192 | 32178.214 | 55255.262 | 988911.709 | 91511.811 | 89544.527 | 0.58x | 0.03x | 0.35x | 0.36x |
| cross-thread free handoff/medium_1024 | 14220.679 | 31493.800 | 154788.084 | 39887.898 | 46895.642 | 0.45x | 0.09x | 0.36x | 0.30x |
| cross-thread free handoff/small_32 | 10119.653 | 30334.037 | 11879.548 | 20700.930 | 29556.656 | 0.33x | 0.85x | 0.49x | 0.34x |
| realloc latency/cross_class_32_to_64 | 8.319 | 52.590 | 13.601 | 36.772 | 21.426 | 0.16x | 0.61x | 0.23x | 0.39x |
| realloc latency/cross_class_8k_to_16k | 47.554 | 133.708 | 67.626 | 138.059 | 67.295 | 0.36x | 0.70x | 0.34x | 0.71x |
| realloc latency/huge_shrink_4m_to_2m | 20.629 | 989227.112 | 9417.219 | 1037162.768 | 252.008 | 0.00x | 0.00x | 0.00x | 0.08x |
| realloc latency/within_class_24_to_32 | 4.776 | 211.922 | 6.176 | 19.673 | 18.877 | 0.02x | 0.77x | 0.24x | 0.25x |
| realloc latency/within_class_6k_to_8k | 39.262 | 169.726 | 72.130 | 99.952 | 52.303 | 0.23x | 0.54x | 0.39x | 0.75x |
| segment cache eviction | 216748.221 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 61922.821 | 406603.189 | 77682.630 | 274767.910 | 153940.371 | 0.15x | 0.80x | 0.23x | 0.40x |
| threaded small allocation cycles | 7104.743 | 34757.476 | 8823.246 | 28192.302 | 22629.031 | 0.20x | 0.81x | 0.25x | 0.31x |
| usable size latency/huge_2m | 24.449 | N/A | 14713.751 | N/A | 128.717 | N/A | 0.00x | N/A | 0.19x |
| usable size latency/large_8192 | 3.407 | N/A | 17.080 | 19.323 | 21.801 | N/A | 0.20x | 0.18x | 0.16x |
| usable size latency/medium_1024 | 5.619 | N/A | 8.054 | 21.479 | 14.343 | N/A | 0.70x | 0.26x | 0.39x |
| usable size latency/small_32 | 5.212 | N/A | 4.345 | 19.875 | 14.184 | N/A | 1.20x | 0.26x | 0.37x |
| usable size query latency/huge_2m | 0.642 | N/A | 0.878 | N/A | 3.584 | N/A | 0.73x | N/A | 0.18x |
| usable size query latency/large_8192 | 0.400 | N/A | 1.107 | 0.753 | 4.208 | N/A | 0.36x | 0.53x | 0.10x |
| usable size query latency/medium_1024 | 0.376 | N/A | 1.215 | 0.698 | 3.797 | N/A | 0.31x | 0.54x | 0.10x |
| usable size query latency/small_32 | 0.367 | N/A | 0.698 | 0.586 | 3.788 | N/A | 0.53x | 0.63x | 0.10x |
