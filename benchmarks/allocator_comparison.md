# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 536.491 | 3646.201 | 5437.918 | 2158.706 | 1973.089 | 0.15x | 0.10x | 0.25x | 0.27x |
| allocator allocation latency/large_8192 | 70.788 | 285.263 | 1379.868 | 482.034 | 92.211 | 0.25x | 0.05x | 0.15x | 0.77x |
| allocator allocation latency/medium_1024 | 12.959 | 59.402 | 253.000 | 54.304 | 31.952 | 0.22x | 0.05x | 0.24x | 0.41x |
| allocator allocation latency/small_32 | 7.806 | 21.873 | 15.724 | 13.104 | 12.798 | 0.36x | 0.50x | 0.60x | 0.61x |
| allocator burst retention/large_8192 | 2305.739 | 11355.081 | 508322.880 | 21542.378 | 31798.246 | 0.20x | 0.00x | 0.11x | 0.07x |
| allocator burst retention/medium_1024 | 1705.984 | 10159.787 | 109679.342 | 10712.594 | 11240.491 | 0.17x | 0.02x | 0.16x | 0.15x |
| allocator burst retention/small_32 | 623.488 | 7291.820 | 1274.036 | 4687.946 | 3377.508 | 0.09x | 0.49x | 0.13x | 0.18x |
| allocator cycle latency/huge_2m | 20.500 | 9475.591 | 13636.105 | 8865.348 | 118.481 | 0.00x | 0.00x | 0.00x | 0.17x |
| allocator cycle latency/large_8192 | 2.664 | 22.948 | 15.358 | 18.149 | 17.649 | 0.12x | 0.17x | 0.15x | 0.15x |
| allocator cycle latency/medium_1024 | 3.110 | 23.230 | 6.248 | 16.967 | 7.923 | 0.13x | 0.50x | 0.18x | 0.39x |
| allocator cycle latency/small_32 | 2.667 | 20.644 | 2.844 | 17.249 | 8.739 | 0.13x | 0.94x | 0.15x | 0.31x |
| allocator deallocation latency/huge_2m | 1025.603 | 4938.439 | 6989.883 | 6003.886 | 4508.246 | 0.21x | 0.15x | 0.17x | 0.23x |
| allocator deallocation latency/large_8192 | 33.556 | 164.736 | 607.941 | 177.076 | 57.479 | 0.20x | 0.06x | 0.19x | 0.58x |
| allocator deallocation latency/medium_1024 | 11.152 | 20.950 | 120.109 | 41.860 | 21.593 | 0.53x | 0.09x | 0.27x | 0.52x |
| allocator deallocation latency/small_32 | 3.194 | 13.710 | 6.582 | 9.534 | 7.648 | 0.23x | 0.49x | 0.34x | 0.42x |
| cross-thread free handoff/huge_2m | 2583.497 | 120302.814 | 116206.361 | 111892.937 | 10300.972 | 0.02x | 0.02x | 0.02x | 0.25x |
| cross-thread free handoff/large_8192 | 43656.877 | 78285.871 | 1097814.738 | 130706.152 | 104673.762 | 0.56x | 0.04x | 0.33x | 0.42x |
| cross-thread free handoff/medium_1024 | 18267.107 | 47936.723 | 191391.340 | 54894.012 | 60959.670 | 0.38x | 0.10x | 0.33x | 0.30x |
| cross-thread free handoff/small_32 | 14116.259 | 48448.871 | 20891.995 | 31687.366 | 43591.195 | 0.29x | 0.68x | 0.45x | 0.32x |
| realloc latency/cross_class_32_to_64 | 9.607 | 55.216 | 12.203 | 34.048 | 26.338 | 0.17x | 0.79x | 0.28x | 0.36x |
| realloc latency/cross_class_8k_to_16k | 79.155 | 166.931 | 169.426 | 151.425 | 87.651 | 0.47x | 0.47x | 0.52x | 0.90x |
| realloc latency/huge_shrink_4m_to_2m | 19.680 | 970953.227 | 10787.459 | 1020113.447 | 273.680 | 0.00x | 0.00x | 0.00x | 0.07x |
| realloc latency/within_class_24_to_32 | 6.037 | 56.568 | 5.687 | 18.311 | 21.489 | 0.11x | 1.06x | 0.33x | 0.28x |
| realloc latency/within_class_6k_to_8k | 49.597 | 113.828 | 73.852 | 126.456 | 77.425 | 0.44x | 0.67x | 0.39x | 0.64x |
| segment cache eviction | 231880.531 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 36901.209 | 56437.753 | 43393.006 | 50151.257 | 47222.257 | 0.65x | 0.85x | 0.74x | 0.78x |
| threaded saturated small allocation cycles | 88056.820 | 432327.230 | 95892.572 | 296573.945 | 173583.794 | 0.20x | 0.92x | 0.30x | 0.51x |
| threaded small allocation cycles | 4528.951 | 58477.916 | 37272.908 | 48434.891 | 42093.652 | 0.08x | 0.12x | 0.09x | 0.11x |
| usable size latency/huge_2m | 22.272 | N/A | 17440.206 | 6365.931 | 122.802 | N/A | 0.00x | 0.00x | 0.18x |
| usable size latency/large_8192 | 3.375 | N/A | 16.947 | 19.966 | 25.835 | N/A | 0.20x | 0.17x | 0.13x |
| usable size latency/medium_1024 | 4.646 | N/A | 6.507 | 17.865 | 11.812 | N/A | 0.71x | 0.26x | 0.39x |
| usable size latency/small_32 | 2.821 | N/A | 4.580 | 17.246 | 11.096 | N/A | 0.62x | 0.16x | 0.25x |
| usable size query latency/huge_2m | 0.542 | N/A | 1.057 | 0.700 | 4.219 | N/A | 0.51x | 0.77x | 0.13x |
| usable size query latency/large_8192 | 0.389 | N/A | 0.883 | 0.668 | 3.535 | N/A | 0.44x | 0.58x | 0.11x |
| usable size query latency/medium_1024 | 0.421 | N/A | 0.974 | 0.637 | 3.439 | N/A | 0.43x | 0.66x | 0.12x |
| usable size query latency/small_32 | 0.271 | N/A | 0.545 | 0.524 | 3.547 | N/A | 0.50x | 0.52x | 0.08x |
