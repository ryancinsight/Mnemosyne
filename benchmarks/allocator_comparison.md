# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 147.142 | 257.643 | 283.303 | 171.802 | N/A | 0.57x | 0.52x | 0.86x | N/A |
| allocator allocation latency/small_32 | 15.413 | 35.949 | 18.028 | 19.896 | N/A | 0.43x | 0.85x | 0.77x | N/A |
| allocator burst retention/large_8192 | 6915.796 | 11001.959 | 468189.062 | 20689.551 | N/A | 0.63x | 0.01x | 0.33x | N/A |
| allocator burst retention/medium_1024 | 1757.752 | 7694.623 | 94704.492 | 7462.231 | N/A | 0.23x | 0.02x | 0.24x | N/A |
| allocator burst retention/small_32 | 1490.839 | 7323.492 | 1159.780 | 4174.310 | N/A | 0.20x | 1.29x | 0.36x | N/A |
| allocator cycle latency/large_8192 | 5.518 | 21.189 | 16.747 | 17.229 | N/A | 0.26x | 0.33x | 0.32x | N/A |
| allocator cycle latency/medium_1024 | 5.470 | 22.920 | 6.713 | 16.754 | N/A | 0.24x | 0.81x | 0.33x | N/A |
| allocator cycle latency/small_32 | 5.308 | 20.046 | 2.768 | 15.072 | N/A | 0.26x | 1.92x | 0.35x | N/A |
| allocator deallocation latency/medium_1024 | 120.404 | 104.459 | 114.488 | 83.729 | N/A | 1.15x | 1.05x | 1.44x | N/A |
| allocator deallocation latency/small_32 | 5.886 | 33.471 | 9.677 | 10.455 | N/A | 0.18x | 0.61x | 0.56x | N/A |
| cross-thread free handoff/medium_1024 | 11749.930 | 43852.808 | 203255.859 | 42601.221 | N/A | 0.27x | 0.06x | 0.28x | N/A |
| cross-thread free handoff/small_32 | 9516.612 | 37468.042 | 8884.113 | 19924.976 | N/A | 0.25x | 1.07x | 0.48x | N/A |
| realloc latency/cross_class_32_to_64 | 13.925 | 54.763 | 11.924 | 33.009 | N/A | 0.25x | 1.17x | 0.42x | N/A |
| realloc latency/within_class_24_to_32 | 6.986 | 54.650 | 5.776 | 16.339 | N/A | 0.13x | 1.21x | 0.43x | N/A |
| segment cache eviction | 89803.430 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 106255.273 | 396813.672 | 91090.820 | 291500.000 | N/A | 0.27x | 1.17x | 0.36x | N/A |
| threaded small allocation cycles | 11391.400 | 36398.608 | 8778.137 | 28234.570 | N/A | 0.31x | 1.30x | 0.40x | N/A |
| usable size latency/medium_1024 | 6.231 | N/A | 6.926 | 16.560 | N/A | N/A | 0.90x | 0.38x | N/A |
| usable size latency/small_32 | 6.496 | N/A | 3.927 | 15.922 | N/A | N/A | 1.65x | 0.41x | N/A |
| usable size query latency/medium_1024 | 0.436 | N/A | 0.885 | 0.564 | N/A | N/A | 0.49x | 0.77x | N/A |
| usable size query latency/small_32 | 0.433 | N/A | 0.739 | 0.670 | N/A | N/A | 0.59x | 0.65x | N/A |
