# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 715.655 | 2676.054 | 4085.268 | N/A | 1732.458 | 0.27x | 0.18x | N/A | 0.41x |
| allocator allocation latency/large_8192 | 22.761 | 254.586 | 1200.910 | 422.870 | 75.731 | 0.09x | 0.02x | 0.05x | 0.30x |
| allocator allocation latency/medium_1024 | 11.116 | 92.285 | 265.338 | 89.187 | 29.162 | 0.12x | 0.04x | 0.12x | 0.38x |
| allocator allocation latency/small_32 | 9.543 | 20.902 | 14.261 | 13.571 | 12.898 | 0.46x | 0.67x | 0.70x | 0.74x |
| allocator burst retention/large_8192 | 2667.430 | 8739.237 | 395296.629 | 19625.598 | 26336.016 | 0.31x | 0.01x | 0.14x | 0.10x |
| allocator burst retention/medium_1024 | 1028.259 | 6917.562 | 76759.380 | 7906.421 | 8737.742 | 0.15x | 0.01x | 0.13x | 0.12x |
| allocator burst retention/small_32 | 605.698 | 6419.038 | 839.679 | 4227.527 | 2615.959 | 0.09x | 0.72x | 0.14x | 0.23x |
| allocator cycle latency/huge_2m | 21.875 | 7466.463 | 8483.326 | N/A | 115.016 | 0.00x | 0.00x | N/A | 0.19x |
| allocator cycle latency/large_8192 | 2.236 | 20.367 | 16.336 | 17.396 | 15.418 | 0.11x | 0.14x | 0.13x | 0.15x |
| allocator cycle latency/medium_1024 | 2.209 | 20.510 | 5.643 | 16.590 | 7.242 | 0.11x | 0.39x | 0.13x | 0.30x |
| allocator cycle latency/small_32 | 2.218 | 20.949 | 2.766 | 16.298 | 6.815 | 0.11x | 0.80x | 0.14x | 0.33x |
| allocator deallocation latency/huge_2m | 1238.985 | 3980.252 | 4305.280 | N/A | 3030.860 | 0.31x | 0.29x | N/A | 0.41x |
| allocator deallocation latency/large_8192 | 15.937 | 72.217 | 460.866 | 162.409 | 46.348 | 0.22x | 0.03x | 0.10x | 0.34x |
| allocator deallocation latency/medium_1024 | 8.746 | 19.717 | 69.441 | 36.942 | 16.857 | 0.44x | 0.13x | 0.24x | 0.52x |
| allocator deallocation latency/small_32 | 3.125 | 8.776 | 4.658 | 9.498 | 6.596 | 0.36x | 0.67x | 0.33x | 0.47x |
| cross-thread free handoff/huge_2m | 2357.041 | 135606.699 | 136003.288 | N/A | 7228.476 | 0.02x | 0.02x | N/A | 0.33x |
| cross-thread free handoff/large_8192 | 28347.903 | 54607.903 | 865529.556 | 94483.674 | 77239.485 | 0.52x | 0.03x | 0.30x | 0.37x |
| cross-thread free handoff/medium_1024 | 19895.731 | 34447.060 | 150288.402 | 38182.754 | 39076.890 | 0.58x | 0.13x | 0.52x | 0.51x |
| cross-thread free handoff/small_32 | 16837.003 | 30427.169 | 18237.876 | 21126.545 | 27554.410 | 0.55x | 0.92x | 0.80x | 0.61x |
| realloc latency/cross_class_32_to_64 | 6.871 | 42.829 | 7.542 | 32.801 | 16.928 | 0.16x | 0.91x | 0.21x | 0.41x |
| realloc latency/cross_class_8k_to_16k | 48.145 | 128.782 | 67.077 | 130.010 | 57.544 | 0.37x | 0.72x | 0.37x | 0.84x |
| realloc latency/huge_shrink_4m_to_2m | 22.405 | 953597.633 | 9032.371 | 1023116.143 | 248.343 | 0.00x | 0.00x | 0.00x | 0.09x |
| realloc latency/within_class_24_to_32 | 3.460 | 43.028 | 4.429 | 17.264 | 15.253 | 0.08x | 0.78x | 0.20x | 0.23x |
| realloc latency/within_class_6k_to_8k | 25.080 | 102.278 | 56.062 | 96.022 | 52.248 | 0.25x | 0.45x | 0.26x | 0.48x |
| segment cache eviction | 205531.743 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 52228.122 | 348464.764 | 61722.806 | 265260.855 | 128288.271 | 0.15x | 0.85x | 0.20x | 0.41x |
| threaded small allocation cycles | 11163.995 | 31924.990 | 13151.721 | 25524.838 | 17486.846 | 0.35x | 0.85x | 0.44x | 0.64x |
| usable size latency/huge_2m | 22.439 | N/A | 8472.338 | N/A | 116.998 | N/A | 0.00x | N/A | 0.19x |
| usable size latency/large_8192 | 2.462 | N/A | 16.592 | 17.787 | 17.806 | N/A | 0.15x | 0.14x | 0.14x |
| usable size latency/medium_1024 | 3.394 | N/A | 5.978 | 17.050 | 10.254 | N/A | 0.57x | 0.20x | 0.33x |
| usable size latency/small_32 | 3.388 | N/A | 2.843 | 16.507 | 9.912 | N/A | 1.19x | 0.21x | 0.34x |
| usable size query latency/huge_2m | 0.401 | N/A | 0.553 | N/A | 3.199 | N/A | 0.73x | N/A | 0.13x |
| usable size query latency/large_8192 | 0.311 | N/A | 0.530 | 0.456 | 3.198 | N/A | 0.59x | 0.68x | 0.10x |
| usable size query latency/medium_1024 | 0.320 | N/A | 0.534 | 0.453 | 3.200 | N/A | 0.60x | 0.71x | 0.10x |
| usable size query latency/small_32 | 0.322 | N/A | 0.530 | 0.457 | 3.212 | N/A | 0.61x | 0.70x | 0.10x |
