# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 2728.030 | 2862.314 | 4349.634 | 2375.928 | N/A | 0.95x | 0.63x | 1.15x | N/A |
| allocator allocation latency/large_8192 | 176.922 | 889.023 | 1202.219 | 707.940 | N/A | 0.20x | 0.15x | 0.25x | N/A |
| allocator allocation latency/medium_1024 | 20.987 | 187.278 | 299.902 | 152.654 | N/A | 0.11x | 0.07x | 0.14x | N/A |
| allocator allocation latency/small_32 | 9.704 | 33.018 | 15.547 | 18.428 | N/A | 0.29x | 0.62x | 0.53x | N/A |
| allocator burst retention/large_8192 | 3143.149 | 10386.200 | 430983.984 | 20471.814 | N/A | 0.30x | 0.01x | 0.15x | N/A |
| allocator burst retention/medium_1024 | 1079.274 | 7019.873 | 81031.494 | 7722.546 | N/A | 0.15x | 0.01x | 0.14x | N/A |
| allocator burst retention/small_32 | 638.924 | 6605.914 | 859.682 | 4397.147 | N/A | 0.10x | 0.74x | 0.15x | N/A |
| allocator cycle latency/huge_2m | 21.068 | 8237.820 | 10094.952 | 5366.031 | N/A | 0.00x | 0.00x | 0.00x | N/A |
| allocator cycle latency/large_8192 | 3.044 | 22.158 | 16.806 | 17.745 | N/A | 0.14x | 0.18x | 0.17x | N/A |
| allocator cycle latency/medium_1024 | 2.773 | 20.723 | 6.193 | 16.112 | N/A | 0.13x | 0.45x | 0.17x | N/A |
| allocator cycle latency/small_32 | 2.879 | 21.040 | 3.008 | 15.227 | N/A | 0.14x | 0.96x | 0.19x | N/A |
| allocator deallocation latency/huge_2m | 4711.919 | 4497.635 | 4942.584 | 4929.480 | N/A | 1.05x | 0.95x | 0.96x | N/A |
| allocator deallocation latency/large_8192 | 95.324 | 324.008 | 721.311 | 90.974 | N/A | 0.29x | 0.13x | 1.05x | N/A |
| allocator deallocation latency/medium_1024 | 23.093 | 68.462 | 125.993 | 33.963 | N/A | 0.34x | 0.18x | 0.68x | N/A |
| allocator deallocation latency/small_32 | 3.761 | 18.863 | 6.535 | 10.375 | N/A | 0.20x | 0.58x | 0.36x | N/A |
| cross-thread free handoff/huge_2m | 1228.051 | 71896.436 | 90664.355 | 90573.584 | N/A | 0.02x | 0.01x | 0.01x | N/A |
| cross-thread free handoff/large_8192 | 23589.587 | 52774.414 | 955807.031 | 91847.314 | N/A | 0.45x | 0.02x | 0.26x | N/A |
| cross-thread free handoff/medium_1024 | 19717.529 | 33670.801 | 149233.691 | 37071.277 | N/A | 0.59x | 0.13x | 0.53x | N/A |
| cross-thread free handoff/small_32 | 16209.937 | 31246.240 | 16253.470 | 20881.033 | N/A | 0.52x | 1.00x | 0.78x | N/A |
| realloc latency/cross_class_32_to_64 | 8.157 | 48.583 | 7.628 | 33.061 | N/A | 0.17x | 1.07x | 0.25x | N/A |
| realloc latency/cross_class_8k_to_16k | 47.963 | 144.046 | 66.969 | 130.092 | N/A | 0.33x | 0.72x | 0.37x | N/A |
| realloc latency/huge_shrink_4m_to_2m | 22.841 | 1005301.562 | 8153.845 | 1081085.156 | N/A | 0.00x | 0.00x | 0.00x | N/A |
| realloc latency/within_class_24_to_32 | 4.162 | 48.676 | 4.464 | 17.144 | N/A | 0.09x | 0.93x | 0.24x | N/A |
| realloc latency/within_class_6k_to_8k | 27.335 | 105.308 | 56.281 | 96.254 | N/A | 0.26x | 0.49x | 0.28x | N/A |
| segment cache eviction | 237957.227 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 7086.163 | 32310.620 | 15826.062 | 27488.501 | N/A | 0.22x | 0.45x | 0.26x | N/A |
| threaded saturated small allocation cycles | 69441.748 | 372763.672 | 74054.736 | 273019.531 | N/A | 0.19x | 0.94x | 0.25x | N/A |
| threaded small allocation cycles | 7530.447 | 32313.916 | 5220.370 | 26394.385 | N/A | 0.23x | 1.44x | 0.29x | N/A |
| usable size latency/huge_2m | 22.082 | N/A | 8222.906 | 5692.157 | N/A | N/A | 0.00x | 0.00x | N/A |
| usable size latency/large_8192 | 2.888 | N/A | 16.600 | 17.690 | N/A | N/A | 0.17x | 0.16x | N/A |
| usable size latency/medium_1024 | 4.095 | N/A | 6.320 | 16.897 | N/A | N/A | 0.65x | 0.24x | N/A |
| usable size latency/small_32 | 4.148 | N/A | 2.964 | 16.415 | N/A | N/A | 1.40x | 0.25x | N/A |
| usable size query latency/huge_2m | 0.349 | N/A | 0.528 | 0.460 | N/A | N/A | 0.66x | 0.76x | N/A |
| usable size query latency/large_8192 | 0.275 | N/A | 0.526 | 0.456 | N/A | N/A | 0.52x | 0.60x | N/A |
| usable size query latency/medium_1024 | 0.283 | N/A | 0.533 | 0.455 | N/A | N/A | 0.53x | 0.62x | N/A |
| usable size query latency/small_32 | 0.293 | N/A | 0.535 | 0.448 | N/A | N/A | 0.55x | 0.65x | N/A |
