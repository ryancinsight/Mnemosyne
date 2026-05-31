# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 691.320 | 2768.621 | 4245.526 | N/A | 1722.316 | 0.25x | 0.16x | N/A | 0.40x |
| allocator allocation latency/large_8192 | 21.179 | 360.272 | 1197.990 | 433.247 | 75.789 | 0.06x | 0.02x | 0.05x | 0.28x |
| allocator allocation latency/medium_1024 | 11.544 | 47.169 | 247.011 | 66.657 | 27.846 | 0.24x | 0.05x | 0.17x | 0.41x |
| allocator allocation latency/small_32 | 9.993 | 21.129 | 16.209 | 13.606 | 12.685 | 0.47x | 0.62x | 0.73x | 0.79x |
| allocator burst retention/large_8192 | 2773.686 | 8568.739 | 397993.279 | 19995.731 | 26806.924 | 0.32x | 0.01x | 0.14x | 0.10x |
| allocator burst retention/medium_1024 | 1018.551 | 7253.427 | 81517.324 | 8003.021 | 9189.794 | 0.14x | 0.01x | 0.13x | 0.11x |
| allocator burst retention/small_32 | 604.525 | 6384.835 | 827.045 | 4215.049 | 2595.998 | 0.09x | 0.73x | 0.14x | 0.23x |
| allocator cycle latency/huge_2m | 21.889 | 9011.949 | 12931.527 | N/A | 115.639 | 0.00x | 0.00x | N/A | 0.19x |
| allocator cycle latency/large_8192 | 2.210 | 20.314 | 16.866 | 17.181 | 15.530 | 0.11x | 0.13x | 0.13x | 0.14x |
| allocator cycle latency/medium_1024 | 2.211 | 20.347 | 5.685 | 16.614 | 7.278 | 0.11x | 0.39x | 0.13x | 0.30x |
| allocator cycle latency/small_32 | 2.227 | 20.210 | 2.785 | 16.152 | 6.901 | 0.11x | 0.80x | 0.14x | 0.32x |
| allocator deallocation latency/huge_2m | 977.805 | 5703.140 | 5177.259 | N/A | 3001.056 | 0.17x | 0.19x | N/A | 0.33x |
| allocator deallocation latency/large_8192 | 15.272 | 67.920 | 483.752 | 205.250 | 47.002 | 0.22x | 0.03x | 0.07x | 0.32x |
| allocator deallocation latency/medium_1024 | 8.719 | 20.764 | 75.482 | 45.300 | 18.108 | 0.42x | 0.12x | 0.19x | 0.48x |
| allocator deallocation latency/small_32 | 2.986 | 10.100 | 5.247 | 9.357 | 6.511 | 0.30x | 0.57x | 0.32x | 0.46x |
| cross-thread free handoff/huge_2m | 56604.749 | 87325.458 | 94139.381 | N/A | 5208.736 | 0.65x | 0.60x | N/A | 10.87x |
| cross-thread free handoff/large_8192 | 29837.481 | 54724.116 | 855801.992 | 93800.901 | 81419.555 | 0.55x | 0.03x | 0.32x | 0.37x |
| cross-thread free handoff/medium_1024 | 19865.315 | 32791.031 | 144623.719 | 37934.344 | 44235.981 | 0.61x | 0.14x | 0.52x | 0.45x |
| cross-thread free handoff/small_32 | 16484.041 | 30579.434 | 18156.715 | 20673.691 | 32499.430 | 0.54x | 0.91x | 0.80x | 0.51x |
| realloc latency/cross_class_32_to_64 | 5.872 | 43.361 | 7.559 | 32.864 | 17.032 | 0.14x | 0.78x | 0.18x | 0.34x |
| realloc latency/cross_class_8k_to_16k | 48.059 | 131.783 | 67.160 | 132.324 | 61.556 | 0.36x | 0.72x | 0.36x | 0.78x |
| realloc latency/huge_shrink_4m_to_2m | 73546.675 | 976030.415 | 6702.816 | 1039354.869 | 251.877 | 0.08x | 10.97x | 0.07x | 291.99x |
| realloc latency/within_class_24_to_32 | 2.699 | 43.437 | 4.410 | 17.131 | 22.998 | 0.06x | 0.61x | 0.16x | 0.12x |
| realloc latency/within_class_6k_to_8k | 24.024 | 103.021 | 56.375 | 93.840 | 53.091 | 0.23x | 0.43x | 0.26x | 0.45x |
| segment cache eviction | 218830.346 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 54875.030 | 359735.313 | 70489.206 | 263140.894 | 145268.548 | 0.15x | 0.78x | 0.21x | 0.38x |
| threaded small allocation cycles | 11829.054 | 32548.744 | 14746.564 | 26844.688 | 19429.154 | 0.36x | 0.80x | 0.44x | 0.61x |
| usable size latency/huge_2m | 22.304 | N/A | 8736.832 | N/A | 118.022 | N/A | 0.00x | N/A | 0.19x |
| usable size latency/large_8192 | 2.474 | N/A | 16.654 | 17.487 | 17.537 | N/A | 0.15x | 0.14x | 0.14x |
| usable size latency/medium_1024 | 3.387 | N/A | 5.966 | 16.780 | 10.484 | N/A | 0.57x | 0.20x | 0.32x |
| usable size latency/small_32 | 3.357 | N/A | 2.846 | 16.391 | 9.922 | N/A | 1.18x | 0.20x | 0.34x |
| usable size query latency/huge_2m | 0.416 | N/A | 0.523 | N/A | 3.224 | N/A | 0.80x | N/A | 0.13x |
| usable size query latency/large_8192 | 0.343 | N/A | 0.526 | 0.457 | 3.264 | N/A | 0.65x | 0.75x | 0.11x |
| usable size query latency/medium_1024 | 0.340 | N/A | 0.523 | 0.454 | 3.316 | N/A | 0.65x | 0.75x | 0.10x |
| usable size query latency/small_32 | 0.342 | N/A | 0.531 | 0.457 | 3.298 | N/A | 0.64x | 0.75x | 0.10x |
