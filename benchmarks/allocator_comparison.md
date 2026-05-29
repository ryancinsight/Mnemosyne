# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 26.122 | 133.967 | 270.344 | 106.723 | N/A | 0.19x | 0.10x | 0.24x | N/A |
| allocator allocation latency/small_32 | 10.922 | 32.515 | 18.288 | 15.894 | N/A | 0.34x | 0.60x | 0.69x | N/A |
| allocator burst retention/large_8192 | 5932.369 | 9397.288 | 403791.300 | 19072.320 | N/A | 0.63x | 0.01x | 0.31x | N/A |
| allocator burst retention/medium_1024 | 1924.226 | 7080.193 | 79890.754 | 7397.458 | N/A | 0.27x | 0.02x | 0.26x | N/A |
| allocator burst retention/small_32 | 969.804 | 6433.536 | 824.928 | 4185.555 | N/A | 0.15x | 1.18x | 0.23x | N/A |
| allocator cycle latency/large_8192 | 4.211 | 21.345 | 15.776 | 17.551 | N/A | 0.20x | 0.27x | 0.24x | N/A |
| allocator cycle latency/medium_1024 | 4.440 | 21.932 | 6.267 | 16.827 | N/A | 0.20x | 0.71x | 0.26x | N/A |
| allocator cycle latency/small_32 | 3.534 | 20.174 | 2.807 | 16.311 | N/A | 0.18x | 1.26x | 0.22x | N/A |
| allocator deallocation latency/medium_1024 | 27.261 | 42.780 | 92.190 | 52.522 | N/A | 0.64x | 0.30x | 0.52x | N/A |
| allocator deallocation latency/small_32 | 4.477 | 11.694 | 5.136 | 10.005 | N/A | 0.38x | 0.87x | 0.45x | N/A |
| cross-thread free handoff/medium_1024 | 13685.785 | 33350.309 | 180279.479 | 38321.002 | N/A | 0.41x | 0.08x | 0.36x | N/A |
| cross-thread free handoff/small_32 | 15854.660 | 27629.194 | 16275.022 | 18280.091 | N/A | 0.57x | 0.97x | 0.87x | N/A |
| realloc latency/cross_class_32_to_64 | 10.080 | 43.301 | 7.788 | 33.795 | N/A | 0.23x | 1.29x | 0.30x | N/A |
| realloc latency/within_class_24_to_32 | 4.399 | 42.919 | 4.471 | 17.534 | N/A | 0.10x | 0.98x | 0.25x | N/A |
| segment cache eviction | 93735.421 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 89874.649 | 474480.447 | 74957.109 | 268508.492 | N/A | 0.19x | 1.20x | 0.33x | N/A |
| threaded small allocation cycles | 6791.987 | 34737.571 | 6028.400 | 24409.817 | N/A | 0.20x | 1.13x | 0.28x | N/A |
| usable size latency/medium_1024 | 3.793 | N/A | 6.177 | 17.235 | N/A | N/A | 0.61x | 0.22x | N/A |
| usable size latency/small_32 | 5.590 | N/A | 3.949 | 17.067 | N/A | N/A | 1.42x | 0.33x | N/A |
| usable size query latency/medium_1024 | 0.321 | N/A | 0.531 | 0.454 | N/A | N/A | 0.61x | 0.71x | N/A |
| usable size query latency/small_32 | 0.320 | N/A | 0.602 | 0.462 | N/A | N/A | 0.53x | 0.69x | N/A |
