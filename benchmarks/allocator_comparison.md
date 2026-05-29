# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 116.591 | 194.000 | 275.786 | 115.963 | N/A | 0.60x | 0.42x | 1.01x | N/A |
| allocator allocation latency/small_32 | 11.912 | 31.124 | 14.835 | 14.659 | N/A | 0.38x | 0.80x | 0.81x | N/A |
| allocator burst retention/large_8192 | 6292.155 | 10183.896 | 475517.383 | 21520.667 | N/A | 0.62x | 0.01x | 0.29x | N/A |
| allocator burst retention/medium_1024 | 1949.624 | 8167.200 | 91201.196 | 7395.782 | N/A | 0.24x | 0.02x | 0.26x | N/A |
| allocator burst retention/small_32 | 1010.409 | 7077.182 | 1543.373 | 4016.202 | N/A | 0.14x | 0.65x | 0.25x | N/A |
| allocator cycle latency/large_8192 | 3.833 | 21.400 | 16.139 | 17.187 | N/A | 0.18x | 0.24x | 0.22x | N/A |
| allocator cycle latency/medium_1024 | 3.768 | 20.196 | 5.621 | 16.545 | N/A | 0.19x | 0.67x | 0.23x | N/A |
| allocator cycle latency/small_32 | 3.834 | 24.409 | 2.751 | 14.905 | N/A | 0.16x | 1.39x | 0.26x | N/A |
| allocator deallocation latency/medium_1024 | 106.772 | 99.880 | 111.931 | 82.915 | N/A | 1.07x | 0.95x | 1.29x | N/A |
| allocator deallocation latency/small_32 | 5.861 | 18.408 | 5.550 | 9.721 | N/A | 0.32x | 1.06x | 0.60x | N/A |
| cross-thread free handoff/medium_1024 | 13939.801 | 35220.276 | 170235.034 | 38414.996 | N/A | 0.40x | 0.08x | 0.36x | N/A |
| cross-thread free handoff/small_32 | 7013.257 | 32932.422 | 8595.723 | 21617.334 | N/A | 0.21x | 0.82x | 0.32x | N/A |
| realloc latency/cross_class_32_to_64 | 11.556 | 54.960 | 9.185 | 32.804 | N/A | 0.21x | 1.26x | 0.35x | N/A |
| realloc latency/within_class_24_to_32 | 5.412 | 45.535 | 5.524 | 16.761 | N/A | 0.12x | 0.98x | 0.32x | N/A |
| segment cache eviction | 65130.762 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 104849.023 | 394683.594 | 78212.646 | 267621.289 | N/A | 0.27x | 1.34x | 0.39x | N/A |
| threaded small allocation cycles | 8920.557 | 34432.593 | 5269.980 | 23019.006 | N/A | 0.26x | 1.69x | 0.39x | N/A |
| usable size latency/medium_1024 | 5.433 | N/A | 6.515 | 16.729 | N/A | N/A | 0.83x | 0.32x | N/A |
| usable size latency/small_32 | 4.001 | N/A | 2.900 | 16.600 | N/A | N/A | 1.38x | 0.24x | N/A |
| usable size query latency/medium_1024 | 0.402 | N/A | 0.631 | 0.591 | N/A | N/A | 0.64x | 0.68x | N/A |
| usable size query latency/small_32 | 0.380 | N/A | 0.688 | 0.536 | N/A | N/A | 0.55x | 0.71x | N/A |
