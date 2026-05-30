# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 134.881 | 217.729 | 287.986 | 146.789 | N/A | 0.62x | 0.47x | 0.92x | N/A |
| allocator allocation latency/small_32 | 12.880 | 33.890 | 16.913 | 19.348 | N/A | 0.38x | 0.76x | 0.67x | N/A |
| allocator burst retention/large_8192 | 3271.762 | 10629.163 | 404764.453 | 19823.047 | N/A | 0.31x | 0.01x | 0.17x | N/A |
| allocator burst retention/medium_1024 | 1459.855 | 6504.700 | 80758.350 | 7368.658 | N/A | 0.22x | 0.02x | 0.20x | N/A |
| allocator burst retention/small_32 | 504.615 | 6437.024 | 802.272 | 4296.564 | N/A | 0.08x | 0.63x | 0.12x | N/A |
| allocator cycle latency/large_8192 | 2.035 | 22.428 | 16.757 | 17.233 | N/A | 0.09x | 0.12x | 0.12x | N/A |
| allocator cycle latency/medium_1024 | 2.008 | 20.780 | 5.958 | 16.772 | N/A | 0.10x | 0.34x | 0.12x | N/A |
| allocator cycle latency/small_32 | 1.876 | 19.831 | 2.771 | 14.974 | N/A | 0.09x | 0.68x | 0.13x | N/A |
| allocator deallocation latency/medium_1024 | 91.104 | 98.910 | 111.492 | 75.358 | N/A | 0.92x | 0.82x | 1.21x | N/A |
| allocator deallocation latency/small_32 | 5.853 | 19.109 | 6.492 | 9.895 | N/A | 0.31x | 0.90x | 0.59x | N/A |
| cross-thread free handoff/medium_1024 | 18883.289 | 33652.368 | 153340.527 | 37938.672 | N/A | 0.56x | 0.12x | 0.50x | N/A |
| cross-thread free handoff/small_32 | 13209.460 | 29055.347 | 16774.622 | 19982.117 | N/A | 0.45x | 0.79x | 0.66x | N/A |
| realloc latency/cross_class_32_to_64 | 5.218 | 45.446 | 8.868 | 32.696 | N/A | 0.11x | 0.59x | 0.16x | N/A |
| realloc latency/huge_shrink_4m_to_2m | 19163.965 | 17252.283 | 1117126.562 | 1125162.500 | N/A | 1.11x | 0.02x | 0.02x | N/A |
| realloc latency/within_class_24_to_32 | 2.184 | 43.222 | 4.534 | 17.186 | N/A | 0.05x | 0.48x | 0.13x | N/A |
| segment cache eviction | 142153.711 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 41154.126 | 357141.406 | 62397.510 | 263053.711 | N/A | 0.12x | 0.66x | 0.16x | N/A |
| threaded small allocation cycles | 3400.568 | 30414.429 | 4564.813 | 24537.280 | N/A | 0.11x | 0.74x | 0.14x | N/A |
| usable size latency/medium_1024 | 2.108 | N/A | 6.136 | 16.733 | N/A | N/A | 0.34x | 0.13x | N/A |
| usable size latency/small_32 | 2.023 | N/A | 2.859 | 16.251 | N/A | N/A | 0.71x | 0.12x | N/A |
| usable size query latency/medium_1024 | 0.307 | N/A | 0.538 | 0.459 | N/A | N/A | 0.57x | 0.67x | N/A |
| usable size query latency/small_32 | 0.307 | N/A | 0.552 | 0.450 | N/A | N/A | 0.56x | 0.68x | N/A |
