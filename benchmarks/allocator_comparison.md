# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 671.204 | 2796.535 | 5530.039 | N/A | 2955.448 | 0.24x | 0.12x | N/A | 0.23x |
| allocator allocation latency/large_8192 | 24.635 | 229.001 | 1378.361 | 391.128 | 83.813 | 0.11x | 0.02x | 0.06x | 0.29x |
| allocator allocation latency/medium_1024 | 10.769 | 71.742 | 270.804 | 66.403 | 30.316 | 0.15x | 0.04x | 0.16x | 0.36x |
| allocator allocation latency/small_32 | 9.807 | 21.810 | 15.023 | 19.567 | 13.011 | 0.45x | 0.65x | 0.50x | 0.75x |
| allocator burst retention/large_8192 | 3449.841 | 8783.188 | 397116.374 | 20461.895 | 26736.792 | 0.39x | 0.01x | 0.17x | 0.13x |
| allocator burst retention/medium_1024 | 1349.444 | 6953.492 | 79592.866 | 7348.581 | 8837.971 | 0.19x | 0.02x | 0.18x | 0.15x |
| allocator burst retention/small_32 | 1243.896 | 6735.138 | 848.199 | 4163.596 | 2717.606 | 0.18x | 1.47x | 0.30x | 0.46x |
| allocator cycle latency/huge_2m | 22.157 | 9058.817 | 9142.196 | N/A | 116.614 | 0.00x | 0.00x | N/A | 0.19x |
| allocator cycle latency/large_8192 | 8.678 | 21.848 | 15.739 | 17.638 | 15.299 | 0.40x | 0.55x | 0.49x | 0.57x |
| allocator cycle latency/medium_1024 | 7.660 | 20.419 | 5.685 | 16.994 | 7.288 | 0.38x | 1.35x | 0.45x | 1.05x |
| allocator cycle latency/small_32 | 7.532 | 20.275 | 2.819 | 16.391 | 7.236 | 0.37x | 2.67x | 0.46x | 1.04x |
| allocator deallocation latency/huge_2m | 1328.163 | 4180.038 | 5127.762 | N/A | 3178.121 | 0.32x | 0.26x | N/A | 0.42x |
| allocator deallocation latency/large_8192 | 18.440 | 100.386 | 491.290 | 143.930 | 48.279 | 0.18x | 0.04x | 0.13x | 0.38x |
| allocator deallocation latency/medium_1024 | 10.099 | 20.264 | 81.480 | 49.033 | 19.907 | 0.50x | 0.12x | 0.21x | 0.51x |
| allocator deallocation latency/small_32 | 5.135 | 17.889 | 5.805 | 10.107 | 7.170 | 0.29x | 0.88x | 0.51x | 0.72x |
| cross-thread free handoff/huge_2m | 1190.347 | 98608.455 | 104844.945 | N/A | 6079.152 | 0.01x | 0.01x | N/A | 0.20x |
| cross-thread free handoff/large_8192 | 27403.922 | 57769.328 | 968991.812 | 96749.857 | 86637.986 | 0.47x | 0.03x | 0.28x | 0.32x |
| cross-thread free handoff/medium_1024 | 15448.403 | 32027.063 | 162303.677 | 37016.680 | 45283.824 | 0.48x | 0.10x | 0.42x | 0.34x |
| cross-thread free handoff/small_32 | 12557.784 | 30909.149 | 12857.776 | 21168.846 | 30011.413 | 0.41x | 0.98x | 0.59x | 0.42x |
| realloc latency/cross_class_32_to_64 | 15.168 | 44.275 | 9.123 | 32.906 | 17.283 | 0.34x | 1.66x | 0.46x | 0.88x |
| realloc latency/cross_class_8k_to_16k | 53.384 | 143.614 | 67.586 | 131.562 | 56.261 | 0.37x | 0.79x | 0.41x | 0.95x |
| realloc latency/huge_shrink_4m_to_2m | 22.746 | 1153555.257 | 8646.651 | 1081421.890 | 248.696 | 0.00x | 0.00x | 0.00x | 0.09x |
| realloc latency/within_class_24_to_32 | 5.489 | 42.859 | 4.368 | 17.334 | 15.569 | 0.13x | 1.26x | 0.32x | 0.35x |
| realloc latency/within_class_6k_to_8k | 27.196 | 106.407 | 56.134 | 97.725 | 51.619 | 0.26x | 0.48x | 0.28x | 0.53x |
| segment cache eviction | 234524.102 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 111678.523 | 394028.896 | 79492.495 | 303490.166 | 147850.861 | 0.28x | 1.40x | 0.37x | 0.76x |
| threaded small allocation cycles | 17117.135 | 34915.325 | 11204.780 | 27756.187 | 18738.037 | 0.49x | 1.53x | 0.62x | 0.91x |
| usable size latency/huge_2m | 23.859 | N/A | 7398.805 | N/A | 116.666 | N/A | 0.00x | N/A | 0.20x |
| usable size latency/large_8192 | 5.108 | N/A | 16.480 | 17.719 | 18.437 | N/A | 0.31x | 0.29x | 0.28x |
| usable size latency/medium_1024 | 6.234 | N/A | 6.265 | 16.804 | 10.286 | N/A | 1.00x | 0.37x | 0.61x |
| usable size latency/small_32 | 5.847 | N/A | 2.824 | 16.393 | 9.903 | N/A | 2.07x | 0.36x | 0.59x |
| usable size query latency/huge_2m | 0.399 | N/A | 0.526 | N/A | 3.247 | N/A | 0.76x | N/A | 0.12x |
| usable size query latency/large_8192 | 0.398 | N/A | 0.679 | 0.462 | 3.212 | N/A | 0.59x | 0.86x | 0.12x |
| usable size query latency/medium_1024 | 0.309 | N/A | 0.567 | 0.482 | 3.221 | N/A | 0.55x | 0.64x | 0.10x |
| usable size query latency/small_32 | 0.275 | N/A | 0.565 | 0.456 | 3.270 | N/A | 0.49x | 0.60x | 0.08x |
