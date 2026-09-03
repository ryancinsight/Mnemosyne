# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | RpMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs RpMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 2745.463 | 2934.753 | 894.479 | 2266.911 | N/A | N/A | 0.94x | 3.07x | 1.21x | N/A | N/A |
| allocator allocation latency/large_8192 | 19.494 | 276.939 | 67.501 | 22.609 | N/A | N/A | 0.07x | 0.29x | 0.86x | N/A | N/A |
| allocator allocation latency/medium_1024 | 11.284 | 48.287 | 25.519 | 23.851 | N/A | N/A | 0.23x | 0.44x | 0.47x | N/A | N/A |
| allocator allocation latency/small_32 | 9.772 | 26.341 | 7.715 | 8.181 | N/A | N/A | 0.37x | 1.27x | 1.19x | N/A | N/A |
| allocator burst retention/large_8192 | 2641.671 | 9090.886 | 13931.094 | 1604.096 | N/A | N/A | 0.29x | 0.19x | 1.65x | N/A | N/A |
| allocator burst retention/medium_1024 | 1202.232 | 6720.236 | 2335.415 | 1275.899 | N/A | N/A | 0.18x | 0.51x | 0.94x | N/A | N/A |
| allocator burst retention/small_32 | 1072.686 | 6145.479 | 721.221 | 755.906 | N/A | N/A | 0.17x | 1.49x | 1.42x | N/A | N/A |
| allocator cycle latency/huge_2m | 53.729 | 8309.249 | 219.511 | 6.743 | N/A | N/A | 0.01x | 0.24x | 7.97x | N/A | N/A |
| allocator cycle latency/large_8192 | 4.572 | 21.548 | 14.408 | 5.157 | N/A | N/A | 0.21x | 0.32x | 0.89x | N/A | N/A |
| allocator cycle latency/medium_1024 | 4.582 | 20.270 | 4.978 | 3.930 | N/A | N/A | 0.23x | 0.92x | 1.17x | N/A | N/A |
| allocator cycle latency/small_32 | 4.567 | 20.107 | 3.246 | 3.240 | N/A | N/A | 0.23x | 1.41x | 1.41x | N/A | N/A |
| allocator deallocation latency/huge_2m | 4477.312 | 4231.750 | 103.799 | 1885.542 | N/A | N/A | 1.06x | 43.13x | 2.37x | N/A | N/A |
| allocator deallocation latency/large_8192 | 14.266 | 80.399 | 34.577 | 16.984 | N/A | N/A | 0.18x | 0.41x | 0.84x | N/A | N/A |
| allocator deallocation latency/medium_1024 | 7.045 | 20.975 | 8.730 | 6.452 | N/A | N/A | 0.34x | 0.81x | 1.09x | N/A | N/A |
| allocator deallocation latency/small_32 | 3.782 | 12.449 | 2.626 | 2.631 | N/A | N/A | 0.30x | 1.44x | 1.44x | N/A | N/A |
| cross-thread free handoff/huge_2m | 1437.356 | 72726.341 | 4383.549 | 2169.050 | N/A | N/A | 0.02x | 0.33x | 0.66x | N/A | N/A |
| cross-thread free handoff/large_8192 | 29186.232 | 55300.793 | 54528.411 | 24129.998 | N/A | N/A | 0.53x | 0.54x | 1.21x | N/A | N/A |
| cross-thread free handoff/medium_1024 | 20227.741 | 32831.851 | 22703.041 | 18173.761 | N/A | N/A | 0.62x | 0.89x | 1.11x | N/A | N/A |
| cross-thread free handoff/small_32 | 15786.175 | 28375.434 | 15401.546 | 14010.874 | N/A | N/A | 0.56x | 1.02x | 1.13x | N/A | N/A |
| leak detector allocator cycle latency/large_8192 | 775.562 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| leak detector allocator cycle latency/medium_1024 | 777.064 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| leak detector allocator cycle latency/small_32 | 774.205 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| realloc latency/cross_class_32_to_64 | 12.909 | 42.553 | 7.822 | 8.398 | N/A | N/A | 0.30x | 1.65x | 1.54x | N/A | N/A |
| realloc latency/cross_class_8k_to_16k | 43.119 | 129.355 | 62.814 | 80.470 | N/A | N/A | 0.33x | 0.69x | 0.54x | N/A | N/A |
| realloc latency/huge_shrink_4m_to_2m | 53.743 | 995005.980 | 302.969 | 585607.441 | N/A | N/A | 0.00x | 0.18x | 0.00x | N/A | N/A |
| realloc latency/within_class_24_to_32 | 8.434 | 42.311 | 4.052 | 8.069 | N/A | N/A | 0.20x | 2.08x | 1.05x | N/A | N/A |
| realloc latency/within_class_6k_to_8k | 28.673 | 99.468 | 49.006 | 83.158 | N/A | N/A | 0.29x | 0.59x | 0.34x | N/A | N/A |
| segment cache eviction | 237312.576 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 14244.078 | 29965.384 | 13744.900 | 12340.885 | N/A | N/A | 0.48x | 1.04x | 1.15x | N/A | N/A |
| threaded saturated small allocation cycles | 91355.643 | 354862.555 | 63416.958 | 62159.348 | N/A | N/A | 0.26x | 1.44x | 1.47x | N/A | N/A |
| threaded small allocation cycles | 13198.889 | 30873.757 | 7142.758 | 5711.076 | N/A | N/A | 0.43x | 1.85x | 2.31x | N/A | N/A |
| usable size latency/huge_2m | 51.849 | N/A | 219.979 | N/A | N/A | N/A | N/A | 0.24x | N/A | N/A | N/A |
| usable size latency/large_8192 | 4.622 | N/A | 14.743 | N/A | N/A | N/A | N/A | 0.31x | N/A | N/A | N/A |
| usable size latency/medium_1024 | 6.989 | N/A | 5.505 | N/A | N/A | N/A | N/A | 1.27x | N/A | N/A | N/A |
| usable size latency/small_32 | 6.974 | N/A | 2.811 | N/A | N/A | N/A | N/A | 2.48x | N/A | N/A | N/A |
| usable size query latency/huge_2m | 0.404 | N/A | 0.541 | N/A | N/A | N/A | N/A | 0.75x | N/A | N/A | N/A |
| usable size query latency/large_8192 | 0.318 | N/A | 0.552 | N/A | N/A | N/A | N/A | 0.58x | N/A | N/A | N/A |
| usable size query latency/medium_1024 | 0.322 | N/A | 0.546 | N/A | N/A | N/A | N/A | 0.59x | N/A | N/A | N/A |
| usable size query latency/small_32 | 0.321 | N/A | 0.543 | N/A | N/A | N/A | N/A | 0.59x | N/A | N/A | N/A |
