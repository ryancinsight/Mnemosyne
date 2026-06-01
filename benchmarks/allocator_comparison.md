# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 3079.298 | 2896.588 | 4367.924 | 2821.193 | 2154.572 | 1.06x | 0.70x | 1.09x | 1.43x |
| allocator allocation latency/large_8192 | 186.093 | 981.359 | 1374.434 | 890.868 | 132.699 | 0.19x | 0.14x | 0.21x | 1.40x |
| allocator allocation latency/medium_1024 | 22.741 | 245.183 | 340.648 | 140.671 | 42.336 | 0.09x | 0.07x | 0.16x | 0.54x |
| allocator allocation latency/small_32 | 10.302 | 38.220 | 20.589 | 15.880 | 16.707 | 0.27x | 0.50x | 0.65x | 0.62x |
| allocator burst retention/large_8192 | 2360.557 | 9626.337 | 428485.156 | 21513.477 | 32007.373 | 0.25x | 0.01x | 0.11x | 0.07x |
| allocator burst retention/medium_1024 | 1092.469 | 8146.356 | 89181.689 | 7792.511 | 9778.357 | 0.13x | 0.01x | 0.14x | 0.11x |
| allocator burst retention/small_32 | 1159.754 | 7720.996 | 824.855 | 4173.141 | 3210.913 | 0.15x | 1.41x | 0.28x | 0.36x |
| allocator cycle latency/huge_2m | 18.609 | 9924.600 | 11938.593 | 6594.682 | 127.573 | 0.00x | 0.00x | 0.00x | 0.15x |
| allocator cycle latency/large_8192 | 3.034 | 23.306 | 14.388 | 17.034 | 16.926 | 0.13x | 0.21x | 0.18x | 0.18x |
| allocator cycle latency/medium_1024 | 2.716 | 20.520 | 5.647 | 16.502 | 7.943 | 0.13x | 0.48x | 0.16x | 0.34x |
| allocator cycle latency/small_32 | 2.634 | 21.165 | 2.794 | 15.757 | 7.117 | 0.12x | 0.94x | 0.17x | 0.37x |
| allocator deallocation latency/huge_2m | 4592.426 | 5868.625 | 6787.439 | 4976.410 | 3132.831 | 0.78x | 0.68x | 0.92x | 1.47x |
| allocator deallocation latency/large_8192 | 97.380 | 346.257 | 556.174 | 269.735 | 66.350 | 0.28x | 0.18x | 0.36x | 1.47x |
| allocator deallocation latency/medium_1024 | 23.954 | 106.804 | 133.467 | 77.371 | 23.782 | 0.22x | 0.18x | 0.31x | 1.01x |
| allocator deallocation latency/small_32 | 3.589 | 18.773 | 5.957 | 9.706 | 6.900 | 0.19x | 0.60x | 0.37x | 0.52x |
| cross-thread free handoff/huge_2m | 725.545 | 106110.254 | 125481.738 | 92512.305 | 2981.596 | 0.01x | 0.01x | 0.01x | 0.24x |
| cross-thread free handoff/large_8192 | 23723.404 | 58158.545 | 1137416.406 | 91093.115 | 81977.002 | 0.41x | 0.02x | 0.26x | 0.29x |
| cross-thread free handoff/medium_1024 | 8602.634 | 31047.168 | 174596.973 | 33310.205 | 45242.090 | 0.28x | 0.05x | 0.26x | 0.19x |
| cross-thread free handoff/small_32 | 5110.876 | 27797.949 | 7399.509 | 19092.267 | 28641.162 | 0.18x | 0.69x | 0.27x | 0.18x |
| realloc latency/cross_class_32_to_64 | 9.264 | 43.186 | 11.017 | 32.733 | 19.902 | 0.21x | 0.84x | 0.28x | 0.47x |
| realloc latency/cross_class_8k_to_16k | 47.078 | 131.814 | 66.754 | 130.259 | 54.483 | 0.36x | 0.71x | 0.36x | 0.86x |
| realloc latency/huge_shrink_4m_to_2m | 21.511 | 977246.094 | 9968.781 | 1028878.906 | 249.906 | 0.00x | 0.00x | 0.00x | 0.09x |
| realloc latency/within_class_24_to_32 | 5.528 | 47.710 | 4.819 | 16.990 | 19.337 | 0.12x | 1.15x | 0.33x | 0.29x |
| realloc latency/within_class_6k_to_8k | 26.967 | 125.444 | 68.344 | 120.072 | 60.956 | 0.21x | 0.39x | 0.22x | 0.44x |
| segment cache eviction | 240100.781 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 5957.140 | 33503.101 | 14949.857 | 25867.188 | 18043.787 | 0.18x | 0.40x | 0.23x | 0.33x |
| threaded saturated small allocation cycles | 76901.953 | 401584.375 | 83356.689 | 274232.422 | 150439.160 | 0.19x | 0.92x | 0.28x | 0.51x |
| threaded small allocation cycles | 5587.231 | 33455.249 | 6502.323 | 30558.875 | 16599.896 | 0.17x | 0.86x | 0.18x | 0.34x |
| usable size latency/huge_2m | 19.756 | N/A | 12548.669 | 7348.972 | 131.399 | N/A | 0.00x | 0.00x | 0.15x |
| usable size latency/large_8192 | 3.011 | N/A | 15.009 | 17.392 | 21.388 | N/A | 0.20x | 0.17x | 0.14x |
| usable size latency/medium_1024 | 4.927 | N/A | 6.298 | 16.442 | 12.335 | N/A | 0.78x | 0.30x | 0.40x |
| usable size latency/small_32 | 4.658 | N/A | 3.061 | 16.409 | 10.529 | N/A | 1.52x | 0.28x | 0.44x |
| usable size query latency/huge_2m | 0.377 | N/A | 0.964 | 0.669 | 3.475 | N/A | 0.39x | 0.56x | 0.11x |
| usable size query latency/large_8192 | 0.333 | N/A | 0.730 | 0.523 | 3.382 | N/A | 0.46x | 0.64x | 0.10x |
| usable size query latency/medium_1024 | 0.348 | N/A | 0.702 | 0.629 | 3.308 | N/A | 0.50x | 0.55x | 0.11x |
| usable size query latency/small_32 | 0.428 | N/A | 1.050 | 0.625 | 3.453 | N/A | 0.41x | 0.68x | 0.12x |
