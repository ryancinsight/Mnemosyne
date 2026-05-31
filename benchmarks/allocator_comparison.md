# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 549.608 | 2642.265 | 4105.014 | N/A | 1722.316 | 0.21x | 0.13x | N/A | 0.32x |
| allocator allocation latency/large_8192 | 22.838 | 292.402 | 1228.571 | 434.936 | 75.789 | 0.08x | 0.02x | 0.05x | 0.30x |
| allocator allocation latency/medium_1024 | 11.330 | 65.635 | 262.975 | 88.413 | 27.846 | 0.17x | 0.04x | 0.13x | 0.41x |
| allocator allocation latency/small_32 | 9.359 | 21.172 | 15.546 | 13.727 | 12.685 | 0.44x | 0.60x | 0.68x | 0.74x |
| allocator burst retention/large_8192 | 3331.312 | 9325.798 | 453370.521 | 21136.079 | 26806.924 | 0.36x | 0.01x | 0.16x | 0.12x |
| allocator burst retention/medium_1024 | 1029.814 | 7155.167 | 76784.013 | 8023.295 | 9189.794 | 0.14x | 0.01x | 0.13x | 0.11x |
| allocator burst retention/small_32 | 617.321 | 6267.871 | 831.227 | 4224.937 | 2595.998 | 0.10x | 0.74x | 0.15x | 0.24x |
| allocator cycle latency/huge_2m | 22.375 | 8103.567 | 9031.164 | N/A | 115.639 | 0.00x | 0.00x | N/A | 0.19x |
| allocator cycle latency/large_8192 | 2.049 | 23.002 | 16.717 | 17.498 | 15.530 | 0.09x | 0.12x | 0.12x | 0.13x |
| allocator cycle latency/medium_1024 | 2.071 | 20.221 | 6.725 | 16.732 | 7.278 | 0.10x | 0.31x | 0.12x | 0.28x |
| allocator cycle latency/small_32 | 2.055 | 20.353 | 2.903 | 16.351 | 6.901 | 0.10x | 0.71x | 0.13x | 0.30x |
| allocator deallocation latency/huge_2m | 1176.117 | 3996.807 | 4424.607 | N/A | 3001.056 | 0.29x | 0.27x | N/A | 0.39x |
| allocator deallocation latency/large_8192 | 15.581 | 76.286 | 468.788 | 151.834 | 47.002 | 0.20x | 0.03x | 0.10x | 0.33x |
| allocator deallocation latency/medium_1024 | 8.420 | 27.542 | 75.777 | 45.112 | 18.108 | 0.31x | 0.11x | 0.19x | 0.46x |
| allocator deallocation latency/small_32 | 2.749 | 9.247 | 4.976 | 9.655 | 6.511 | 0.30x | 0.55x | 0.28x | 0.42x |
| cross-thread free handoff/huge_2m | 948.457 | 115493.557 | 121617.879 | N/A | 5208.736 | 0.01x | 0.01x | N/A | 0.18x |
| cross-thread free handoff/large_8192 | 28522.679 | 59202.939 | 1048273.409 | 88960.907 | 81419.555 | 0.48x | 0.03x | 0.32x | 0.35x |
| cross-thread free handoff/medium_1024 | 13628.369 | 30576.426 | 154251.158 | 35973.616 | 44235.981 | 0.45x | 0.09x | 0.38x | 0.31x |
| cross-thread free handoff/small_32 | 8613.432 | 31816.037 | 11507.613 | 24965.847 | 32499.430 | 0.27x | 0.75x | 0.35x | 0.27x |
| realloc latency/cross_class_32_to_64 | 9.272 | 46.611 | 7.595 | 32.906 | 17.032 | 0.20x | 1.22x | 0.28x | 0.54x |
| realloc latency/cross_class_8k_to_16k | 47.290 | 135.800 | 67.133 | 134.172 | 61.556 | 0.35x | 0.70x | 0.35x | 0.77x |
| realloc latency/huge_shrink_4m_to_2m | 21.909 | 956832.480 | 7558.091 | 1034319.414 | 251.877 | 0.00x | 0.00x | 0.00x | 0.09x |
| realloc latency/within_class_24_to_32 | 3.980 | 42.640 | 4.837 | 17.646 | 22.998 | 0.09x | 0.82x | 0.23x | 0.17x |
| realloc latency/within_class_6k_to_8k | 23.600 | 105.290 | 58.946 | 108.910 | 53.091 | 0.22x | 0.40x | 0.22x | 0.44x |
| segment cache eviction | 206746.735 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 59394.144 | 397273.866 | 76890.421 | 309049.910 | 145268.548 | 0.15x | 0.77x | 0.19x | 0.41x |
| threaded small allocation cycles | 10142.952 | 34120.626 | 10306.273 | 27175.308 | 19429.154 | 0.30x | 0.98x | 0.37x | 0.52x |
| usable size latency/huge_2m | 22.122 | N/A | 6036.355 | N/A | 118.022 | N/A | 0.00x | N/A | 0.19x |
| usable size latency/large_8192 | 2.568 | N/A | 16.554 | 17.703 | 17.537 | N/A | 0.16x | 0.15x | 0.15x |
| usable size latency/medium_1024 | 3.472 | N/A | 5.953 | 16.966 | 10.484 | N/A | 0.58x | 0.20x | 0.33x |
| usable size latency/small_32 | 3.453 | N/A | 2.928 | 16.523 | 9.922 | N/A | 1.18x | 0.21x | 0.35x |
| usable size query latency/huge_2m | 0.443 | N/A | 0.751 | N/A | 3.224 | N/A | 0.59x | N/A | 0.14x |
| usable size query latency/large_8192 | 0.304 | N/A | 0.617 | 0.581 | 3.264 | N/A | 0.49x | 0.52x | 0.09x |
| usable size query latency/medium_1024 | 0.386 | N/A | 0.871 | 0.505 | 3.316 | N/A | 0.44x | 0.76x | 0.12x |
| usable size query latency/small_32 | 0.263 | N/A | 0.893 | 0.607 | 3.298 | N/A | 0.29x | 0.43x | 0.08x |
