# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 12.624 | 175.337 | 251.090 | 176.352 | 44.316 | 0.07x | 0.05x | 0.07x | 0.28x |
| allocator allocation latency/small_32 | 8.430 | 30.480 | 14.812 | 50.356 | 12.794 | 0.28x | 0.57x | 0.17x | 0.66x |
| allocator burst retention/large_8192 | 2513.932 | 8701.035 | 422218.066 | 192355.176 | 28184.546 | 0.29x | 0.01x | 0.01x | 0.09x |
| allocator burst retention/medium_1024 | 1254.695 | 7752.078 | 88698.535 | 59242.969 | 10284.113 | 0.16x | 0.01x | 0.02x | 0.12x |
| allocator burst retention/small_32 | 560.114 | 7144.081 | 1239.178 | 15047.337 | 2783.067 | 0.08x | 0.45x | 0.04x | 0.20x |
| allocator cycle latency/large_8192 | 2.270 | 23.081 | 16.747 | 74.877 | 15.943 | 0.10x | 0.14x | 0.03x | 0.14x |
| allocator cycle latency/medium_1024 | 2.104 | 20.298 | 5.632 | 56.044 | 7.620 | 0.10x | 0.37x | 0.04x | 0.28x |
| allocator cycle latency/small_32 | 1.803 | 20.224 | 2.754 | 48.240 | 6.894 | 0.09x | 0.65x | 0.04x | 0.26x |
| allocator deallocation latency/medium_1024 | 101.384 | 102.682 | 110.681 | 198.114 | 21.734 | 0.99x | 0.92x | 0.51x | 4.66x |
| allocator deallocation latency/small_32 | 3.078 | 18.506 | 4.971 | 29.075 | 8.103 | 0.17x | 0.62x | 0.11x | 0.38x |
| cross-thread free handoff/medium_1024 | 7883.521 | 30288.916 | 162808.105 | 242622.070 | 44238.037 | 0.26x | 0.05x | 0.03x | 0.18x |
| cross-thread free handoff/small_32 | 4840.057 | 29818.958 | 7459.857 | 54031.494 | 30838.989 | 0.16x | 0.65x | 0.09x | 0.16x |
| realloc latency/cross_class_32_to_64 | 7.764 | 48.746 | 8.370 | 111.580 | 19.870 | 0.16x | 0.93x | 0.07x | 0.39x |
| realloc latency/huge_shrink_4m_to_2m | 33.002 | 17781.616 | 909176.562 | 998914.844 | 268.858 | 0.00x | 0.00x | 0.00x | 0.12x |
| realloc latency/within_class_24_to_32 | 2.494 | 49.013 | 5.168 | 73.949 | 17.146 | 0.05x | 0.48x | 0.03x | 0.15x |
| segment cache eviction | 196893.555 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 58322.021 | 395882.031 | 76111.768 | 1067171.094 | 148177.246 | 0.15x | 0.77x | 0.05x | 0.39x |
| threaded small allocation cycles | 4497.555 | 30485.126 | 6295.108 | 76900.732 | 18870.178 | 0.15x | 0.71x | 0.06x | 0.24x |
| usable size latency/medium_1024 | 2.673 | N/A | 6.734 | 74.738 | 12.267 | N/A | 0.40x | 0.04x | 0.22x |
| usable size latency/small_32 | 2.390 | N/A | 3.789 | 76.361 | 11.232 | N/A | 0.63x | 0.03x | 0.21x |
| usable size query latency/medium_1024 | 0.366 | N/A | 0.526 | 12.156 | 3.308 | N/A | 0.69x | 0.03x | 0.11x |
| usable size query latency/small_32 | 0.421 | N/A | 0.576 | 12.422 | 3.190 | N/A | 0.73x | 0.03x | 0.13x |
