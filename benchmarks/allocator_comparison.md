# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 173.131 | 308.245 | 353.910 | 189.147 | N/A | 0.56x | 0.49x | 0.92x | N/A |
| allocator allocation latency/small_32 | 14.860 | 34.718 | 18.809 | 15.411 | N/A | 0.43x | 0.79x | 0.96x | N/A |
| allocator burst retention/large_8192 | 6623.889 | 11787.201 | 504795.312 | 20568.811 | N/A | 0.56x | 0.01x | 0.32x | N/A |
| allocator burst retention/medium_1024 | 1711.771 | 7459.592 | 89783.105 | 7415.186 | N/A | 0.23x | 0.02x | 0.23x | N/A |
| allocator burst retention/small_32 | 1575.529 | 7220.030 | 1219.671 | 4086.581 | N/A | 0.22x | 1.29x | 0.39x | N/A |
| allocator cycle latency/large_8192 | 6.938 | 21.905 | 15.251 | 17.239 | N/A | 0.32x | 0.45x | 0.40x | N/A |
| allocator cycle latency/medium_1024 | 5.317 | 20.465 | 5.660 | 16.735 | N/A | 0.26x | 0.94x | 0.32x | N/A |
| allocator cycle latency/small_32 | 5.286 | 20.075 | 2.815 | 15.083 | N/A | 0.26x | 1.88x | 0.35x | N/A |
| allocator deallocation latency/medium_1024 | 106.293 | 101.928 | 138.628 | 97.295 | N/A | 1.04x | 0.77x | 1.09x | N/A |
| allocator deallocation latency/small_32 | 6.113 | 22.094 | 8.498 | 10.076 | N/A | 0.28x | 0.72x | 0.61x | N/A |
| cross-thread free handoff/medium_1024 | 18919.446 | 40189.893 | 194390.869 | 42547.510 | N/A | 0.47x | 0.10x | 0.44x | N/A |
| cross-thread free handoff/small_32 | 11996.242 | 33391.046 | 7417.236 | 22245.587 | N/A | 0.36x | 1.62x | 0.54x | N/A |
| realloc latency/cross_class_32_to_64 | 14.632 | 49.695 | 13.845 | 32.466 | N/A | 0.29x | 1.06x | 0.45x | N/A |
| realloc latency/within_class_24_to_32 | 6.513 | 46.344 | 5.221 | 16.735 | N/A | 0.14x | 1.25x | 0.39x | N/A |
| segment cache eviction | 70024.695 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 106067.383 | 399397.266 | 81084.766 | 268170.312 | N/A | 0.27x | 1.31x | 0.40x | N/A |
| threaded small allocation cycles | 11833.110 | 38425.098 | 6320.190 | 27211.945 | N/A | 0.31x | 1.87x | 0.43x | N/A |
| usable size latency/medium_1024 | 6.256 | N/A | 6.423 | 16.603 | N/A | N/A | 0.97x | 0.38x | N/A |
| usable size latency/small_32 | 5.741 | N/A | 3.933 | 15.673 | N/A | N/A | 1.46x | 0.37x | N/A |
| usable size query latency/medium_1024 | 0.403 | N/A | 0.876 | 0.576 | N/A | N/A | 0.46x | 0.70x | N/A |
| usable size query latency/small_32 | 0.417 | N/A | 0.745 | 0.639 | N/A | N/A | 0.56x | 0.65x | N/A |
