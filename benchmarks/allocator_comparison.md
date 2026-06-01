# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MnemosyneHeap (ns) | BrandedHeap (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc | MnemosyneHeap vs Mnemosyne | BrandedHeap vs Mnemosyne |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 551.199 | N/A | N/A | 3479.757 | 4616.423 | N/A | 1732.458 | 0.16x | 0.12x | N/A | 0.32x | N/A | N/A |
| allocator allocation latency/large_8192 | 65.936 | N/A | N/A | 364.253 | 1399.069 | 415.106 | 75.731 | 0.18x | 0.05x | 0.16x | 0.87x | N/A | N/A |
| allocator allocation latency/medium_1024 | 13.630 | N/A | N/A | 49.254 | 296.090 | 65.389 | 29.162 | 0.28x | 0.05x | 0.21x | 0.47x | N/A | N/A |
| allocator allocation latency/small_32 | 8.434 | N/A | N/A | 21.586 | 15.663 | 14.335 | 12.898 | 0.39x | 0.54x | 0.59x | 0.65x | N/A | N/A |
| allocator burst retention/large_8192 | 2268.267 | N/A | N/A | 9875.754 | 467860.689 | 19368.237 | 26336.016 | 0.23x | 0.00x | 0.12x | 0.09x | N/A | N/A |
| allocator burst retention/medium_1024 | 1150.198 | N/A | N/A | 7865.691 | 100796.250 | 7838.347 | 8737.742 | 0.15x | 0.01x | 0.15x | 0.13x | N/A | N/A |
| allocator burst retention/small_32 | 795.266 | N/A | N/A | 6779.792 | 1018.721 | 4215.152 | 2615.959 | 0.12x | 0.78x | 0.19x | 0.30x | N/A | N/A |
| allocator cycle latency/huge_2m | 20.237 | 21.865 | 20.793 | 8492.179 | 8744.859 | N/A | 115.016 | 0.00x | 0.00x | N/A | 0.18x | 1.08x | 1.03x |
| allocator cycle latency/large_8192 | 3.038 | 3.162 | 3.230 | 21.737 | 22.523 | 17.266 | 15.418 | 0.14x | 0.13x | 0.18x | 0.20x | 1.04x | 1.06x |
| allocator cycle latency/medium_1024 | 2.955 | 3.205 | 3.039 | 20.579 | 6.336 | 16.717 | 7.242 | 0.14x | 0.47x | 0.18x | 0.41x | 1.08x | 1.03x |
| allocator cycle latency/small_32 | 2.860 | 2.766 | 2.905 | 21.530 | 2.843 | 15.777 | 6.815 | 0.13x | 1.01x | 0.18x | 0.42x | 0.97x | 1.02x |
| allocator deallocation latency/huge_2m | 850.307 | N/A | N/A | 5035.047 | 5174.923 | N/A | 3030.860 | 0.17x | 0.16x | N/A | 0.28x | N/A | N/A |
| allocator deallocation latency/large_8192 | 46.754 | N/A | N/A | 68.905 | 483.345 | 139.702 | 46.348 | 0.68x | 0.10x | 0.33x | 1.01x | N/A | N/A |
| allocator deallocation latency/medium_1024 | 9.949 | N/A | N/A | 23.850 | 86.311 | 50.121 | 16.857 | 0.42x | 0.12x | 0.20x | 0.59x | N/A | N/A |
| allocator deallocation latency/small_32 | 3.315 | N/A | N/A | 11.383 | 5.471 | 10.013 | 6.596 | 0.29x | 0.61x | 0.33x | 0.50x | N/A | N/A |
| cross-thread free handoff/huge_2m | 935.221 | N/A | N/A | 96679.175 | 106126.288 | N/A | 7228.476 | 0.01x | 0.01x | N/A | 0.13x | N/A | N/A |
| cross-thread free handoff/large_8192 | 23954.183 | N/A | N/A | 57615.385 | 1074570.142 | 100953.203 | 77239.485 | 0.42x | 0.02x | 0.24x | 0.31x | N/A | N/A |
| cross-thread free handoff/medium_1024 | 12163.469 | N/A | N/A | 31237.970 | 175127.392 | 35340.456 | 39076.890 | 0.39x | 0.07x | 0.34x | 0.31x | N/A | N/A |
| cross-thread free handoff/small_32 | 4933.826 | N/A | N/A | 31055.383 | 9722.656 | 20888.502 | 27554.410 | 0.16x | 0.51x | 0.24x | 0.18x | N/A | N/A |
| realloc latency/cross_class_32_to_64 | 9.919 | N/A | N/A | 46.058 | 10.156 | 32.444 | 16.928 | 0.22x | 0.98x | 0.31x | 0.59x | N/A | N/A |
| realloc latency/cross_class_8k_to_16k | 71.737 | N/A | N/A | 147.238 | 97.512 | 146.708 | 57.544 | 0.49x | 0.74x | 0.49x | 1.25x | N/A | N/A |
| realloc latency/huge_shrink_4m_to_2m | 20.842 | N/A | N/A | 918917.658 | 10893.132 | 995641.684 | 248.343 | 0.00x | 0.00x | 0.00x | 0.08x | N/A | N/A |
| realloc latency/within_class_24_to_32 | 4.895 | N/A | N/A | 51.860 | 5.197 | 17.330 | 15.253 | 0.09x | 0.94x | 0.28x | 0.32x | N/A | N/A |
| realloc latency/within_class_6k_to_8k | 36.962 | N/A | N/A | 119.774 | 65.471 | 110.352 | 52.248 | 0.31x | 0.56x | 0.33x | 0.71x | N/A | N/A |
| segment cache eviction | 266345.673 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 6208.649 | N/A | N/A | 32075.955 | 17339.123 | 26353.776 | 18411.964 | 0.19x | 0.36x | 0.24x | 0.34x | N/A | N/A |
| threaded saturated small allocation cycles | 80787.493 | N/A | N/A | 401927.442 | 81601.640 | 272100.769 | 128288.271 | 0.20x | 0.99x | 0.30x | 0.63x | N/A | N/A |
| threaded small allocation cycles | 6349.655 | N/A | N/A | 32962.515 | 6253.926 | 27515.034 | 17486.846 | 0.19x | 1.02x | 0.23x | 0.36x | N/A | N/A |
| usable size latency/huge_2m | 20.713 | N/A | N/A | N/A | 8896.113 | N/A | 116.998 | N/A | 0.00x | N/A | 0.18x | N/A | N/A |
| usable size latency/large_8192 | 3.099 | N/A | N/A | N/A | 14.983 | 17.593 | 17.806 | N/A | 0.21x | 0.18x | 0.17x | N/A | N/A |
| usable size latency/medium_1024 | 4.355 | N/A | N/A | N/A | 6.259 | 16.825 | 10.254 | N/A | 0.70x | 0.26x | 0.42x | N/A | N/A |
| usable size latency/small_32 | 4.423 | N/A | N/A | N/A | 3.274 | 16.563 | 9.912 | N/A | 1.35x | 0.27x | 0.45x | N/A | N/A |
| usable size query latency/huge_2m | 0.367 | N/A | N/A | N/A | 0.545 | N/A | 3.199 | N/A | 0.67x | N/A | 0.11x | N/A | N/A |
| usable size query latency/large_8192 | 0.344 | N/A | N/A | N/A | 0.563 | 0.485 | 3.198 | N/A | 0.61x | 0.71x | 0.11x | N/A | N/A |
| usable size query latency/medium_1024 | 0.381 | N/A | N/A | N/A | 0.657 | 0.474 | 3.200 | N/A | 0.58x | 0.80x | 0.12x | N/A | N/A |
| usable size query latency/small_32 | 0.312 | N/A | N/A | N/A | 0.910 | 0.543 | 3.212 | N/A | 0.34x | 0.57x | 0.10x | N/A | N/A |
