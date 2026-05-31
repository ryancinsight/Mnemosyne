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
| cross-thread free handoff/huge_2m | 1836.475 | 126629.366 | 123828.186 | N/A | 7573.986 | 0.01x | 0.01x | N/A | 0.24x |
| cross-thread free handoff/large_8192 | 31631.125 | 69320.498 | 991135.920 | 109641.151 | 98625.424 | 0.46x | 0.03x | 0.29x | 0.32x |
| cross-thread free handoff/medium_1024 | 16795.855 | 37836.751 | 150611.464 | 35235.228 | 44645.433 | 0.44x | 0.11x | 0.48x | 0.38x |
| cross-thread free handoff/small_32 | 14291.042 | 29643.015 | 12282.700 | 21281.238 | 29140.143 | 0.48x | 1.16x | 0.67x | 0.49x |
| realloc latency/cross_class_32_to_64 | 9.222 | 42.420 | 8.696 | 32.905 | 17.053 | 0.22x | 1.06x | 0.28x | 0.54x |
| realloc latency/cross_class_8k_to_16k | 47.047 | 128.926 | 66.917 | 131.139 | 54.636 | 0.36x | 0.70x | 0.36x | 0.86x |
| realloc latency/huge_shrink_4m_to_2m | 22.153 | 977276.631 | 7663.930 | 1024728.574 | 248.334 | 0.00x | 0.00x | 0.00x | 0.09x |
| realloc latency/within_class_24_to_32 | 3.421 | 42.285 | 4.627 | 17.026 | 15.355 | 0.08x | 0.74x | 0.20x | 0.22x |
| realloc latency/within_class_6k_to_8k | 24.345 | 99.731 | 56.002 | 98.663 | 51.181 | 0.24x | 0.43x | 0.25x | 0.48x |
| segment cache eviction | 284524.099 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 91393.445 | 546703.531 | 115484.066 | 360365.344 | 207357.210 | 0.17x | 0.79x | 0.25x | 0.44x |
| threaded small allocation cycles | 5994.001 | 41352.280 | 9953.161 | 25925.546 | 20431.256 | 0.14x | 0.60x | 0.23x | 0.29x |
| usable size latency/huge_2m | 21.321 | N/A | 8627.466 | N/A | 115.101 | N/A | 0.00x | N/A | 0.19x |
| usable size latency/large_8192 | 2.174 | N/A | 16.592 | 18.208 | 18.077 | N/A | 0.13x | 0.12x | 0.12x |
| usable size latency/medium_1024 | 3.093 | N/A | 5.980 | 16.866 | 10.439 | N/A | 0.52x | 0.18x | 0.30x |
| usable size latency/small_32 | 2.982 | N/A | 2.812 | 16.460 | 10.071 | N/A | 1.06x | 0.18x | 0.30x |
| usable size query latency/huge_2m | 0.402 | N/A | 0.528 | N/A | 3.194 | N/A | 0.76x | N/A | 0.13x |
| usable size query latency/large_8192 | 0.313 | N/A | 0.527 | 0.451 | 3.196 | N/A | 0.59x | 0.69x | 0.10x |
| usable size query latency/medium_1024 | 0.324 | N/A | 0.529 | 0.455 | 3.205 | N/A | 0.61x | 0.71x | 0.10x |
| usable size query latency/small_32 | 0.276 | N/A | 0.522 | 0.460 | 3.214 | N/A | 0.53x | 0.60x | 0.09x |
