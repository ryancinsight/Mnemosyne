# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 745.097 | 2801.644 | 4241.128 | N/A | 1946.042 | 0.27x | 0.18x | N/A | 0.38x |
| allocator allocation latency/large_8192 | 21.552 | 303.770 | 1217.154 | 422.496 | 78.048 | 0.07x | 0.02x | 0.05x | 0.28x |
| allocator allocation latency/medium_1024 | 12.038 | 60.459 | 266.428 | 88.946 | 29.670 | 0.20x | 0.05x | 0.14x | 0.41x |
| allocator allocation latency/small_32 | 11.212 | 21.459 | 17.308 | 14.915 | 14.008 | 0.52x | 0.65x | 0.75x | 0.80x |
| allocator burst retention/large_8192 | 2857.455 | 9601.801 | 401566.247 | 19886.414 | 26820.326 | 0.30x | 0.01x | 0.14x | 0.11x |
| allocator burst retention/medium_1024 | 931.994 | 6708.843 | 77487.925 | 7401.095 | 8775.474 | 0.14x | 0.01x | 0.13x | 0.11x |
| allocator burst retention/small_32 | 528.086 | 6288.995 | 824.219 | 4198.816 | 2610.104 | 0.08x | 0.64x | 0.13x | 0.20x |
| allocator cycle latency/huge_2m | 23.433 | 7832.011 | 8611.420 | N/A | 113.362 | 0.00x | 0.00x | N/A | 0.21x |
| allocator cycle latency/large_8192 | 8.615 | 20.438 | 16.858 | 17.353 | 15.257 | 0.42x | 0.51x | 0.50x | 0.56x |
| allocator cycle latency/medium_1024 | 7.687 | 20.298 | 5.641 | 16.640 | 7.224 | 0.38x | 1.36x | 0.46x | 1.06x |
| allocator cycle latency/small_32 | 7.604 | 20.345 | 2.760 | 16.361 | 6.870 | 0.37x | 2.76x | 0.46x | 1.11x |
| allocator deallocation latency/huge_2m | 1191.874 | 4055.045 | 4321.805 | N/A | 2866.217 | 0.29x | 0.28x | N/A | 0.42x |
| allocator deallocation latency/large_8192 | 15.199 | 79.114 | 514.298 | 151.499 | 49.220 | 0.19x | 0.03x | 0.10x | 0.31x |
| allocator deallocation latency/medium_1024 | 8.895 | 29.858 | 75.955 | 36.514 | 17.307 | 0.30x | 0.12x | 0.24x | 0.51x |
| allocator deallocation latency/small_32 | 3.099 | 10.565 | 5.180 | 10.260 | 6.662 | 0.29x | 0.60x | 0.30x | 0.47x |
| cross-thread free handoff/huge_2m | 1529.530 | 66194.386 | 86016.056 | N/A | 4886.458 | 0.02x | 0.02x | N/A | 0.31x |
| cross-thread free handoff/large_8192 | 29172.876 | 54512.450 | 879764.398 | 93119.927 | 73692.977 | 0.54x | 0.03x | 0.31x | 0.40x |
| cross-thread free handoff/medium_1024 | 18648.991 | 31557.726 | 144882.020 | 37880.319 | 36615.901 | 0.59x | 0.13x | 0.49x | 0.51x |
| cross-thread free handoff/small_32 | 14601.672 | 28001.833 | 15702.779 | 17809.913 | 25702.492 | 0.52x | 0.93x | 0.82x | 0.57x |
| realloc latency/cross_class_32_to_64 | 8.716 | 42.493 | 7.648 | 32.846 | 17.310 | 0.21x | 1.14x | 0.27x | 0.50x |
| realloc latency/cross_class_8k_to_16k | 47.412 | 133.662 | 67.304 | 131.003 | 55.263 | 0.35x | 0.70x | 0.36x | 0.86x |
| realloc latency/huge_shrink_4m_to_2m | 22.464 | 1171115.751 | 7592.505 | 1150543.543 | 244.788 | 0.00x | 0.00x | 0.00x | 0.09x |
| realloc latency/within_class_24_to_32 | 3.436 | 42.253 | 4.434 | 17.117 | 15.117 | 0.08x | 0.77x | 0.20x | 0.23x |
| realloc latency/within_class_6k_to_8k | 23.006 | 99.276 | 55.887 | 95.529 | 51.362 | 0.23x | 0.41x | 0.24x | 0.45x |
| segment cache eviction | 206477.345 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 54435.930 | 352691.241 | 68792.663 | 266105.543 | 126254.663 | 0.15x | 0.79x | 0.20x | 0.43x |
| threaded small allocation cycles | 5434.241 | 30999.942 | 6834.716 | 23456.092 | 14488.985 | 0.18x | 0.80x | 0.23x | 0.38x |
| usable size latency/huge_2m | 22.190 | N/A | 7703.079 | N/A | 116.158 | N/A | 0.00x | N/A | 0.19x |
| usable size latency/large_8192 | 2.327 | N/A | 16.389 | 17.453 | 17.517 | N/A | 0.14x | 0.13x | 0.13x |
| usable size latency/medium_1024 | 3.209 | N/A | 5.919 | 16.843 | 10.183 | N/A | 0.54x | 0.19x | 0.32x |
| usable size latency/small_32 | 3.208 | N/A | 2.842 | 16.316 | 9.819 | N/A | 1.13x | 0.20x | 0.33x |
| usable size query latency/huge_2m | 0.395 | N/A | 0.523 | N/A | 3.190 | N/A | 0.76x | N/A | 0.12x |
| usable size query latency/large_8192 | 0.273 | N/A | 0.532 | 0.456 | 3.193 | N/A | 0.51x | 0.60x | 0.09x |
| usable size query latency/medium_1024 | 0.282 | N/A | 0.524 | 0.452 | 3.213 | N/A | 0.54x | 0.62x | 0.09x |
| usable size query latency/small_32 | 0.278 | N/A | 0.523 | 0.457 | 3.216 | N/A | 0.53x | 0.61x | 0.09x |
