# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 21.604 | 111.950 | 262.898 | 80.611 | N/A | 0.19x | 0.08x | 0.27x | N/A |
| allocator allocation latency/small_32 | 11.206 | 31.818 | 15.354 | 14.208 | N/A | 0.35x | 0.73x | 0.79x | N/A |
| allocator burst retention/large_8192 | 3815.732 | 9092.154 | 398524.264 | 20455.283 | N/A | 0.42x | 0.01x | 0.19x | N/A |
| allocator burst retention/medium_1024 | 1468.060 | 6615.788 | 80277.205 | 8244.089 | N/A | 0.22x | 0.02x | 0.18x | N/A |
| allocator burst retention/small_32 | 492.049 | 6326.095 | 827.764 | 4203.025 | N/A | 0.08x | 0.59x | 0.12x | N/A |
| allocator cycle latency/large_8192 | 2.313 | 22.534 | 20.798 | 18.689 | N/A | 0.10x | 0.11x | 0.12x | N/A |
| allocator cycle latency/medium_1024 | 2.260 | 22.821 | 6.320 | 17.575 | N/A | 0.10x | 0.36x | 0.13x | N/A |
| allocator cycle latency/small_32 | 1.989 | 21.358 | 3.148 | 17.132 | N/A | 0.09x | 0.63x | 0.12x | N/A |
| allocator deallocation latency/medium_1024 | 26.702 | 37.006 | 82.362 | 47.230 | N/A | 0.72x | 0.32x | 0.57x | N/A |
| allocator deallocation latency/small_32 | 4.237 | 11.535 | 4.953 | 9.574 | N/A | 0.37x | 0.86x | 0.44x | N/A |
| cross-thread free handoff/medium_1024 | 17384.083 | 31135.990 | 147207.738 | 35184.446 | N/A | 0.56x | 0.12x | 0.49x | N/A |
| cross-thread free handoff/small_32 | 14195.026 | 27564.882 | 15185.857 | 18157.680 | N/A | 0.51x | 0.93x | 0.78x | N/A |
| realloc latency/cross_class_32_to_64 | 5.329 | 42.703 | 7.455 | 33.052 | N/A | 0.12x | 0.71x | 0.16x | N/A |
| realloc latency/within_class_24_to_32 | 2.266 | 42.923 | 4.366 | 17.100 | N/A | 0.05x | 0.52x | 0.13x | N/A |
| segment cache eviction | 132915.391 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 38532.370 | 343419.291 | 60169.559 | 289713.622 | N/A | 0.11x | 0.64x | 0.13x | N/A |
| threaded small allocation cycles | 3670.150 | 28524.128 | 4684.654 | 22756.824 | N/A | 0.13x | 0.78x | 0.16x | N/A |
| usable size latency/medium_1024 | 2.132 | N/A | 6.126 | 16.769 | N/A | N/A | 0.35x | 0.13x | N/A |
| usable size latency/small_32 | 2.038 | N/A | 2.832 | 16.503 | N/A | N/A | 0.72x | 0.12x | N/A |
| usable size query latency/medium_1024 | 0.316 | N/A | 0.530 | 0.451 | N/A | N/A | 0.60x | 0.70x | N/A |
| usable size query latency/small_32 | 0.316 | N/A | 0.547 | 0.450 | N/A | N/A | 0.58x | 0.70x | N/A |
