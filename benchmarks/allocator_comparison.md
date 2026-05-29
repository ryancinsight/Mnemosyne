# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 146.240 | 237.697 | 310.306 | 174.407 | N/A | 0.62x | 0.47x | 0.84x | N/A |
| allocator allocation latency/small_32 | 15.150 | 38.737 | 18.373 | 19.793 | N/A | 0.39x | 0.82x | 0.77x | N/A |
| allocator burst retention/large_8192 | 5570.172 | 12551.683 | 521922.852 | 21367.468 | N/A | 0.44x | 0.01x | 0.26x | N/A |
| allocator burst retention/medium_1024 | 2132.854 | 8319.873 | 93061.499 | 7277.240 | N/A | 0.26x | 0.02x | 0.29x | N/A |
| allocator burst retention/small_32 | 1341.059 | 6457.990 | 1516.093 | 3957.855 | N/A | 0.21x | 0.88x | 0.34x | N/A |
| allocator cycle latency/large_8192 | 3.923 | 20.806 | 16.776 | 17.358 | N/A | 0.19x | 0.23x | 0.23x | N/A |
| allocator cycle latency/medium_1024 | 3.726 | 20.983 | 5.762 | 16.706 | N/A | 0.18x | 0.65x | 0.22x | N/A |
| allocator cycle latency/small_32 | 3.609 | 20.318 | 3.189 | 15.165 | N/A | 0.18x | 1.13x | 0.24x | N/A |
| allocator deallocation latency/medium_1024 | 95.367 | 93.027 | 130.893 | 91.549 | N/A | 1.03x | 0.73x | 1.04x | N/A |
| allocator deallocation latency/small_32 | 5.880 | 21.218 | 7.156 | 10.097 | N/A | 0.28x | 0.82x | 0.58x | N/A |
| cross-thread free handoff/medium_1024 | 16980.484 | 39202.393 | 196977.148 | 41360.400 | N/A | 0.43x | 0.09x | 0.41x | N/A |
| cross-thread free handoff/small_32 | 7781.299 | 37015.747 | 8543.457 | 20859.399 | N/A | 0.21x | 0.91x | 0.37x | N/A |
| realloc latency/cross_class_32_to_64 | 13.025 | 56.737 | 10.297 | 32.545 | N/A | 0.23x | 1.26x | 0.40x | N/A |
| realloc latency/within_class_24_to_32 | 6.010 | 51.371 | 5.770 | 16.869 | N/A | 0.12x | 1.04x | 0.36x | N/A |
| segment cache eviction | 91788.245 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 107201.855 | 395451.172 | 79539.111 | 267374.414 | N/A | 0.27x | 1.35x | 0.40x | N/A |
| threaded small allocation cycles | 9035.428 | 37928.192 | 6077.338 | 26143.033 | N/A | 0.24x | 1.49x | 0.35x | N/A |
| usable size latency/medium_1024 | 5.113 | N/A | 6.615 | 16.790 | N/A | N/A | 0.77x | 0.30x | N/A |
| usable size latency/small_32 | 5.120 | N/A | 3.933 | 15.717 | N/A | N/A | 1.30x | 0.33x | N/A |
| usable size query latency/medium_1024 | 0.452 | N/A | 0.854 | 0.703 | N/A | N/A | 0.53x | 0.64x | N/A |
| usable size query latency/small_32 | 0.474 | N/A | 1.070 | 0.611 | N/A | N/A | 0.44x | 0.78x | N/A |
