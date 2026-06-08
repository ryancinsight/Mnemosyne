# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | RpMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs RpMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 499.686 | 3598.447 | 5798.025 | 1592.667 | 2839.288 | 2582.927 | 0.14x | 0.09x | 0.31x | 0.18x | 0.19x |
| allocator allocation latency/large_8192 | 42.678 | 251.177 | 1727.318 | 19.026 | 344.651 | 99.651 | 0.17x | 0.02x | 2.24x | 0.12x | 0.43x |
| allocator allocation latency/medium_1024 | 13.763 | 108.935 | 385.622 | 40.395 | 149.332 | 53.842 | 0.13x | 0.04x | 0.34x | 0.09x | 0.26x |
| allocator allocation latency/small_32 | 9.979 | 21.892 | 18.557 | 14.226 | 16.674 | 16.807 | 0.46x | 0.54x | 0.70x | 0.60x | 0.59x |
| allocator burst retention/large_8192 | 2686.985 | 13071.555 | 615630.872 | 3709.600 | 25416.118 | 39005.246 | 0.21x | 0.00x | 0.72x | 0.11x | 0.07x |
| allocator burst retention/medium_1024 | 1052.330 | 8053.053 | 115003.136 | 3569.989 | 8126.511 | 10914.167 | 0.13x | 0.01x | 0.29x | 0.13x | 0.10x |
| allocator burst retention/small_32 | 591.601 | 8345.407 | 1075.881 | 3121.830 | 5526.720 | 3439.732 | 0.07x | 0.55x | 0.19x | 0.11x | 0.17x |
| allocator cycle latency/huge_2m | 29.336 | 10229.943 | 12232.593 | 13.298 | 7506.143 | 129.702 | 0.00x | 0.00x | 2.21x | 0.00x | 0.23x |
| allocator cycle latency/large_8192 | 2.136 | 24.355 | 17.558 | 11.232 | 17.905 | 19.273 | 0.09x | 0.12x | 0.19x | 0.12x | 0.11x |
| allocator cycle latency/medium_1024 | 2.278 | 29.878 | 6.871 | 11.480 | 17.999 | 9.989 | 0.08x | 0.33x | 0.20x | 0.13x | 0.23x |
| allocator cycle latency/small_32 | 2.068 | 23.090 | 3.871 | 10.950 | 18.306 | 9.918 | 0.09x | 0.53x | 0.19x | 0.11x | 0.21x |
| allocator deallocation latency/huge_2m | 911.914 | 5893.984 | 7118.871 | 1721.399 | 5845.850 | 4949.679 | 0.15x | 0.13x | 0.53x | 0.16x | 0.18x |
| allocator deallocation latency/large_8192 | 40.909 | 190.180 | 796.876 | 6.871 | 122.139 | 63.324 | 0.22x | 0.05x | 5.95x | 0.33x | 0.65x |
| allocator deallocation latency/medium_1024 | 10.023 | 22.699 | 142.231 | 9.314 | 36.483 | 33.397 | 0.44x | 0.07x | 1.08x | 0.27x | 0.30x |
| allocator deallocation latency/small_32 | 3.541 | 20.155 | 7.581 | 2.614 | 10.146 | 10.114 | 0.18x | 0.47x | 1.35x | 0.35x | 0.35x |
| cross-thread free handoff/huge_2m | 1348.774 | 111233.360 | 126430.668 | 1418.066 | 105232.239 | 4950.147 | 0.01x | 0.01x | 0.95x | 0.01x | 0.27x |
| cross-thread free handoff/large_8192 | 36894.844 | 80126.989 | 1216257.149 | 30086.943 | 131560.487 | 84088.558 | 0.46x | 0.03x | 1.23x | 0.28x | 0.44x |
| cross-thread free handoff/medium_1024 | 16110.732 | 34532.840 | 167640.559 | 31104.247 | 44016.111 | 50979.747 | 0.47x | 0.10x | 0.52x | 0.37x | 0.32x |
| cross-thread free handoff/small_32 | 11889.856 | 38304.881 | 15234.193 | 22547.697 | 23587.075 | 27118.771 | 0.31x | 0.78x | 0.53x | 0.50x | 0.44x |
| realloc latency/cross_class_32_to_64 | 8.598 | 49.063 | 8.558 | 24.942 | 35.640 | 17.758 | 0.18x | 1.00x | 0.34x | 0.24x | 0.48x |
| realloc latency/cross_class_8k_to_16k | 58.379 | 143.873 | 72.959 | 161.870 | 166.967 | 61.349 | 0.41x | 0.80x | 0.36x | 0.35x | 0.95x |
| realloc latency/huge_shrink_4m_to_2m | 30.146 | 1111614.563 | 10427.957 | 775355.766 | 1106926.963 | 258.775 | 0.00x | 0.00x | 0.00x | 0.00x | 0.12x |
| realloc latency/within_class_24_to_32 | 4.743 | 77.318 | 14.828 | 24.623 | 22.996 | 20.726 | 0.06x | 0.32x | 0.19x | 0.21x | 0.23x |
| realloc latency/within_class_6k_to_8k | 29.373 | 114.356 | 60.940 | 125.810 | 101.601 | 57.286 | 0.26x | 0.48x | 0.23x | 0.29x | 0.51x |
| segment cache eviction | 251201.804 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 10990.446 | 32136.560 | 12647.735 | 17243.193 | 40443.313 | 19324.531 | 0.34x | 0.87x | 0.64x | 0.27x | 0.57x |
| threaded saturated small allocation cycles | 60613.544 | 742689.584 | 101800.246 | 186017.600 | 372178.643 | 324933.933 | 0.08x | 0.60x | 0.33x | 0.16x | 0.19x |
| threaded small allocation cycles | 9079.876 | 31506.057 | 7026.878 | 17270.568 | 26709.558 | 15249.947 | 0.29x | 1.29x | 0.53x | 0.34x | 0.60x |
| usable size latency/huge_2m | 51.311 | N/A | 16778.027 | N/A | 8614.773 | 314.310 | N/A | 0.00x | N/A | 0.01x | 0.16x |
| usable size latency/large_8192 | 5.206 | N/A | 19.583 | N/A | 20.801 | 32.716 | N/A | 0.27x | N/A | 0.25x | 0.16x |
| usable size latency/medium_1024 | 2.340 | N/A | 7.566 | N/A | 22.603 | 14.962 | N/A | 0.31x | N/A | 0.10x | 0.16x |
| usable size latency/small_32 | 2.297 | N/A | 4.603 | N/A | 17.831 | 12.360 | N/A | 0.50x | N/A | 0.13x | 0.19x |
| usable size query latency/huge_2m | 1.214 | N/A | 1.041 | N/A | 1.053 | 4.496 | N/A | 1.17x | N/A | 1.15x | 0.27x |
| usable size query latency/large_8192 | 0.396 | N/A | 0.619 | N/A | 0.825 | 5.881 | N/A | 0.64x | N/A | 0.48x | 0.07x |
| usable size query latency/medium_1024 | 0.384 | N/A | 0.746 | N/A | 0.727 | 4.105 | N/A | 0.51x | N/A | 0.53x | 0.09x |
| usable size query latency/small_32 | 0.457 | N/A | 0.829 | N/A | 0.751 | 5.015 | N/A | 0.55x | N/A | 0.61x | 0.09x |
