# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 3953.381 | 3963.715 | 5828.149 | N/A | 2754.089 | 1.00x | 0.68x | N/A | 1.44x |
| allocator allocation latency/large_8192 | 38.653 | 740.879 | 1406.417 | 1162.810 | 143.806 | 0.05x | 0.03x | 0.03x | 0.27x |
| allocator allocation latency/medium_1024 | 13.308 | 164.216 | 235.115 | 186.080 | 32.448 | 0.08x | 0.06x | 0.07x | 0.41x |
| allocator allocation latency/small_32 | 9.105 | 30.226 | 14.681 | 47.294 | 12.799 | 0.30x | 0.62x | 0.19x | 0.71x |
| allocator burst retention/large_8192 | 2706.804 | 8772.318 | 405639.844 | 248723.438 | 28575.720 | 0.31x | 0.01x | 0.01x | 0.09x |
| allocator burst retention/medium_1024 | 1275.591 | 7044.129 | 83243.506 | 57572.821 | 8995.364 | 0.18x | 0.02x | 0.02x | 0.14x |
| allocator burst retention/small_32 | 604.821 | 7713.348 | 1020.701 | 13846.631 | 2718.938 | 0.08x | 0.59x | 0.04x | 0.22x |
| allocator cycle latency/huge_2m | 32.613 | 7556.122 | 8461.230 | N/A | 113.320 | 0.00x | 0.00x | N/A | 0.29x |
| allocator cycle latency/large_8192 | 1.954 | 22.012 | 16.830 | 74.579 | 15.842 | 0.09x | 0.12x | 0.03x | 0.12x |
| allocator cycle latency/medium_1024 | 1.907 | 20.200 | 5.625 | 56.163 | 7.560 | 0.09x | 0.34x | 0.03x | 0.25x |
| allocator cycle latency/small_32 | 1.806 | 20.493 | 2.743 | 48.300 | 6.921 | 0.09x | 0.66x | 0.04x | 0.26x |
| allocator deallocation latency/huge_2m | 5716.409 | 4220.886 | 4436.865 | N/A | 3102.277 | 1.35x | 1.29x | N/A | 1.84x |
| allocator deallocation latency/large_8192 | 714.656 | 255.316 | 543.718 | 821.296 | 70.742 | 2.80x | 1.31x | 0.87x | 10.10x |
| allocator deallocation latency/medium_1024 | 92.800 | 90.651 | 91.846 | 214.058 | 21.427 | 1.02x | 1.01x | 0.43x | 4.33x |
| allocator deallocation latency/small_32 | 3.784 | 14.379 | 5.305 | 29.921 | 7.822 | 0.26x | 0.71x | 0.13x | 0.48x |
| cross-thread free handoff/huge_2m | 926.762 | 114609.668 | 123571.777 | N/A | 3343.964 | 0.01x | 0.01x | N/A | 0.28x |
| cross-thread free handoff/large_8192 | 552573.438 | 52364.893 | 1080846.875 | 720524.219 | 84377.148 | 10.55x | 0.51x | 0.77x | 6.55x |
| cross-thread free handoff/medium_1024 | 10762.518 | 31776.880 | 164441.113 | 258834.375 | 43370.874 | 0.34x | 0.07x | 0.04x | 0.25x |
| cross-thread free handoff/small_32 | 4273.871 | 30306.873 | 9618.945 | 53000.293 | 28631.934 | 0.14x | 0.44x | 0.08x | 0.15x |
| realloc latency/cross_class_32_to_64 | 7.892 | 47.189 | 13.832 | 149.129 | 18.729 | 0.17x | 0.57x | 0.05x | 0.42x |
| realloc latency/cross_class_8k_to_16k | 72.143 | 143.192 | 124.656 | 352.608 | 56.193 | 0.50x | 0.58x | 0.20x | 1.28x |
| realloc latency/huge_shrink_4m_to_2m | 32.709 | 1186214.062 | 9755.830 | 1134407.812 | 249.505 | 0.00x | 0.00x | 0.00x | 0.13x |
| realloc latency/within_class_24_to_32 | 2.969 | 48.023 | 5.052 | 72.755 | 17.266 | 0.06x | 0.59x | 0.04x | 0.17x |
| realloc latency/within_class_6k_to_8k | 28.591 | 130.383 | 58.177 | 354.134 | 52.763 | 0.22x | 0.49x | 0.08x | 0.54x |
| segment cache eviction | 139535.083 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 60722.314 | 398044.141 | 78987.744 | 1034303.125 | 149366.992 | 0.15x | 0.77x | 0.06x | 0.41x |
| threaded small allocation cycles | 4368.411 | 33951.514 | 5241.179 | 73282.275 | 16314.285 | 0.13x | 0.83x | 0.06x | 0.27x |
| usable size latency/huge_2m | 32.922 | N/A | 8883.574 | N/A | 116.545 | N/A | 0.00x | N/A | 0.28x |
| usable size latency/large_8192 | 2.115 | N/A | 16.294 | 87.100 | 18.020 | N/A | 0.13x | 0.02x | 0.12x |
| usable size latency/medium_1024 | 2.077 | N/A | 5.923 | 69.194 | 12.231 | N/A | 0.35x | 0.03x | 0.17x |
| usable size latency/small_32 | 1.991 | N/A | 3.484 | 61.847 | 10.035 | N/A | 0.57x | 0.03x | 0.20x |
| usable size query latency/huge_2m | 0.560 | N/A | 0.703 | N/A | 3.474 | N/A | 0.80x | N/A | 0.16x |
| usable size query latency/large_8192 | 0.376 | N/A | 0.654 | 12.574 | 3.450 | N/A | 0.58x | 0.03x | 0.11x |
| usable size query latency/medium_1024 | 0.371 | N/A | 0.542 | 13.696 | 3.470 | N/A | 0.69x | 0.03x | 0.11x |
| usable size query latency/small_32 | 0.338 | N/A | 0.534 | 12.348 | 3.412 | N/A | 0.63x | 0.03x | 0.10x |
