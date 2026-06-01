# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MnemosyneHeap (ns) | BrandedHeap (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc | MnemosyneHeap vs Mnemosyne | BrandedHeap vs Mnemosyne |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 2569.389 | N/A | N/A | 2591.138 | 4093.445 | N/A | 1732.458 | 0.99x | 0.63x | N/A | 1.48x | N/A | N/A |
| allocator allocation latency/large_8192 | 59.891 | N/A | N/A | 813.398 | 1153.267 | 712.307 | 75.731 | 0.07x | 0.05x | 0.08x | 0.79x | N/A | N/A |
| allocator allocation latency/medium_1024 | 30.248 | N/A | N/A | 166.602 | 239.024 | 112.244 | 29.162 | 0.18x | 0.13x | 0.27x | 1.04x | N/A | N/A |
| allocator allocation latency/small_32 | 10.315 | N/A | N/A | 31.824 | 14.958 | 16.993 | 12.898 | 0.32x | 0.69x | 0.61x | 0.80x | N/A | N/A |
| allocator burst retention/large_8192 | 3195.688 | N/A | N/A | 8408.917 | 391459.375 | 20339.941 | 26336.016 | 0.38x | 0.01x | 0.16x | 0.12x | N/A | N/A |
| allocator burst retention/medium_1024 | 1281.437 | N/A | N/A | 7367.365 | 80279.175 | 7678.754 | 8737.742 | 0.17x | 0.02x | 0.17x | 0.15x | N/A | N/A |
| allocator burst retention/small_32 | 887.909 | N/A | N/A | 6388.953 | 817.400 | 4210.999 | 2615.959 | 0.14x | 1.09x | 0.21x | 0.34x | N/A | N/A |
| allocator cycle latency/huge_2m | 22.348 | 22.047 | 22.406 | 7583.295 | 8368.005 | N/A | 115.016 | 0.00x | 0.00x | N/A | 0.19x | 0.99x | 1.00x |
| allocator cycle latency/large_8192 | 2.828 | 2.134 | 2.347 | 21.782 | 16.763 | 17.392 | 15.418 | 0.13x | 0.17x | 0.16x | 0.18x | 0.75x | 0.83x |
| allocator cycle latency/medium_1024 | 2.815 | 2.132 | 2.350 | 20.257 | 5.628 | 16.620 | 7.242 | 0.14x | 0.50x | 0.17x | 0.39x | 0.76x | 0.83x |
| allocator cycle latency/small_32 | 2.834 | 2.128 | 2.350 | 19.999 | 2.732 | 14.917 | 6.815 | 0.14x | 1.04x | 0.19x | 0.42x | 0.75x | 0.83x |
| allocator deallocation latency/huge_2m | 4276.167 | N/A | N/A | 3994.812 | 4262.659 | N/A | 3030.860 | 1.07x | 1.00x | N/A | 1.41x | N/A | N/A |
| allocator deallocation latency/large_8192 | 270.955 | N/A | N/A | 233.974 | 489.042 | 254.376 | 46.348 | 1.16x | 0.55x | 1.07x | 5.85x | N/A | N/A |
| allocator deallocation latency/medium_1024 | 29.502 | N/A | N/A | 87.030 | 95.479 | 61.540 | 16.857 | 0.34x | 0.31x | 0.48x | 1.75x | N/A | N/A |
| allocator deallocation latency/small_32 | 5.930 | N/A | N/A | 14.581 | 5.010 | 9.381 | 6.596 | 0.41x | 1.18x | 0.63x | 0.90x | N/A | N/A |
| cross-thread free handoff/huge_2m | 1536.378 | N/A | N/A | 87051.221 | 102643.848 | N/A | 7228.476 | 0.02x | 0.01x | N/A | 0.21x | N/A | N/A |
| cross-thread free handoff/large_8192 | 26213.940 | N/A | N/A | 53200.134 | 853485.938 | 93404.346 | 77239.485 | 0.49x | 0.03x | 0.28x | 0.34x | N/A | N/A |
| cross-thread free handoff/medium_1024 | 17822.510 | N/A | N/A | 32139.404 | 139281.348 | 36440.674 | 39076.890 | 0.55x | 0.13x | 0.49x | 0.46x | N/A | N/A |
| cross-thread free handoff/small_32 | 15552.185 | N/A | N/A | 28203.442 | 16144.739 | 19012.195 | 27554.410 | 0.55x | 0.96x | 0.82x | 0.56x | N/A | N/A |
| realloc latency/cross_class_32_to_64 | 8.043 | N/A | N/A | 42.542 | 8.151 | 32.812 | 16.928 | 0.19x | 0.99x | 0.25x | 0.48x | N/A | N/A |
| realloc latency/cross_class_8k_to_16k | 49.966 | N/A | N/A | 132.111 | 71.193 | 131.495 | 57.544 | 0.38x | 0.70x | 0.38x | 0.87x | N/A | N/A |
| realloc latency/huge_shrink_4m_to_2m | 22.039 | N/A | N/A | 987839.844 | 7575.275 | 1020445.312 | 248.343 | 0.00x | 0.00x | 0.00x | 0.09x | N/A | N/A |
| realloc latency/within_class_24_to_32 | 4.419 | N/A | N/A | 42.910 | 4.375 | 17.126 | 15.253 | 0.10x | 1.01x | 0.26x | 0.29x | N/A | N/A |
| realloc latency/within_class_6k_to_8k | 25.101 | N/A | N/A | 99.386 | 55.738 | 94.959 | 52.248 | 0.25x | 0.45x | 0.26x | 0.48x | N/A | N/A |
| segment cache eviction | 202785.742 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 13232.153 | N/A | N/A | 30427.222 | 15948.511 | 24660.461 | 18411.964 | 0.43x | 0.83x | 0.54x | 0.72x | N/A | N/A |
| threaded saturated small allocation cycles | 65480.518 | N/A | N/A | 344640.039 | 65035.840 | 267118.164 | 128288.271 | 0.19x | 1.01x | 0.25x | 0.51x | N/A | N/A |
| threaded small allocation cycles | 10246.313 | N/A | N/A | 28560.620 | 6743.256 | 23400.208 | 17486.846 | 0.36x | 1.52x | 0.44x | 0.59x | N/A | N/A |
| usable size latency/huge_2m | 22.167 | N/A | N/A | N/A | 8934.326 | N/A | 116.998 | N/A | 0.00x | N/A | 0.19x | N/A | N/A |
| usable size latency/large_8192 | 3.450 | N/A | N/A | N/A | 16.501 | 17.552 | 17.806 | N/A | 0.21x | 0.20x | 0.19x | N/A | N/A |
| usable size latency/medium_1024 | 4.126 | N/A | N/A | N/A | 6.719 | 16.734 | 10.254 | N/A | 0.61x | 0.25x | 0.40x | N/A | N/A |
| usable size latency/small_32 | 4.143 | N/A | N/A | N/A | 2.798 | 16.484 | 9.912 | N/A | 1.48x | 0.25x | 0.42x | N/A | N/A |
| usable size query latency/huge_2m | 0.345 | N/A | N/A | N/A | 0.528 | N/A | 3.199 | N/A | 0.65x | N/A | 0.11x | N/A | N/A |
| usable size query latency/large_8192 | 0.263 | N/A | N/A | N/A | 0.527 | 0.451 | 3.198 | N/A | 0.50x | 0.58x | 0.08x | N/A | N/A |
| usable size query latency/medium_1024 | 0.265 | N/A | N/A | N/A | 0.530 | 0.450 | 3.200 | N/A | 0.50x | 0.59x | 0.08x | N/A | N/A |
| usable size query latency/small_32 | 0.261 | N/A | N/A | N/A | 0.529 | 0.449 | 3.212 | N/A | 0.49x | 0.58x | 0.08x | N/A | N/A |
