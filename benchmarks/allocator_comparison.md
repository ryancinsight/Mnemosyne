# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 18.946 | 105.100 | 380.838 | 78.327 | N/A | 0.18x | 0.05x | 0.24x | N/A |
| allocator allocation latency/small_32 | 12.443 | 25.042 | 19.113 | 15.344 | N/A | 0.50x | 0.65x | 0.81x | N/A |
| allocator burst retention/large_8192 | 6264.226 | 12433.662 | 586019.692 | 19635.589 | N/A | 0.50x | 0.01x | 0.32x | N/A |
| allocator burst retention/medium_1024 | 2053.848 | 7886.184 | 84281.447 | 7418.435 | N/A | 0.26x | 0.02x | 0.28x | N/A |
| allocator burst retention/small_32 | 1211.880 | 6879.537 | 918.301 | 4050.872 | N/A | 0.18x | 1.32x | 0.30x | N/A |
| allocator cycle latency/large_8192 | 4.143 | 21.290 | 16.452 | 17.355 | N/A | 0.19x | 0.25x | 0.24x | N/A |
| allocator cycle latency/medium_1024 | 4.243 | 21.471 | 6.123 | 16.480 | N/A | 0.20x | 0.69x | 0.26x | N/A |
| allocator cycle latency/small_32 | 3.095 | 21.131 | 2.806 | 15.632 | N/A | 0.15x | 1.10x | 0.20x | N/A |
| allocator deallocation latency/medium_1024 | 23.087 | 38.690 | 133.087 | 63.766 | N/A | 0.60x | 0.17x | 0.36x | N/A |
| allocator deallocation latency/small_32 | 4.660 | 11.795 | 6.050 | 9.512 | N/A | 0.40x | 0.77x | 0.49x | N/A |
| cross-thread free handoff/medium_1024 | 11959.002 | 36996.755 | 181310.248 | 36608.041 | N/A | 0.32x | 0.07x | 0.33x | N/A |
| cross-thread free handoff/small_32 | 6288.433 | 28973.556 | 6855.199 | 21024.377 | N/A | 0.22x | 0.92x | 0.30x | N/A |
| realloc latency/cross_class_32_to_64 | 10.187 | 50.740 | 9.340 | 32.821 | N/A | 0.20x | 1.09x | 0.31x | N/A |
| realloc latency/within_class_24_to_32 | 4.891 | 43.850 | 6.576 | 17.080 | N/A | 0.11x | 0.74x | 0.29x | N/A |
| segment cache eviction | 126709.401 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 94717.681 | 413217.224 | 77361.573 | 276839.877 | N/A | 0.23x | 1.22x | 0.34x | N/A |
| threaded small allocation cycles | 6207.666 | 34894.632 | 6386.402 | 26218.230 | N/A | 0.18x | 0.97x | 0.24x | N/A |
| usable size latency/medium_1024 | 4.692 | N/A | 6.752 | 16.455 | N/A | N/A | 0.69x | 0.29x | N/A |
| usable size latency/small_32 | 4.028 | N/A | 3.770 | 15.680 | N/A | N/A | 1.07x | 0.26x | N/A |
| usable size query latency/medium_1024 | 0.422 | N/A | 0.768 | 0.693 | N/A | N/A | 0.55x | 0.61x | N/A |
| usable size query latency/small_32 | 0.421 | N/A | 0.971 | 0.701 | N/A | N/A | 0.43x | 0.60x | N/A |
