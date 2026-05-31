# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 724.933 | 2680.653 | 3991.774 | N/A | 1750.219 | 0.27x | 0.18x | N/A | 0.41x |
| allocator allocation latency/large_8192 | 21.075 | 323.099 | 1181.771 | 394.286 | 95.525 | 0.07x | 0.02x | 0.05x | 0.22x |
| allocator allocation latency/medium_1024 | 11.002 | 53.796 | 258.879 | 65.095 | 30.353 | 0.20x | 0.04x | 0.17x | 0.36x |
| allocator allocation latency/small_32 | 11.348 | 23.829 | 16.274 | 16.045 | 12.928 | 0.48x | 0.70x | 0.71x | 0.88x |
| allocator burst retention/large_8192 | 2796.875 | 9118.970 | 405709.221 | 20520.156 | 26622.631 | 0.31x | 0.01x | 0.14x | 0.11x |
| allocator burst retention/medium_1024 | 954.288 | 6464.351 | 78237.303 | 7755.957 | 8917.637 | 0.15x | 0.01x | 0.12x | 0.11x |
| allocator burst retention/small_32 | 533.298 | 6584.112 | 833.771 | 4225.831 | 2617.825 | 0.08x | 0.64x | 0.13x | 0.20x |
| allocator cycle latency/huge_2m | 24.261 | 9069.220 | 10160.033 | N/A | 131.502 | 0.00x | 0.00x | N/A | 0.18x |
| allocator cycle latency/large_8192 | 9.776 | 28.917 | 21.489 | 22.986 | 17.778 | 0.34x | 0.45x | 0.43x | 0.55x |
| allocator cycle latency/medium_1024 | 9.104 | 23.668 | 6.752 | 20.735 | 8.833 | 0.38x | 1.35x | 0.44x | 1.03x |
| allocator cycle latency/small_32 | 8.729 | 26.128 | 4.856 | 23.421 | 8.122 | 0.33x | 1.80x | 0.37x | 1.07x |
| allocator deallocation latency/huge_2m | 1236.172 | 4080.969 | 4320.575 | N/A | 2859.849 | 0.30x | 0.29x | N/A | 0.43x |
| allocator deallocation latency/large_8192 | 16.188 | 62.403 | 474.558 | 168.568 | 47.640 | 0.26x | 0.03x | 0.10x | 0.34x |
| allocator deallocation latency/medium_1024 | 8.290 | 18.095 | 105.644 | 43.542 | 17.820 | 0.46x | 0.08x | 0.19x | 0.47x |
| allocator deallocation latency/small_32 | 3.037 | 10.210 | 5.407 | 9.364 | 6.557 | 0.30x | 0.56x | 0.32x | 0.46x |
| cross-thread free handoff/huge_2m | 1596.616 | 87774.083 | 103163.734 | N/A | 5831.227 | 0.02x | 0.02x | N/A | 0.27x |
| cross-thread free handoff/large_8192 | 27737.751 | 52715.427 | 864067.672 | 95695.757 | 77954.094 | 0.53x | 0.03x | 0.29x | 0.36x |
| cross-thread free handoff/medium_1024 | 17410.172 | 30583.664 | 142047.973 | 35953.765 | 36643.436 | 0.57x | 0.12x | 0.48x | 0.48x |
| cross-thread free handoff/small_32 | 14762.754 | 28340.043 | 16046.602 | 18614.913 | 25908.169 | 0.52x | 0.92x | 0.79x | 0.57x |
| realloc latency/cross_class_32_to_64 | 7.982 | 43.338 | 7.646 | 33.363 | 16.998 | 0.18x | 1.04x | 0.24x | 0.47x |
| realloc latency/cross_class_8k_to_16k | 47.246 | 137.324 | 67.096 | 135.189 | 54.237 | 0.34x | 0.70x | 0.35x | 0.87x |
| realloc latency/huge_shrink_4m_to_2m | 21.672 | 994832.245 | 7564.736 | 1047675.293 | 260.080 | 0.00x | 0.00x | 0.00x | 0.08x |
| realloc latency/within_class_24_to_32 | 3.504 | 42.998 | 4.362 | 17.136 | 15.408 | 0.08x | 0.80x | 0.20x | 0.23x |
| realloc latency/within_class_6k_to_8k | 23.775 | 100.783 | 64.566 | 123.362 | 66.188 | 0.24x | 0.37x | 0.19x | 0.36x |
| segment cache eviction | 211197.500 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 46228.397 | 350937.903 | 74618.960 | 270004.356 | 136829.306 | 0.13x | 0.62x | 0.17x | 0.34x |
| threaded small allocation cycles | 8783.600 | 64473.148 | 6518.214 | 24739.034 | 15786.646 | 0.14x | 1.35x | 0.36x | 0.56x |
| usable size latency/huge_2m | 21.321 | N/A | 8627.466 | N/A | 115.101 | N/A | 0.00x | N/A | 0.19x |
| usable size latency/large_8192 | 2.174 | N/A | 16.592 | 18.208 | 18.077 | N/A | 0.13x | 0.12x | 0.12x |
| usable size latency/medium_1024 | 3.093 | N/A | 5.980 | 16.866 | 10.439 | N/A | 0.52x | 0.18x | 0.30x |
| usable size latency/small_32 | 2.982 | N/A | 2.812 | 16.460 | 10.071 | N/A | 1.06x | 0.18x | 0.30x |
| usable size query latency/huge_2m | 0.406 | N/A | 0.551 | N/A | 3.228 | N/A | 0.74x | N/A | 0.13x |
| usable size query latency/large_8192 | 0.272 | N/A | 0.544 | 0.462 | 3.236 | N/A | 0.50x | 0.59x | 0.08x |
| usable size query latency/medium_1024 | 0.273 | N/A | 0.532 | 0.458 | 3.216 | N/A | 0.51x | 0.60x | 0.08x |
| usable size query latency/small_32 | 0.276 | N/A | 0.531 | 0.459 | 3.211 | N/A | 0.52x | 0.60x | 0.09x |
