# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | RpMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs RpMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 5639.040 | 2665.839 | 4070.380 | 6595.569 | 2301.331 | 2229.760 | 2.12x | 1.39x | 0.85x | 2.45x | 2.53x |
| allocator allocation latency/large_8192 | 51.878 | 777.843 | 1124.805 | 494.749 | 674.494 | 134.117 | 0.07x | 0.05x | 0.10x | 0.08x | 0.39x |
| allocator allocation latency/medium_1024 | 13.779 | 160.034 | 245.259 | 39.685 | 111.045 | 38.848 | 0.09x | 0.06x | 0.35x | 0.12x | 0.35x |
| allocator allocation latency/small_32 | 10.255 | 33.023 | 15.624 | 16.117 | 22.072 | 16.387 | 0.31x | 0.66x | 0.64x | 0.46x | 0.63x |
| allocator burst retention/large_8192 | 3632.718 | 8690.381 | 394014.062 | 3937.939 | 22201.038 | 29117.190 | 0.42x | 0.01x | 0.92x | 0.16x | 0.12x |
| allocator burst retention/medium_1024 | 1404.052 | 6906.055 | 78563.477 | 3211.932 | 7722.064 | 9621.532 | 0.20x | 0.02x | 0.44x | 0.18x | 0.15x |
| allocator burst retention/small_32 | 620.086 | 6507.800 | 796.523 | 2664.081 | 4207.916 | 3090.903 | 0.10x | 0.78x | 0.23x | 0.15x | 0.20x |
| allocator cycle latency/huge_2m | 28.300 | 6600.763 | 10598.383 | 11.966 | 5194.897 | 127.686 | 0.00x | 0.00x | 2.37x | 0.01x | 0.22x |
| allocator cycle latency/large_8192 | 2.601 | 20.773 | 16.595 | 17.876 | 17.587 | 16.071 | 0.13x | 0.16x | 0.15x | 0.15x | 0.16x |
| allocator cycle latency/medium_1024 | 2.763 | 36.549 | 6.069 | 10.854 | 17.011 | 10.267 | 0.08x | 0.46x | 0.25x | 0.16x | 0.27x |
| allocator cycle latency/small_32 | 2.548 | 20.192 | 2.804 | 10.119 | 15.102 | 8.258 | 0.13x | 0.91x | 0.25x | 0.17x | 0.31x |
| allocator deallocation latency/huge_2m | 6025.688 | 4526.138 | 4614.417 | 5249.338 | 3885.226 | 3416.682 | 1.33x | 1.31x | 1.15x | 1.55x | 1.76x |
| allocator deallocation latency/large_8192 | 279.897 | 270.275 | 727.572 | 319.639 | 288.998 | 62.811 | 1.04x | 0.38x | 0.88x | 0.97x | 4.46x |
| allocator deallocation latency/medium_1024 | 18.069 | 54.120 | 110.714 | 31.196 | 48.980 | 28.735 | 0.33x | 0.16x | 0.58x | 0.37x | 0.63x |
| allocator deallocation latency/small_32 | 3.585 | 17.242 | 5.215 | 2.962 | 9.720 | 8.076 | 0.21x | 0.69x | 1.21x | 0.37x | 0.44x |
| cross-thread free handoff/huge_2m | 1589.254 | 67193.604 | 82094.434 | 5697.093 | 74914.917 | 3087.252 | 0.02x | 0.02x | 0.28x | 0.02x | 0.51x |
| cross-thread free handoff/large_8192 | 28626.343 | 54155.029 | 914203.906 | 31553.198 | 97733.691 | 95343.407 | 0.53x | 0.03x | 0.91x | 0.29x | 0.30x |
| cross-thread free handoff/medium_1024 | 19428.162 | 34774.121 | 146933.008 | 24655.103 | 39956.055 | 48662.281 | 0.56x | 0.13x | 0.79x | 0.49x | 0.40x |
| cross-thread free handoff/small_32 | 16978.918 | 31670.508 | 18809.363 | 20877.069 | 20603.931 | 32877.836 | 0.54x | 0.90x | 0.81x | 0.82x | 0.52x |
| realloc latency/cross_class_32_to_64 | 11.485 | 43.482 | 7.887 | 21.199 | 32.768 | 19.419 | 0.26x | 1.46x | 0.54x | 0.35x | 0.59x |
| realloc latency/cross_class_8k_to_16k | 66.786 | 129.378 | 67.083 | 127.034 | 130.568 | 66.515 | 0.52x | 1.00x | 0.53x | 0.51x | 1.00x |
| realloc latency/huge_shrink_4m_to_2m | 28.163 | 963153.125 | 8554.498 | 543071.484 | 1042344.531 | 259.275 | 0.00x | 0.00x | 0.00x | 0.00x | 0.11x |
| realloc latency/within_class_24_to_32 | 5.828 | 43.658 | 4.426 | 21.002 | 17.248 | 16.165 | 0.13x | 1.32x | 0.28x | 0.34x | 0.36x |
| realloc latency/within_class_6k_to_8k | 28.990 | 100.236 | 56.216 | 94.326 | 96.039 | 59.063 | 0.29x | 0.52x | 0.31x | 0.30x | 0.49x |
| segment cache eviction | 225936.914 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 9127.643 | 32733.862 | 18447.156 | 21293.713 | 26970.435 | 19056.324 | 0.28x | 0.49x | 0.43x | 0.34x | 0.48x |
| threaded saturated small allocation cycles | 63228.125 | 352347.266 | 71297.021 | 187570.312 | 270447.266 | 162240.632 | 0.18x | 0.89x | 0.34x | 0.23x | 0.39x |
| threaded small allocation cycles | 11177.489 | 32507.739 | 9366.951 | 20373.755 | 26995.850 | 17491.391 | 0.34x | 1.19x | 0.55x | 0.41x | 0.64x |
| usable size latency/huge_2m | 28.263 | N/A | 7374.493 | N/A | 5531.882 | 121.670 | N/A | 0.00x | N/A | 0.01x | 0.23x |
| usable size latency/large_8192 | 5.831 | N/A | 16.790 | N/A | 17.786 | 18.400 | N/A | 0.35x | N/A | 0.33x | 0.32x |
| usable size latency/medium_1024 | 5.800 | N/A | 6.384 | N/A | 16.960 | 10.864 | N/A | 0.91x | N/A | 0.34x | 0.53x |
| usable size latency/small_32 | 5.818 | N/A | 3.002 | N/A | 16.525 | 11.088 | N/A | 1.94x | N/A | 0.35x | 0.52x |
| usable size query latency/huge_2m | 0.361 | N/A | 0.527 | N/A | 0.458 | 3.617 | N/A | 0.69x | N/A | 0.79x | 0.10x |
| usable size query latency/large_8192 | 0.283 | N/A | 0.524 | N/A | 0.454 | 3.423 | N/A | 0.54x | N/A | 0.62x | 0.08x |
| usable size query latency/medium_1024 | 0.292 | N/A | 0.528 | N/A | 0.459 | 3.297 | N/A | 0.55x | N/A | 0.64x | 0.09x |
| usable size query latency/small_32 | 0.290 | N/A | 0.536 | N/A | 0.458 | 3.382 | N/A | 0.54x | N/A | 0.63x | 0.09x |
