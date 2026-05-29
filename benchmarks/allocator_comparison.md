# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 115.404 | 179.044 | 233.292 | 109.390 | N/A | 0.64x | 0.49x | 1.05x | N/A |
| allocator allocation latency/small_32 | 12.475 | 35.060 | 14.716 | 19.117 | N/A | 0.36x | 0.85x | 0.65x | N/A |
| allocator burst retention/large_8192 | 6011.786 | 10432.025 | 391192.188 | 19366.687 | N/A | 0.58x | 0.02x | 0.31x | N/A |
| allocator burst retention/medium_1024 | 2564.490 | 6761.053 | 79506.494 | 7491.071 | N/A | 0.38x | 0.03x | 0.34x | N/A |
| allocator burst retention/small_32 | 2580.449 | 6272.937 | 785.521 | 4185.602 | N/A | 0.41x | 3.29x | 0.62x | N/A |
| allocator cycle latency/large_8192 | 21.094 | 19.631 | 16.824 | 17.255 | N/A | 1.07x | 1.25x | 1.22x | N/A |
| allocator cycle latency/medium_1024 | 21.153 | 20.044 | 5.981 | 16.622 | N/A | 1.06x | 3.54x | 1.27x | N/A |
| allocator cycle latency/small_32 | 21.119 | 19.904 | 2.745 | 14.923 | N/A | 1.06x | 7.69x | 1.42x | N/A |
| allocator deallocation latency/medium_1024 | 74.934 | 86.104 | 106.011 | 75.927 | N/A | 0.87x | 0.71x | 0.99x | N/A |
| allocator deallocation latency/small_32 | 5.927 | 18.421 | 5.051 | 9.604 | N/A | 0.32x | 1.17x | 0.62x | N/A |
| cross-thread free handoff/medium_1024 | 26646.558 | 30531.952 | 144586.230 | 35622.705 | N/A | 0.87x | 0.18x | 0.75x | N/A |
| cross-thread free handoff/small_32 | 19918.701 | 29676.465 | 16039.160 | 18665.628 | N/A | 0.67x | 1.24x | 1.07x | N/A |
| realloc latency/cross_class_32_to_64 | 10.024 | 43.609 | 8.656 | 32.966 | N/A | 0.23x | 1.16x | 0.30x | N/A |
| realloc latency/within_class_24_to_32 | 21.429 | 44.106 | 4.506 | 17.460 | N/A | 0.49x | 4.76x | 1.23x | N/A |
| segment cache eviction | 138863.477 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 451774.414 | 373144.336 | 58197.290 | 275326.367 | N/A | 1.21x | 7.76x | 1.64x | N/A |
| threaded small allocation cycles | 38777.368 | 30267.554 | 5615.543 | 22707.874 | N/A | 1.28x | 6.91x | 1.71x | N/A |
| usable size latency/medium_1024 | 21.203 | N/A | 6.152 | 16.692 | N/A | N/A | 3.45x | 1.27x | N/A |
| usable size latency/small_32 | 21.386 | N/A | 2.880 | 16.348 | N/A | N/A | 7.42x | 1.31x | N/A |
| usable size query latency/medium_1024 | 0.317 | N/A | 0.521 | 0.450 | N/A | N/A | 0.61x | 0.70x | N/A |
| usable size query latency/small_32 | 0.314 | N/A | 0.531 | 0.451 | N/A | N/A | 0.59x | 0.70x | N/A |
