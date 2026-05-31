# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 723.939 | 2708.433 | 4091.733 | N/A | 1702.937 | 0.27x | 0.18x | N/A | 0.43x |
| allocator allocation latency/large_8192 | 22.434 | 352.334 | 1209.976 | 402.180 | 83.840 | 0.06x | 0.02x | 0.06x | 0.27x |
| allocator allocation latency/medium_1024 | 11.381 | 47.275 | 235.359 | 69.298 | 29.436 | 0.24x | 0.05x | 0.16x | 0.39x |
| allocator allocation latency/small_32 | 9.733 | 19.860 | 14.537 | 13.621 | 12.675 | 0.49x | 0.67x | 0.71x | 0.77x |
| allocator burst retention/large_8192 | 2842.041 | 9484.959 | 398406.728 | 20072.698 | 26669.637 | 0.30x | 0.01x | 0.14x | 0.11x |
| allocator burst retention/medium_1024 | 1019.205 | 6755.177 | 77778.805 | 7418.642 | 8786.716 | 0.15x | 0.01x | 0.14x | 0.12x |
| allocator burst retention/small_32 | 633.675 | 6567.502 | 829.663 | 4289.431 | 2669.522 | 0.10x | 0.76x | 0.15x | 0.24x |
| allocator cycle latency/huge_2m | 22.332 | 7570.020 | 8442.797 | N/A | 114.235 | 0.00x | 0.00x | N/A | 0.20x |
| allocator cycle latency/large_8192 | 8.520 | 20.493 | 16.868 | 17.381 | 15.304 | 0.42x | 0.51x | 0.49x | 0.56x |
| allocator cycle latency/medium_1024 | 7.645 | 20.316 | 5.658 | 16.622 | 7.266 | 0.38x | 1.35x | 0.46x | 1.05x |
| allocator cycle latency/small_32 | 7.566 | 20.311 | 2.777 | 16.390 | 6.835 | 0.37x | 2.72x | 0.46x | 1.11x |
| allocator deallocation latency/huge_2m | 1253.784 | 4106.307 | 4368.587 | N/A | 3042.598 | 0.31x | 0.29x | N/A | 0.41x |
| allocator deallocation latency/large_8192 | 15.736 | 83.960 | 554.743 | 182.893 | 48.053 | 0.19x | 0.03x | 0.09x | 0.33x |
| allocator deallocation latency/medium_1024 | 8.399 | 30.613 | 78.330 | 49.583 | 17.847 | 0.27x | 0.11x | 0.17x | 0.47x |
| allocator deallocation latency/small_32 | 2.847 | 8.807 | 4.951 | 9.640 | 6.559 | 0.32x | 0.58x | 0.30x | 0.43x |
| cross-thread free handoff/huge_2m | 2473.730 | 78784.665 | 90610.015 | N/A | 7643.188 | 0.03x | 0.03x | N/A | 0.32x |
| cross-thread free handoff/large_8192 | 29343.829 | 53704.995 | 956743.429 | 92746.320 | 75667.756 | 0.55x | 0.03x | 0.32x | 0.39x |
| cross-thread free handoff/medium_1024 | 19983.037 | 32233.033 | 147634.737 | 37589.195 | 38809.170 | 0.62x | 0.14x | 0.53x | 0.51x |
| cross-thread free handoff/small_32 | 16978.685 | 30555.831 | 18113.878 | 20552.811 | 27633.823 | 0.56x | 0.94x | 0.83x | 0.61x |
| realloc latency/cross_class_32_to_64 | 8.724 | 42.554 | 8.595 | 33.027 | 16.969 | 0.21x | 1.02x | 0.26x | 0.51x |
| realloc latency/cross_class_8k_to_16k | 49.581 | 137.232 | 67.224 | 132.289 | 57.458 | 0.36x | 0.74x | 0.37x | 0.86x |
| realloc latency/huge_shrink_4m_to_2m | 22.270 | 977315.851 | 7941.467 | 1035543.578 | 248.944 | 0.00x | 0.00x | 0.00x | 0.09x |
| realloc latency/within_class_24_to_32 | 3.689 | 42.227 | 4.434 | 17.205 | 15.147 | 0.09x | 0.83x | 0.21x | 0.24x |
| realloc latency/within_class_6k_to_8k | 25.946 | 99.038 | 56.030 | 98.132 | 52.251 | 0.26x | 0.46x | 0.26x | 0.50x |
| segment cache eviction | 208133.190 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 53348.712 | 350588.662 | 74907.496 | 267400.037 | 129105.718 | 0.15x | 0.71x | 0.20x | 0.41x |
| threaded small allocation cycles | 12619.264 | 33519.926 | 13293.554 | 26807.131 | 17758.029 | 0.38x | 0.95x | 0.47x | 0.71x |
| usable size latency/huge_2m | 22.234 | N/A | 7360.731 | N/A | 116.147 | N/A | 0.00x | N/A | 0.19x |
| usable size latency/large_8192 | 2.518 | N/A | 16.434 | 17.575 | 17.911 | N/A | 0.15x | 0.14x | 0.14x |
| usable size latency/medium_1024 | 3.437 | N/A | 5.985 | 16.788 | 10.309 | N/A | 0.57x | 0.20x | 0.33x |
| usable size latency/small_32 | 3.421 | N/A | 2.868 | 16.596 | 9.876 | N/A | 1.19x | 0.21x | 0.35x |
| usable size query latency/huge_2m | 0.396 | N/A | 0.524 | N/A | 3.214 | N/A | 0.75x | N/A | 0.12x |
| usable size query latency/large_8192 | 0.275 | N/A | 0.535 | 0.462 | 3.213 | N/A | 0.51x | 0.59x | 0.09x |
| usable size query latency/medium_1024 | 0.266 | N/A | 0.524 | 0.456 | 3.219 | N/A | 0.51x | 0.58x | 0.08x |
| usable size query latency/small_32 | 0.264 | N/A | 0.529 | 0.455 | 3.207 | N/A | 0.50x | 0.58x | 0.08x |
