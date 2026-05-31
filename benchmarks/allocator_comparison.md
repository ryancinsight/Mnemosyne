# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 507.992 | 3052.384 | 4339.177 | N/A | 1722.316 | 0.17x | 0.12x | N/A | 0.29x |
| allocator allocation latency/large_8192 | 22.477 | 272.493 | 1404.174 | 455.125 | 75.789 | 0.08x | 0.02x | 0.05x | 0.30x |
| allocator allocation latency/medium_1024 | 11.397 | 58.484 | 286.767 | 92.376 | 27.846 | 0.19x | 0.04x | 0.12x | 0.41x |
| allocator allocation latency/small_32 | 9.147 | 21.799 | 15.434 | 13.703 | 12.685 | 0.42x | 0.59x | 0.67x | 0.72x |
| allocator burst retention/large_8192 | 2639.885 | 8879.757 | 407991.512 | 21026.129 | 26806.924 | 0.30x | 0.01x | 0.13x | 0.10x |
| allocator burst retention/medium_1024 | 1069.685 | 7671.921 | 97610.374 | 7757.894 | 9189.794 | 0.14x | 0.01x | 0.14x | 0.12x |
| allocator burst retention/small_32 | 697.850 | 6327.288 | 880.593 | 4128.631 | 2595.998 | 0.11x | 0.79x | 0.17x | 0.27x |
| allocator cycle latency/huge_2m | 20.513 | 7777.913 | 11072.912 | N/A | 115.639 | 0.00x | 0.00x | N/A | 0.18x |
| allocator cycle latency/large_8192 | 2.048 | 20.680 | 15.092 | 18.326 | 15.530 | 0.10x | 0.14x | 0.11x | 0.13x |
| allocator cycle latency/medium_1024 | 2.301 | 20.452 | 5.664 | 16.647 | 7.278 | 0.11x | 0.41x | 0.14x | 0.32x |
| allocator cycle latency/small_32 | 2.022 | 20.114 | 2.778 | 16.067 | 6.901 | 0.10x | 0.73x | 0.13x | 0.29x |
| allocator deallocation latency/huge_2m | 1057.672 | 4141.863 | 4606.557 | N/A | 3001.056 | 0.26x | 0.23x | N/A | 0.35x |
| allocator deallocation latency/large_8192 | 16.970 | 85.601 | 460.617 | 157.646 | 47.002 | 0.20x | 0.04x | 0.11x | 0.36x |
| allocator deallocation latency/medium_1024 | 8.695 | 21.897 | 88.381 | 46.575 | 18.108 | 0.40x | 0.10x | 0.19x | 0.48x |
| allocator deallocation latency/small_32 | 3.200 | 9.284 | 5.269 | 9.740 | 6.511 | 0.34x | 0.61x | 0.33x | 0.49x |
| cross-thread free handoff/huge_2m | 838.925 | 103570.096 | 121928.780 | N/A | 5208.736 | 0.01x | 0.01x | N/A | 0.16x |
| cross-thread free handoff/large_8192 | 28676.483 | 58087.086 | 962915.108 | 93994.044 | 81419.555 | 0.49x | 0.03x | 0.31x | 0.35x |
| cross-thread free handoff/medium_1024 | 12912.053 | 33762.078 | 196511.139 | 38759.876 | 44235.981 | 0.38x | 0.07x | 0.33x | 0.29x |
| cross-thread free handoff/small_32 | 9619.403 | 35421.850 | 11596.595 | 21097.852 | 32499.430 | 0.27x | 0.83x | 0.46x | 0.30x |
| realloc latency/cross_class_32_to_64 | 8.492 | 52.314 | 10.988 | 32.666 | 17.032 | 0.16x | 0.77x | 0.26x | 0.50x |
| realloc latency/cross_class_8k_to_16k | 68.056 | 144.769 | 93.697 | 145.268 | 61.556 | 0.47x | 0.73x | 0.47x | 1.11x |
| realloc latency/huge_shrink_4m_to_2m | 20.257 | 1184321.167 | 10912.084 | 1123452.879 | 251.877 | 0.00x | 0.00x | 0.00x | 0.08x |
| realloc latency/within_class_24_to_32 | 3.850 | 53.561 | 4.793 | 17.039 | 22.998 | 0.07x | 0.80x | 0.23x | 0.17x |
| realloc latency/within_class_6k_to_8k | 34.938 | 117.364 | 69.954 | 109.980 | 53.091 | 0.30x | 0.50x | 0.32x | 0.66x |
| segment cache eviction | 240363.185 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 68105.848 | 419163.933 | 80387.854 | 279144.940 | 145268.548 | 0.16x | 0.85x | 0.24x | 0.47x |
| threaded small allocation cycles | 7968.016 | 35359.124 | 10924.757 | 28673.009 | 19429.154 | 0.23x | 0.73x | 0.28x | 0.41x |
| usable size latency/huge_2m | 21.305 | N/A | 7887.720 | N/A | 118.022 | N/A | 0.00x | N/A | 0.18x |
| usable size latency/large_8192 | 2.548 | N/A | 16.569 | 17.664 | 17.537 | N/A | 0.15x | 0.14x | 0.15x |
| usable size latency/medium_1024 | 3.596 | N/A | 6.350 | 17.563 | 10.484 | N/A | 0.57x | 0.20x | 0.34x |
| usable size latency/small_32 | 3.436 | N/A | 3.018 | 16.250 | 9.922 | N/A | 1.14x | 0.21x | 0.35x |
| usable size query latency/huge_2m | 0.559 | N/A | 0.560 | N/A | 3.224 | N/A | 1.00x | N/A | 0.17x |
| usable size query latency/large_8192 | 0.308 | N/A | 0.536 | 0.474 | 3.264 | N/A | 0.57x | 0.65x | 0.09x |
| usable size query latency/medium_1024 | 0.312 | N/A | 0.553 | 0.458 | 3.316 | N/A | 0.56x | 0.68x | 0.09x |
| usable size query latency/small_32 | 0.343 | N/A | 0.661 | 0.461 | 3.298 | N/A | 0.52x | 0.74x | 0.10x |
