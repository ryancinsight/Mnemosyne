# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | RpMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs RpMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 462.705 | 3646.595 | 5337.122 | 1559.941 | N/A | N/A | 0.13x | 0.09x | 0.30x | N/A | N/A |
| allocator allocation latency/large_8192 | 30.314 | 289.530 | 1238.255 | 28.643 | 566.735 | N/A | 0.10x | 0.02x | 1.06x | 0.05x | N/A |
| allocator allocation latency/medium_1024 | 11.369 | 54.426 | 267.153 | 31.570 | 115.305 | N/A | 0.21x | 0.04x | 0.36x | 0.10x | N/A |
| allocator allocation latency/small_32 | 9.349 | 22.667 | 15.291 | 13.984 | 51.985 | N/A | 0.41x | 0.61x | 0.67x | 0.18x | N/A |
| allocator burst retention/large_8192 | 3084.780 | 9209.698 | 432589.063 | 3715.195 | 244159.571 | N/A | 0.33x | 0.01x | 0.83x | 0.01x | N/A |
| allocator burst retention/medium_1024 | 1498.932 | 6783.197 | 83348.268 | 3239.931 | 47132.072 | N/A | 0.22x | 0.02x | 0.46x | 0.03x | N/A |
| allocator burst retention/small_32 | 835.632 | 6894.954 | 900.368 | 2967.423 | 13557.834 | N/A | 0.12x | 0.93x | 0.28x | 0.06x | N/A |
| allocator cycle latency/huge_2m | 28.165 | 10184.120 | 12917.038 | 13.050 | N/A | N/A | 0.00x | 0.00x | 2.16x | N/A | N/A |
| allocator cycle latency/large_8192 | 4.371 | 22.613 | 15.685 | 12.245 | 140.968 | N/A | 0.19x | 0.28x | 0.36x | 0.03x | N/A |
| allocator cycle latency/medium_1024 | 4.299 | 23.825 | 6.435 | 10.857 | 78.951 | N/A | 0.18x | 0.67x | 0.40x | 0.05x | N/A |
| allocator cycle latency/small_32 | 3.361 | 20.188 | 2.817 | 10.291 | 63.899 | N/A | 0.17x | 1.19x | 0.33x | 0.05x | N/A |
| allocator deallocation latency/huge_2m | 935.181 | 4120.294 | 4988.906 | 1673.293 | N/A | N/A | 0.23x | 0.19x | 0.56x | N/A | N/A |
| allocator deallocation latency/large_8192 | 35.546 | 132.454 | 496.456 | 16.167 | 719.193 | N/A | 0.27x | 0.07x | 2.20x | 0.05x | N/A |
| allocator deallocation latency/medium_1024 | 11.344 | 21.666 | 98.733 | 7.887 | 163.970 | N/A | 0.52x | 0.11x | 1.44x | 0.07x | N/A |
| allocator deallocation latency/small_32 | 3.347 | 14.860 | 5.718 | 2.546 | 31.802 | N/A | 0.23x | 0.59x | 1.31x | 0.11x | N/A |
| cross-thread free handoff/huge_2m | 991.193 | 105409.888 | 94861.390 | 996.928 | N/A | N/A | 0.01x | 0.01x | 0.99x | N/A | N/A |
| cross-thread free handoff/large_8192 | 27546.830 | 52817.221 | 1067406.675 | 25850.438 | 733181.780 | N/A | 0.52x | 0.03x | 1.07x | 0.04x | N/A |
| cross-thread free handoff/medium_1024 | 15015.383 | 34211.399 | 158229.221 | 22912.970 | 173256.812 | N/A | 0.44x | 0.09x | 0.66x | 0.09x | N/A |
| cross-thread free handoff/small_32 | 9419.017 | 28977.653 | 7580.420 | 19000.847 | 49385.736 | N/A | 0.33x | 1.24x | 0.50x | 0.19x | N/A |
| realloc latency/cross_class_32_to_64 | 10.412 | 44.067 | 7.754 | 20.769 | 114.991 | N/A | 0.24x | 1.34x | 0.50x | 0.09x | N/A |
| realloc latency/cross_class_8k_to_16k | 54.738 | 149.514 | 107.087 | 126.388 | 274.703 | N/A | 0.37x | 0.51x | 0.43x | 0.20x | N/A |
| realloc latency/huge_shrink_4m_to_2m | 45435.607 | 1122813.674 | 5975.994 | 656586.967 | N/A | N/A | 0.04x | 7.60x | 0.07x | N/A | N/A |
| realloc latency/within_class_24_to_32 | 5.373 | 46.697 | 4.422 | 20.703 | 62.956 | N/A | 0.12x | 1.21x | 0.26x | 0.09x | N/A |
| realloc latency/within_class_6k_to_8k | 53.491 | 115.279 | 60.549 | 94.371 | 248.404 | N/A | 0.46x | 0.88x | 0.57x | 0.22x | N/A |
| segment cache eviction | 236685.408 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 8088.386 | 33253.889 | 14208.211 | 18965.081 | 106456.416 | N/A | 0.24x | 0.57x | 0.43x | 0.08x | N/A |
| threaded saturated small allocation cycles | 91407.087 | 400127.938 | 82252.823 | 209877.486 | 1047826.991 | N/A | 0.23x | 1.11x | 0.44x | 0.09x | N/A |
| threaded small allocation cycles | 9609.384 | 30959.406 | 6357.249 | 18381.279 | 73737.795 | N/A | 0.31x | 1.51x | 0.52x | 0.13x | N/A |
| usable size latency/huge_2m | 27.423 | N/A | 6308.072 | N/A | N/A | N/A | N/A | 0.00x | N/A | N/A | N/A |
| usable size latency/large_8192 | 3.780 | N/A | 16.416 | N/A | 85.800 | N/A | N/A | 0.23x | N/A | 0.04x | N/A |
| usable size latency/medium_1024 | 5.036 | N/A | 7.174 | N/A | 93.825 | N/A | N/A | 0.70x | N/A | 0.05x | N/A |
| usable size latency/small_32 | 6.664 | N/A | 4.320 | N/A | 70.853 | N/A | N/A | 1.54x | N/A | 0.09x | N/A |
| usable size query latency/huge_2m | 0.353 | N/A | 0.534 | N/A | N/A | N/A | N/A | 0.66x | N/A | N/A | N/A |
| usable size query latency/large_8192 | 0.281 | N/A | 0.573 | N/A | 12.286 | N/A | N/A | 0.49x | N/A | 0.02x | N/A |
| usable size query latency/medium_1024 | 0.283 | N/A | 0.538 | N/A | 12.327 | N/A | N/A | 0.53x | N/A | 0.02x | N/A |
| usable size query latency/small_32 | 0.297 | N/A | 0.631 | N/A | 12.226 | N/A | N/A | 0.47x | N/A | 0.02x | N/A |
