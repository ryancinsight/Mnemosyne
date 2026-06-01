# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 729.932 | 2651.283 | 4070.726 | N/A | 1732.458 | 0.28x | 0.18x | N/A | 0.42x |
| allocator allocation latency/large_8192 | 21.827 | 296.953 | 1204.755 | 423.655 | 75.731 | 0.07x | 0.02x | 0.05x | 0.29x |
| allocator allocation latency/medium_1024 | 11.427 | 62.549 | 270.483 | 68.399 | 29.162 | 0.18x | 0.04x | 0.17x | 0.39x |
| allocator allocation latency/small_32 | 9.849 | 20.892 | 15.102 | 14.318 | 12.898 | 0.47x | 0.65x | 0.69x | 0.76x |
| allocator burst retention/large_8192 | 2765.466 | 8524.747 | 396272.362 | 20183.858 | 26336.016 | 0.32x | 0.01x | 0.14x | 0.11x |
| allocator burst retention/medium_1024 | 1095.004 | 6886.878 | 78380.977 | 7840.815 | 8737.742 | 0.16x | 0.01x | 0.14x | 0.13x |
| allocator burst retention/small_32 | 622.756 | 6402.397 | 831.887 | 4300.185 | 2615.959 | 0.10x | 0.75x | 0.14x | 0.24x |
| allocator cycle latency/huge_2m | 22.085 | 7632.146 | 8505.168 | N/A | 115.016 | 0.00x | 0.00x | N/A | 0.19x |
| allocator cycle latency/large_8192 | 2.320 | 20.501 | 16.502 | 17.393 | 15.418 | 0.11x | 0.14x | 0.13x | 0.15x |
| allocator cycle latency/medium_1024 | 2.373 | 20.453 | 5.707 | 16.583 | 7.242 | 0.12x | 0.42x | 0.14x | 0.33x |
| allocator cycle latency/small_32 | 2.306 | 20.499 | 2.767 | 16.245 | 6.815 | 0.11x | 0.83x | 0.14x | 0.34x |
| allocator deallocation latency/huge_2m | 1301.133 | 4082.617 | 4502.410 | N/A | 3030.860 | 0.32x | 0.29x | N/A | 0.43x |
| allocator deallocation latency/large_8192 | 19.835 | 64.247 | 511.049 | 175.696 | 46.348 | 0.31x | 0.04x | 0.11x | 0.43x |
| allocator deallocation latency/medium_1024 | 8.472 | 22.337 | 113.286 | 56.564 | 16.857 | 0.38x | 0.07x | 0.15x | 0.50x |
| allocator deallocation latency/small_32 | 3.114 | 10.664 | 4.958 | 9.535 | 6.596 | 0.29x | 0.63x | 0.33x | 0.47x |
| cross-thread free handoff/huge_2m | 2959.524 | 73023.930 | 83058.088 | N/A | 7228.476 | 0.04x | 0.04x | N/A | 0.41x |
| cross-thread free handoff/large_8192 | 29474.278 | 56534.908 | 845470.728 | 93807.745 | 77239.485 | 0.52x | 0.03x | 0.31x | 0.38x |
| cross-thread free handoff/medium_1024 | 19309.804 | 33357.774 | 145321.986 | 38519.978 | 39076.890 | 0.58x | 0.13x | 0.50x | 0.49x |
| cross-thread free handoff/small_32 | 9086.959 | 30426.820 | 11199.730 | 21302.658 | 27554.410 | 0.30x | 0.81x | 0.43x | 0.33x |
| realloc latency/cross_class_32_to_64 | 6.678 | 44.014 | 7.449 | 32.900 | 16.928 | 0.15x | 0.90x | 0.20x | 0.39x |
| realloc latency/cross_class_8k_to_16k | 48.225 | 129.537 | 67.149 | 131.322 | 57.544 | 0.37x | 0.72x | 0.37x | 0.84x |
| realloc latency/huge_shrink_4m_to_2m | 22.483 | 953996.573 | 7223.729 | 1024000.653 | 248.343 | 0.00x | 0.00x | 0.00x | 0.09x |
| realloc latency/within_class_24_to_32 | 3.236 | 42.669 | 4.389 | 17.414 | 15.253 | 0.08x | 0.74x | 0.19x | 0.21x |
| realloc latency/within_class_6k_to_8k | 24.402 | 101.866 | 56.046 | 94.799 | 52.248 | 0.24x | 0.44x | 0.26x | 0.47x |
| segment cache eviction | 209668.554 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 53721.147 | 377404.211 | 62868.801 | 267770.397 | 128288.271 | 0.14x | 0.85x | 0.20x | 0.42x |
| threaded small allocation cycles | 11690.604 | 32201.059 | 13498.955 | 26839.314 | 17486.846 | 0.36x | 0.87x | 0.44x | 0.67x |
| usable size latency/huge_2m | 22.341 | N/A | 7926.451 | N/A | 116.998 | N/A | 0.00x | N/A | 0.19x |
| usable size latency/large_8192 | 2.473 | N/A | 16.477 | 17.474 | 17.806 | N/A | 0.15x | 0.14x | 0.14x |
| usable size latency/medium_1024 | 3.388 | N/A | 5.968 | 17.281 | 10.254 | N/A | 0.57x | 0.20x | 0.33x |
| usable size latency/small_32 | 2.492 | N/A | 2.889 | 16.401 | 9.912 | N/A | 0.86x | 0.15x | 0.25x |
| usable size query latency/huge_2m | 0.349 | N/A | 0.529 | N/A | 3.199 | N/A | 0.66x | N/A | 0.11x |
| usable size query latency/large_8192 | 0.286 | N/A | 0.533 | 0.456 | 3.198 | N/A | 0.54x | 0.63x | 0.09x |
| usable size query latency/medium_1024 | 0.302 | N/A | 0.532 | 0.455 | 3.200 | N/A | 0.57x | 0.67x | 0.09x |
| usable size query latency/small_32 | 0.286 | N/A | 0.535 | 0.455 | 3.212 | N/A | 0.53x | 0.63x | 0.09x |
