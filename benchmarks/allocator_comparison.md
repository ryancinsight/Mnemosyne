# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 31.260 | 225.653 | 364.778 | 111.322 | N/A | 0.14x | 0.09x | 0.28x | N/A |
| allocator allocation latency/small_32 | 17.686 | 38.856 | 19.136 | 18.839 | N/A | 0.46x | 0.92x | 0.94x | N/A |
| allocator burst retention/large_8192 | 8156.318 | 9762.789 | 415899.799 | 20012.661 | N/A | 0.84x | 0.02x | 0.41x | N/A |
| allocator burst retention/medium_1024 | 3807.490 | 8545.817 | 89547.593 | 7824.978 | N/A | 0.45x | 0.04x | 0.49x | N/A |
| allocator burst retention/small_32 | 3526.223 | 7028.778 | 898.409 | 4468.358 | N/A | 0.50x | 3.92x | 0.79x | N/A |
| allocator cycle latency/large_8192 | 11.537 | 26.218 | 19.935 | 22.805 | N/A | 0.44x | 0.58x | 0.51x | N/A |
| allocator cycle latency/medium_1024 | 11.184 | 24.887 | 6.403 | 20.530 | N/A | 0.45x | 1.75x | 0.54x | N/A |
| allocator cycle latency/small_32 | 12.168 | 28.209 | 3.294 | 20.685 | N/A | 0.43x | 3.69x | 0.59x | N/A |
| allocator deallocation latency/medium_1024 | 29.820 | 92.887 | 114.297 | 71.771 | N/A | 0.32x | 0.26x | 0.42x | N/A |
| allocator deallocation latency/small_32 | 6.414 | 20.864 | 5.828 | 17.283 | N/A | 0.31x | 1.10x | 0.37x | N/A |
| cross-thread free handoff/medium_1024 | 25777.748 | 35551.765 | 161137.953 | 40503.867 | N/A | 0.73x | 0.16x | 0.64x | N/A |
| cross-thread free handoff/small_32 | 19049.876 | 36856.715 | 14007.058 | 21678.814 | N/A | 0.52x | 1.36x | 0.88x | N/A |
| realloc latency/cross_class_32_to_64 | 32.121 | 48.250 | 8.986 | 36.711 | N/A | 0.67x | 3.57x | 0.87x | N/A |
| realloc latency/within_class_24_to_32 | 14.738 | 49.111 | 5.250 | 18.947 | N/A | 0.30x | 2.81x | 0.78x | N/A |
| segment cache eviction | 59078.183 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 204220.164 | 386645.939 | 74456.200 | 255348.010 | N/A | 0.53x | 2.74x | 0.80x | N/A |
| threaded small allocation cycles | 39867.045 | N/A | 7434.712 | 26918.384 | N/A | N/A | 5.36x | 1.48x | N/A |
| usable size latency/medium_1024 | 15.593 | N/A | 8.373 | 22.415 | N/A | N/A | 1.86x | 0.70x | N/A |
| usable size latency/small_32 | 16.077 | N/A | 3.047 | 26.209 | N/A | N/A | 5.28x | 0.61x | N/A |
| usable size query latency/medium_1024 | 0.383 | N/A | 0.544 | 0.520 | N/A | N/A | 0.70x | 0.74x | N/A |
| usable size query latency/small_32 | 0.411 | N/A | 0.589 | 0.497 | N/A | N/A | 0.70x | 0.83x | N/A |
