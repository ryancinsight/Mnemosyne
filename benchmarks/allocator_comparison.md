# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | RpMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs RpMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 2810.820 | 2565.796 | 911.008 | 2054.429 | N/A | N/A | 1.10x | 3.09x | 1.37x | N/A | N/A |
| allocator allocation latency/large_8192 | 27.775 | 292.303 | 71.913 | 27.426 | N/A | N/A | 0.10x | 0.39x | 1.01x | N/A | N/A |
| allocator allocation latency/medium_1024 | 11.405 | 51.079 | 22.203 | 22.916 | N/A | N/A | 0.22x | 0.51x | 0.50x | N/A | N/A |
| allocator allocation latency/small_32 | 9.250 | 19.753 | 7.029 | 7.166 | N/A | N/A | 0.47x | 1.32x | 1.29x | N/A | N/A |
| allocator burst retention/large_8192 | 2566.873 | 9953.585 | 13610.433 | 1615.344 | N/A | N/A | 0.26x | 0.19x | 1.59x | N/A | N/A |
| allocator burst retention/medium_1024 | 922.537 | 6266.298 | 2301.467 | 1304.911 | N/A | N/A | 0.15x | 0.40x | 0.71x | N/A | N/A |
| allocator burst retention/small_32 | 719.704 | 6115.593 | 797.395 | 969.911 | N/A | N/A | 0.12x | 0.90x | 0.74x | N/A | N/A |
| allocator cycle latency/huge_2m | 43.302 | 8665.095 | 239.220 | 8.289 | N/A | N/A | 0.00x | 0.18x | 5.22x | N/A | N/A |
| allocator cycle latency/large_8192 | 3.826 | 19.486 | 13.441 | 4.980 | N/A | N/A | 0.20x | 0.28x | 0.77x | N/A | N/A |
| allocator cycle latency/medium_1024 | 3.270 | 19.567 | 5.046 | 3.779 | N/A | N/A | 0.17x | 0.65x | 0.87x | N/A | N/A |
| allocator cycle latency/small_32 | 3.269 | 20.129 | 3.109 | 3.067 | N/A | N/A | 0.16x | 1.05x | 1.07x | N/A | N/A |
| allocator deallocation latency/huge_2m | 4582.003 | 3970.730 | 103.404 | 1924.217 | N/A | N/A | 1.15x | 44.31x | 2.38x | N/A | N/A |
| allocator deallocation latency/large_8192 | 34.144 | 105.681 | 36.124 | 18.586 | N/A | N/A | 0.32x | 0.95x | 1.84x | N/A | N/A |
| allocator deallocation latency/medium_1024 | 9.767 | 24.087 | 8.546 | 6.516 | N/A | N/A | 0.41x | 1.14x | 1.50x | N/A | N/A |
| allocator deallocation latency/small_32 | 2.633 | 8.993 | 2.573 | 2.723 | N/A | N/A | 0.29x | 1.02x | 0.97x | N/A | N/A |
| cross-thread free handoff/huge_2m | 1072.682 | 81451.756 | 7005.503 | 876.564 | N/A | N/A | 0.01x | 0.15x | 1.22x | N/A | N/A |
| cross-thread free handoff/large_8192 | 24504.317 | 53842.088 | 62200.687 | 22353.974 | N/A | N/A | 0.46x | 0.39x | 1.10x | N/A | N/A |
| cross-thread free handoff/medium_1024 | 17707.118 | 28034.571 | 23281.066 | 19390.488 | N/A | N/A | 0.63x | 0.76x | 0.91x | N/A | N/A |
| cross-thread free handoff/small_32 | 14907.947 | 26908.596 | 14895.799 | 15437.951 | N/A | N/A | 0.55x | 1.00x | 0.97x | N/A | N/A |
| leak detector allocator cycle latency/large_8192 | 828.008 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| leak detector allocator cycle latency/medium_1024 | 880.773 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| leak detector allocator cycle latency/small_32 | 790.661 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| realloc latency/cross_class_32_to_64 | 13.419 | 43.670 | 7.439 | 8.391 | N/A | N/A | 0.31x | 1.80x | 1.60x | N/A | N/A |
| realloc latency/cross_class_8k_to_16k | 121.124 | 143.555 | 97.140 | 117.569 | N/A | N/A | 0.84x | 1.25x | 1.03x | N/A | N/A |
| realloc latency/huge_shrink_4m_to_2m | 11275.553 | 932305.977 | 293.601 | 587354.479 | N/A | N/A | 0.01x | 38.40x | 0.02x | N/A | N/A |
| realloc latency/within_class_24_to_32 | 6.526 | 41.210 | 5.317 | 8.640 | N/A | N/A | 0.16x | 1.23x | 0.76x | N/A | N/A |
| realloc latency/within_class_6k_to_8k | 27.343 | 105.970 | 57.944 | 93.435 | N/A | N/A | 0.26x | 0.47x | 0.29x | N/A | N/A |
| segment cache eviction | 231416.954 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 14348.028 | 33085.041 | 18185.456 | 19669.685 | N/A | N/A | 0.43x | 0.79x | 0.73x | N/A | N/A |
| threaded saturated small allocation cycles | 91112.649 | 394454.519 | 75785.124 | 86181.786 | N/A | N/A | 0.23x | 1.20x | 1.06x | N/A | N/A |
| threaded small allocation cycles | 13848.113 | 32344.158 | 15258.939 | 14747.184 | N/A | N/A | 0.43x | 0.91x | 0.94x | N/A | N/A |
| usable size latency/huge_2m | 42.857 | N/A | 224.277 | N/A | N/A | N/A | N/A | 0.19x | N/A | N/A | N/A |
| usable size latency/large_8192 | 4.073 | N/A | 14.547 | N/A | N/A | N/A | N/A | 0.28x | N/A | N/A | N/A |
| usable size latency/medium_1024 | 5.804 | N/A | 5.660 | N/A | N/A | N/A | N/A | 1.03x | N/A | N/A | N/A |
| usable size latency/small_32 | 5.802 | N/A | 2.768 | N/A | N/A | N/A | N/A | 2.10x | N/A | N/A | N/A |
| usable size query latency/huge_2m | 0.498 | N/A | 0.568 | N/A | N/A | N/A | N/A | 0.88x | N/A | N/A | N/A |
| usable size query latency/large_8192 | 0.315 | N/A | 0.623 | N/A | N/A | N/A | N/A | 0.51x | N/A | N/A | N/A |
| usable size query latency/medium_1024 | 0.338 | N/A | 0.744 | N/A | N/A | N/A | N/A | 0.45x | N/A | N/A | N/A |
| usable size query latency/small_32 | 0.304 | N/A | 0.588 | N/A | N/A | N/A | N/A | 0.52x | N/A | N/A | N/A |
