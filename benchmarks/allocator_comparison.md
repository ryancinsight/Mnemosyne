# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MiMalloc (ns) | SnMalloc (ns) | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc |
| :--- | :---: | :---: | :---: | :---: | :---: |
| allocator burst retention/large_8192 | 8097.886 | 546531.210 | 19932.688 | 0.01x | 0.41x |
| allocator burst retention/medium_1024 | 3550.455 | 88419.780 | 7300.294 | 0.04x | 0.49x |
| allocator burst retention/small_32 | 3358.014 | 997.006 | 4140.650 | 3.37x | 0.81x |
| allocator cycle latency/large_8192 | 13.673 | 15.594 | 17.603 | 0.88x | 0.78x |
| allocator cycle latency/medium_1024 | 13.243 | 6.270 | 16.762 | 2.11x | 0.79x |
| allocator cycle latency/small_32 | 12.925 | 2.794 | 16.293 | 4.63x | 0.79x |
| cross-thread free handoff/medium_1024 | 26913.384 | 185155.442 | 38601.196 | 0.15x | 0.70x |
| cross-thread free handoff/small_32 | 19197.338 | 7369.711 | 23013.837 | 2.60x | 0.83x |
| segment cache eviction | 61139.487 | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 208204.245 | 79659.639 | 282682.156 | 2.61x | 0.74x |
| threaded small allocation cycles | 38537.584 | 6611.215 | 26321.064 | 5.83x | 1.46x |
