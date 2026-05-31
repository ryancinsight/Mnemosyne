# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 722.953 | 2676.446 | 4075.186 | N/A | 1716.232 | 0.27x | 0.18x | N/A | 0.42x |
| allocator allocation latency/large_8192 | 20.939 | 322.762 | 1166.254 | 418.354 | 77.288 | 0.06x | 0.02x | 0.05x | 0.27x |
| allocator allocation latency/medium_1024 | 10.907 | 58.374 | 239.959 | 68.833 | 28.715 | 0.19x | 0.05x | 0.16x | 0.38x |
| allocator allocation latency/small_32 | 9.504 | 19.533 | 14.384 | 13.616 | 12.535 | 0.49x | 0.66x | 0.70x | 0.76x |
| allocator burst retention/large_8192 | 2632.056 | 9294.424 | 397998.764 | 20207.191 | 26567.096 | 0.28x | 0.01x | 0.13x | 0.10x |
| allocator burst retention/medium_1024 | 949.455 | 7199.425 | 77226.864 | 7802.428 | 8941.095 | 0.13x | 0.01x | 0.12x | 0.11x |
| allocator burst retention/small_32 | 516.850 | 6864.544 | 822.398 | 4221.034 | 2588.212 | 0.08x | 0.63x | 0.12x | 0.20x |
| allocator cycle latency/huge_2m | 21.343 | 7696.745 | 8353.077 | N/A | 114.544 | 0.00x | 0.00x | N/A | 0.19x |
| allocator cycle latency/large_8192 | 8.539 | 20.882 | 16.851 | 17.287 | 15.352 | 0.41x | 0.51x | 0.49x | 0.56x |
| allocator cycle latency/medium_1024 | 7.690 | 20.441 | 5.646 | 16.653 | 7.230 | 0.38x | 1.36x | 0.46x | 1.06x |
| allocator cycle latency/small_32 | 7.580 | 20.096 | 2.753 | 16.290 | 6.826 | 0.38x | 2.75x | 0.47x | 1.11x |
| allocator deallocation latency/huge_2m | 1280.798 | 3990.366 | 4343.767 | N/A | 2878.062 | 0.32x | 0.29x | N/A | 0.45x |
| allocator deallocation latency/large_8192 | 16.853 | 70.404 | 470.667 | 158.650 | 48.011 | 0.24x | 0.04x | 0.11x | 0.35x |
| allocator deallocation latency/medium_1024 | 7.883 | 25.623 | 73.707 | 45.015 | 17.549 | 0.31x | 0.11x | 0.18x | 0.45x |
| allocator deallocation latency/small_32 | 2.946 | 10.315 | 4.806 | 9.630 | 6.610 | 0.29x | 0.61x | 0.31x | 0.45x |
| cross-thread free handoff/huge_2m | 2405.660 | 71187.324 | 84380.102 | N/A | 7953.567 | 0.03x | 0.03x | N/A | 0.30x |
| cross-thread free handoff/large_8192 | 30168.314 | 377366.322 | 856537.514 | 93045.408 | 77047.396 | 0.08x | 0.04x | 0.32x | 0.39x |
| cross-thread free handoff/medium_1024 | 19406.909 | 35633.991 | 142116.928 | 37646.006 | 38524.519 | 0.54x | 0.14x | 0.52x | 0.50x |
| cross-thread free handoff/small_32 | 16197.767 | 29026.242 | 18478.906 | 20856.990 | 27906.936 | 0.56x | 0.88x | 0.78x | 0.58x |
| realloc latency/cross_class_32_to_64 | 7.981 | 42.930 | 7.611 | 32.953 | 16.970 | 0.19x | 1.05x | 0.24x | 0.47x |
| realloc latency/cross_class_8k_to_16k | 47.220 | 129.692 | 67.512 | 130.217 | 54.348 | 0.36x | 0.70x | 0.36x | 0.87x |
| realloc latency/huge_shrink_4m_to_2m | 23.538 | 987151.285 | 8256.386 | 1037607.889 | 242.776 | 0.00x | 0.00x | 0.00x | 0.10x |
| realloc latency/within_class_24_to_32 | 3.493 | 43.302 | 4.364 | 17.244 | 15.156 | 0.08x | 0.80x | 0.20x | 0.23x |
| realloc latency/within_class_6k_to_8k | 25.777 | 100.410 | 56.151 | 97.728 | 51.323 | 0.26x | 0.46x | 0.26x | 0.50x |
| segment cache eviction | 208804.832 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 47718.218 | 349546.603 | 65200.719 | 263495.738 | 129565.980 | 0.14x | 0.73x | 0.18x | 0.37x |
| threaded small allocation cycles | 8671.303 | 31898.445 | 13623.725 | 27406.548 | 19778.662 | 0.27x | 0.64x | 0.32x | 0.44x |
| usable size latency/huge_2m | 21.131 | N/A | 7367.777 | N/A | 115.269 | N/A | 0.00x | N/A | 0.18x |
| usable size latency/large_8192 | 2.156 | N/A | 16.439 | 17.594 | 17.943 | N/A | 0.13x | 0.12x | 0.12x |
| usable size latency/medium_1024 | 3.065 | N/A | 5.967 | 16.895 | 10.283 | N/A | 0.51x | 0.18x | 0.30x |
| usable size latency/small_32 | 2.977 | N/A | 2.836 | 16.425 | 9.957 | N/A | 1.05x | 0.18x | 0.30x |
| usable size query latency/huge_2m | 0.402 | N/A | 0.530 | N/A | 3.199 | N/A | 0.76x | N/A | 0.13x |
| usable size query latency/large_8192 | 0.282 | N/A | 0.525 | 0.454 | 3.204 | N/A | 0.54x | 0.62x | 0.09x |
| usable size query latency/medium_1024 | 0.286 | N/A | 0.525 | 0.456 | 3.205 | N/A | 0.54x | 0.63x | 0.09x |
| usable size query latency/small_32 | 0.298 | N/A | 0.537 | 0.459 | 3.221 | N/A | 0.55x | 0.65x | 0.09x |
