# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | RpMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs RpMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 476.569 | 3400.462 | 5304.579 | 1793.375 | 3014.911 | 2229.760 | 0.14x | 0.09x | 0.27x | 0.16x | 0.21x |
| allocator allocation latency/large_8192 | 45.199 | 510.362 | 1542.247 | 17.837 | 360.792 | 134.117 | 0.09x | 0.03x | 2.53x | 0.13x | 0.34x |
| allocator allocation latency/medium_1024 | 14.740 | 102.884 | 396.726 | 56.459 | 102.333 | 38.848 | 0.14x | 0.04x | 0.26x | 0.14x | 0.38x |
| allocator allocation latency/small_32 | 12.449 | 30.227 | 24.341 | 18.372 | 18.480 | 16.387 | 0.41x | 0.51x | 0.68x | 0.67x | 0.76x |
| allocator burst retention/large_8192 | 2431.129 | 9177.451 | 454248.146 | 3601.344 | 23706.389 | 29117.190 | 0.26x | 0.01x | 0.68x | 0.10x | 0.08x |
| allocator burst retention/medium_1024 | 1124.944 | 7922.438 | 96592.994 | 3238.928 | 7586.533 | 9621.532 | 0.14x | 0.01x | 0.35x | 0.15x | 0.12x |
| allocator burst retention/small_32 | 555.162 | 6453.767 | 803.026 | 2805.767 | 4414.249 | 3090.903 | 0.09x | 0.69x | 0.20x | 0.13x | 0.18x |
| allocator cycle latency/huge_2m | 27.306 | 10645.793 | 12241.796 | 12.169 | 6680.590 | 127.686 | 0.00x | 0.00x | 2.24x | 0.00x | 0.21x |
| allocator cycle latency/large_8192 | 2.305 | 20.272 | 16.896 | 10.844 | 17.703 | 16.071 | 0.11x | 0.14x | 0.21x | 0.13x | 0.14x |
| allocator cycle latency/medium_1024 | 2.434 | 22.500 | 6.281 | 11.340 | 17.137 | 10.267 | 0.11x | 0.39x | 0.21x | 0.14x | 0.24x |
| allocator cycle latency/small_32 | 2.125 | 20.309 | 2.929 | 10.518 | 16.669 | 8.258 | 0.10x | 0.73x | 0.20x | 0.13x | 0.26x |
| allocator deallocation latency/huge_2m | 993.954 | 4789.157 | 5362.739 | 1636.711 | 4686.579 | 3416.682 | 0.21x | 0.19x | 0.61x | 0.21x | 0.29x |
| allocator deallocation latency/large_8192 | 39.788 | 169.778 | 835.526 | 8.567 | 169.286 | 62.811 | 0.23x | 0.05x | 4.64x | 0.24x | 0.63x |
| allocator deallocation latency/medium_1024 | 15.070 | 38.187 | 134.085 | 12.791 | 54.096 | 28.735 | 0.39x | 0.11x | 1.18x | 0.28x | 0.52x |
| allocator deallocation latency/small_32 | 4.806 | 28.677 | 9.611 | 4.275 | 11.143 | 8.076 | 0.17x | 0.50x | 1.12x | 0.43x | 0.60x |
| cross-thread free handoff/huge_2m | 984.071 | 121412.146 | 127173.966 | 961.011 | 110463.129 | 3087.252 | 0.01x | 0.01x | 1.02x | 0.01x | 0.32x |
| cross-thread free handoff/large_8192 | 28608.766 | 69809.748 | 1325705.799 | 32260.066 | 115280.458 | 95343.407 | 0.41x | 0.02x | 0.89x | 0.25x | 0.30x |
| cross-thread free handoff/medium_1024 | 10384.773 | 40468.736 | 228368.251 | 36184.847 | 42898.883 | 48662.281 | 0.26x | 0.05x | 0.29x | 0.24x | 0.21x |
| cross-thread free handoff/small_32 | 5215.640 | 34733.528 | 7233.278 | 22319.203 | 22694.943 | 32877.836 | 0.15x | 0.72x | 0.23x | 0.23x | 0.16x |
| realloc latency/cross_class_32_to_64 | 9.510 | 55.019 | 12.019 | 20.970 | 33.313 | 19.419 | 0.17x | 0.79x | 0.45x | 0.29x | 0.49x |
| realloc latency/cross_class_8k_to_16k | 60.166 | 146.956 | 70.924 | 142.191 | 150.221 | 66.515 | 0.41x | 0.85x | 0.42x | 0.40x | 0.90x |
| realloc latency/huge_shrink_4m_to_2m | 28.586 | 1302107.923 | 10159.391 | 768251.613 | 1371188.383 | 259.275 | 0.00x | 0.00x | 0.00x | 0.00x | 0.11x |
| realloc latency/within_class_24_to_32 | 8.751 | 49.024 | 5.299 | 21.047 | 17.754 | 16.165 | 0.18x | 1.65x | 0.42x | 0.49x | 0.54x |
| realloc latency/within_class_6k_to_8k | 27.082 | 105.238 | 59.876 | 114.594 | 107.738 | 59.063 | 0.26x | 0.45x | 0.24x | 0.25x | 0.46x |
| segment cache eviction | 249453.566 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 4715.894 | 35896.974 | 19682.832 | 21179.355 | 29738.889 | 19056.324 | 0.13x | 0.24x | 0.22x | 0.16x | 0.25x |
| threaded saturated small allocation cycles | 57862.381 | 418998.501 | 82287.193 | 184490.575 | 275787.494 | 162240.632 | 0.14x | 0.70x | 0.31x | 0.21x | 0.36x |
| threaded small allocation cycles | 4808.195 | 36158.282 | 6662.770 | 19192.940 | 26270.044 | 17491.391 | 0.13x | 0.72x | 0.25x | 0.18x | 0.27x |
| usable size latency/huge_2m | 28.331 | N/A | 9253.104 | N/A | 7828.524 | 121.670 | N/A | 0.00x | N/A | 0.00x | 0.23x |
| usable size latency/large_8192 | 2.369 | N/A | 16.670 | N/A | 17.924 | 18.400 | N/A | 0.14x | N/A | 0.13x | 0.13x |
| usable size latency/medium_1024 | 5.318 | N/A | 6.758 | N/A | 17.556 | 10.864 | N/A | 0.79x | N/A | 0.30x | 0.49x |
| usable size latency/small_32 | 5.722 | N/A | 3.572 | N/A | 16.682 | 11.088 | N/A | 1.60x | N/A | 0.34x | 0.52x |
| usable size query latency/huge_2m | 0.479 | N/A | 0.710 | N/A | 0.553 | 3.617 | N/A | 0.68x | N/A | 0.87x | 0.13x |
| usable size query latency/large_8192 | 0.319 | N/A | 0.543 | N/A | 0.530 | 3.423 | N/A | 0.59x | N/A | 0.60x | 0.09x |
| usable size query latency/medium_1024 | 0.333 | N/A | 0.776 | N/A | 0.480 | 3.297 | N/A | 0.43x | N/A | 0.69x | 0.10x |
| usable size query latency/small_32 | 0.323 | N/A | 0.600 | N/A | 0.461 | 3.382 | N/A | 0.54x | N/A | 0.70x | 0.10x |
