# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 43.958 | 51.241 | 271.136 | 71.215 | 28.515 | 0.86x | 0.16x | 0.62x | 1.54x |
| allocator allocation latency/small_32 | 10.284 | 24.947 | 14.766 | 13.428 | 12.718 | 0.41x | 0.70x | 0.77x | 0.81x |
| allocator burst retention/large_8192 | 2922.395 | 8841.758 | 392379.275 | 21193.244 | 26607.355 | 0.33x | 0.01x | 0.14x | 0.11x |
| allocator burst retention/medium_1024 | 1529.274 | 7146.826 | 80889.755 | 8061.580 | 10208.443 | 0.21x | 0.02x | 0.19x | 0.15x |
| allocator burst retention/small_32 | 516.954 | 6265.430 | 871.714 | 4188.563 | 2677.250 | 0.08x | 0.59x | 0.12x | 0.19x |
| allocator cycle latency/large_8192 | 1.920 | 20.852 | 16.655 | 17.504 | 15.953 | 0.09x | 0.12x | 0.11x | 0.12x |
| allocator cycle latency/medium_1024 | 1.908 | 20.976 | 5.837 | 16.632 | 7.751 | 0.09x | 0.33x | 0.11x | 0.25x |
| allocator cycle latency/small_32 | 1.778 | 20.107 | 2.767 | 16.132 | 7.146 | 0.09x | 0.64x | 0.11x | 0.25x |
| allocator deallocation latency/medium_1024 | 19.593 | 19.472 | 92.218 | 63.917 | 18.726 | 1.01x | 0.21x | 0.31x | 1.05x |
| allocator deallocation latency/small_32 | 2.805 | 9.738 | 4.916 | 9.407 | 6.598 | 0.29x | 0.57x | 0.30x | 0.43x |
| cross-thread free handoff/medium_1024 | 9002.717 | 34206.163 | 167620.723 | 35887.183 | 42154.080 | 0.26x | 0.05x | 0.25x | 0.21x |
| cross-thread free handoff/small_32 | 4541.605 | 29266.595 | 9190.551 | 19621.760 | 28923.781 | 0.16x | 0.49x | 0.23x | 0.16x |
| realloc latency/cross_class_32_to_64 | 5.272 | 44.536 | 8.970 | 32.843 | 17.393 | 0.12x | 0.59x | 0.16x | 0.30x |
| realloc latency/huge_shrink_4m_to_2m | 9264.058 | 16998.374 | 911334.721 | 1020579.682 | 247.372 | 0.54x | 0.01x | 0.01x | 37.45x |
| realloc latency/within_class_24_to_32 | 2.184 | 43.341 | 4.529 | 17.492 | 18.923 | 0.05x | 0.48x | 0.12x | 0.12x |
| segment cache eviction | 137906.138 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 46487.823 | 395117.865 | 75397.969 | 266250.295 | 144065.638 | 0.12x | 0.62x | 0.17x | 0.32x |
| threaded small allocation cycles | 4294.642 | 30798.442 | 5337.158 | 23708.330 | 15893.986 | 0.14x | 0.80x | 0.18x | 0.27x |
| usable size latency/medium_1024 | 2.284 | N/A | 6.473 | 16.788 | 10.811 | N/A | 0.35x | 0.14x | 0.21x |
| usable size latency/small_32 | 2.040 | N/A | 3.267 | 16.409 | 12.707 | N/A | 0.62x | 0.12x | 0.16x |
| usable size query latency/medium_1024 | 0.370 | N/A | 0.599 | 0.480 | 3.303 | N/A | 0.62x | 0.77x | 0.11x |
| usable size query latency/small_32 | 0.352 | N/A | 0.559 | 0.496 | 3.385 | N/A | 0.63x | 0.71x | 0.10x |
