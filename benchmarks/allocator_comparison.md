# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MnemosyneHeap (ns) | BrandedHeap (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc | MnemosyneHeap vs Mnemosyne | BrandedHeap vs Mnemosyne |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 2560.393 | N/A | N/A | 3347.360 | 6782.452 | 3209.050 | 1732.458 | 0.76x | 0.38x | 0.80x | 1.48x | N/A | N/A |
| allocator allocation latency/large_8192 | 71.014 | N/A | N/A | 285.774 | 1493.926 | 476.766 | 75.731 | 0.25x | 0.05x | 0.15x | 0.94x | N/A | N/A |
| allocator allocation latency/medium_1024 | 13.417 | N/A | N/A | 46.038 | 313.973 | 55.296 | 29.162 | 0.29x | 0.04x | 0.24x | 0.46x | N/A | N/A |
| allocator allocation latency/small_32 | 8.205 | N/A | N/A | 20.873 | 15.854 | 12.952 | 12.898 | 0.39x | 0.52x | 0.63x | 0.64x | N/A | N/A |
| allocator burst retention/large_8192 | 2112.005 | N/A | N/A | 10862.128 | 514667.053 | 20901.414 | 26336.016 | 0.19x | 0.00x | 0.10x | 0.08x | N/A | N/A |
| allocator burst retention/medium_1024 | 1218.189 | N/A | N/A | 8307.256 | 94060.570 | 7783.800 | 8737.742 | 0.15x | 0.01x | 0.16x | 0.14x | N/A | N/A |
| allocator burst retention/small_32 | 885.537 | N/A | N/A | 6929.297 | 1339.608 | 4181.211 | 2615.959 | 0.13x | 0.66x | 0.21x | 0.34x | N/A | N/A |
| allocator cycle latency/huge_2m | 22.343 | 22.328 | 22.227 | 8080.200 | 7372.516 | 5442.081 | 115.016 | 0.00x | 0.00x | 0.00x | 0.19x | 1.00x | 0.99x |
| allocator cycle latency/large_8192 | 3.169 | 3.122 | 3.692 | 22.740 | 15.524 | 16.839 | 15.418 | 0.14x | 0.20x | 0.19x | 0.21x | 0.99x | 1.17x |
| allocator cycle latency/medium_1024 | 2.918 | 3.298 | 3.323 | 21.347 | 6.373 | 16.673 | 7.242 | 0.14x | 0.46x | 0.17x | 0.40x | 1.13x | 1.14x |
| allocator cycle latency/small_32 | 2.813 | 2.784 | 2.889 | 21.518 | 3.465 | 15.740 | 6.815 | 0.13x | 0.81x | 0.18x | 0.41x | 0.99x | 1.03x |
| allocator deallocation latency/huge_2m | 5985.007 | N/A | N/A | 6335.898 | 6656.827 | 5374.644 | 3030.860 | 0.94x | 0.90x | 1.11x | 1.97x | N/A | N/A |
| allocator deallocation latency/large_8192 | 36.272 | N/A | N/A | 86.903 | 528.635 | 153.423 | 46.348 | 0.42x | 0.07x | 0.24x | 0.78x | N/A | N/A |
| allocator deallocation latency/medium_1024 | 11.901 | N/A | N/A | 25.209 | 98.828 | 50.018 | 16.857 | 0.47x | 0.12x | 0.24x | 0.71x | N/A | N/A |
| allocator deallocation latency/small_32 | 3.234 | N/A | N/A | 14.256 | 5.834 | 9.453 | 6.596 | 0.23x | 0.55x | 0.34x | 0.49x | N/A | N/A |
| cross-thread free handoff/huge_2m | 932.871 | N/A | N/A | 97890.527 | 120134.570 | 114095.410 | 7228.476 | 0.01x | 0.01x | 0.01x | 0.13x | N/A | N/A |
| cross-thread free handoff/large_8192 | 24058.661 | N/A | N/A | 52835.735 | 1089332.211 | 118363.566 | 77239.485 | 0.46x | 0.02x | 0.20x | 0.31x | N/A | N/A |
| cross-thread free handoff/medium_1024 | 9195.291 | N/A | N/A | 30981.341 | 179143.205 | 35112.517 | 39076.890 | 0.30x | 0.05x | 0.26x | 0.24x | N/A | N/A |
| cross-thread free handoff/small_32 | 4951.319 | N/A | N/A | 29946.753 | 10155.165 | 21451.587 | 27554.410 | 0.17x | 0.49x | 0.23x | 0.18x | N/A | N/A |
| realloc latency/cross_class_32_to_64 | 8.759 | N/A | N/A | 51.720 | 10.246 | 32.272 | 16.928 | 0.17x | 0.85x | 0.27x | 0.52x | N/A | N/A |
| realloc latency/cross_class_8k_to_16k | 62.609 | N/A | N/A | 138.131 | 106.884 | 136.426 | 57.544 | 0.45x | 0.59x | 0.46x | 1.09x | N/A | N/A |
| realloc latency/huge_shrink_4m_to_2m | 20.479 | N/A | N/A | 1055406.187 | 9310.892 | 1125932.938 | 248.343 | 0.00x | 0.00x | 0.00x | 0.08x | N/A | N/A |
| realloc latency/within_class_24_to_32 | 5.010 | N/A | N/A | 48.812 | 6.027 | 16.145 | 15.253 | 0.10x | 0.83x | 0.31x | 0.33x | N/A | N/A |
| realloc latency/within_class_6k_to_8k | 35.009 | N/A | N/A | 106.077 | 70.469 | 97.740 | 52.248 | 0.33x | 0.50x | 0.36x | 0.67x | N/A | N/A |
| segment cache eviction | 329428.454 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 6515.030 | N/A | N/A | 37406.998 | 19955.358 | 54278.069 | 18411.964 | 0.17x | 0.33x | 0.12x | 0.35x | N/A | N/A |
| threaded saturated small allocation cycles | 103964.759 | N/A | N/A | 417833.139 | 84557.011 | 277327.581 | 128288.271 | 0.25x | 1.23x | 0.37x | 0.81x | N/A | N/A |
| threaded small allocation cycles | 7086.189 | N/A | N/A | 31624.592 | 5866.088 | 25943.093 | 17486.846 | 0.22x | 1.21x | 0.27x | 0.41x | N/A | N/A |
| usable size latency/huge_2m | 21.197 | N/A | N/A | N/A | 9733.969 | 5006.668 | 116.998 | N/A | 0.00x | 0.00x | 0.18x | N/A | N/A |
| usable size latency/large_8192 | 3.243 | N/A | N/A | N/A | 15.371 | 17.103 | 17.806 | N/A | 0.21x | 0.19x | 0.18x | N/A | N/A |
| usable size latency/medium_1024 | 5.324 | N/A | N/A | N/A | 6.571 | 16.769 | 10.254 | N/A | 0.81x | 0.32x | 0.52x | N/A | N/A |
| usable size latency/small_32 | 4.808 | N/A | N/A | N/A | 3.502 | 15.879 | 9.912 | N/A | 1.37x | 0.30x | 0.49x | N/A | N/A |
| usable size query latency/huge_2m | 0.356 | N/A | N/A | N/A | 0.733 | 0.576 | 3.199 | N/A | 0.49x | 0.62x | 0.11x | N/A | N/A |
| usable size query latency/large_8192 | 0.408 | N/A | N/A | N/A | 0.666 | 0.612 | 3.198 | N/A | 0.61x | 0.67x | 0.13x | N/A | N/A |
| usable size query latency/medium_1024 | 0.349 | N/A | N/A | N/A | 0.824 | 0.499 | 3.200 | N/A | 0.42x | 0.70x | 0.11x | N/A | N/A |
| usable size query latency/small_32 | 0.418 | N/A | N/A | N/A | 0.906 | 0.567 | 3.212 | N/A | 0.46x | 0.74x | 0.13x | N/A | N/A |
