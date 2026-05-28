# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 133.627 | 200.797 | 340.193 | 85.498 | N/A | 0.67x | 0.39x | 1.56x | N/A |
| allocator allocation latency/small_32 | 9.249 | 33.017 | 16.212 | 15.459 | N/A | 0.28x | 0.57x | 0.60x | N/A |
| allocator burst retention/large_8192 | 7246.274 | 12175.098 | 558314.062 | 21621.436 | N/A | 0.60x | 0.01x | 0.34x | N/A |
| allocator burst retention/medium_1024 | 2211.653 | 7992.914 | 101986.865 | 7684.222 | N/A | 0.28x | 0.02x | 0.29x | N/A |
| allocator burst retention/small_32 | 2195.262 | 7644.434 | 1293.334 | 3978.256 | N/A | 0.29x | 1.70x | 0.55x | N/A |
| allocator cycle latency/large_8192 | 7.358 | 20.960 | 17.183 | 17.562 | N/A | 0.35x | 0.43x | 0.42x | N/A |
| allocator cycle latency/medium_1024 | 6.818 | 20.803 | 5.855 | 16.938 | N/A | 0.33x | 1.16x | 0.40x | N/A |
| allocator cycle latency/small_32 | 6.793 | 20.130 | 2.809 | 16.141 | N/A | 0.34x | 2.42x | 0.42x | N/A |
| allocator deallocation latency/medium_1024 | 93.029 | 114.997 | 133.881 | 100.081 | N/A | 0.81x | 0.69x | 0.93x | N/A |
| allocator deallocation latency/small_32 | 4.717 | 21.295 | 6.614 | 9.745 | N/A | 0.22x | 0.71x | 0.48x | N/A |
| cross-thread free handoff/medium_1024 | 21168.188 | 38093.152 | 182569.434 | 41369.971 | N/A | 0.56x | 0.12x | 0.51x | N/A |
| cross-thread free handoff/small_32 | 20722.424 | 38668.628 | 7168.394 | 23262.134 | N/A | 0.54x | 2.89x | 0.89x | N/A |
| realloc latency/cross_class_32_to_64 | 17.433 | 45.920 | 10.739 | 34.257 | N/A | 0.38x | 1.62x | 0.51x | N/A |
| realloc latency/within_class_24_to_32 | 7.460 | 46.437 | 4.791 | 17.885 | N/A | 0.16x | 1.56x | 0.42x | N/A |
| segment cache eviction | 65683.398 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 93759.712 | 369930.364 | 66582.071 | 278652.007 | N/A | 0.25x | 1.41x | 0.34x | N/A |
| threaded small allocation cycles | 11877.045 | 36228.373 | 4736.763 | 26687.898 | N/A | 0.33x | 2.51x | 0.45x | N/A |
| usable size latency/medium_1024 | 6.824 | N/A | 6.067 | 16.770 | N/A | N/A | 1.12x | 0.41x | N/A |
| usable size latency/small_32 | 6.846 | N/A | 2.950 | 16.372 | N/A | N/A | 2.32x | 0.42x | N/A |
| usable size query latency/medium_1024 | 0.427 | N/A | 0.729 | 0.683 | N/A | N/A | 0.59x | 0.63x | N/A |
| usable size query latency/small_32 | 0.419 | N/A | 0.744 | 0.449 | N/A | N/A | 0.56x | 0.93x | N/A |
