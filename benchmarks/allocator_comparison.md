# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MnemosyneHeap (ns) | BrandedHeap (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc | MnemosyneHeap vs Mnemosyne | BrandedHeap vs Mnemosyne |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 2386.756 | N/A | N/A | 2565.308 | 3977.942 | N/A | 1732.458 | 0.93x | 0.60x | N/A | 1.38x | N/A | N/A |
| allocator allocation latency/large_8192 | 60.654 | N/A | N/A | 715.667 | 1103.767 | 682.644 | 75.731 | 0.08x | 0.05x | 0.09x | 0.80x | N/A | N/A |
| allocator allocation latency/medium_1024 | 29.910 | N/A | N/A | 148.595 | 250.942 | 102.239 | 29.162 | 0.20x | 0.12x | 0.29x | 1.03x | N/A | N/A |
| allocator allocation latency/small_32 | 10.082 | N/A | N/A | 30.615 | 14.940 | 14.702 | 12.898 | 0.33x | 0.67x | 0.69x | 0.78x | N/A | N/A |
| allocator burst retention/large_8192 | 2955.605 | N/A | N/A | 9189.166 | 392814.844 | 20854.492 | 26336.016 | 0.32x | 0.01x | 0.14x | 0.11x | N/A | N/A |
| allocator burst retention/medium_1024 | 1172.875 | N/A | N/A | 6717.981 | 76500.879 | 7755.139 | 8737.742 | 0.17x | 0.02x | 0.15x | 0.13x | N/A | N/A |
| allocator burst retention/small_32 | 666.657 | N/A | N/A | 6298.004 | 871.779 | 4203.973 | 2615.959 | 0.11x | 0.76x | 0.16x | 0.25x | N/A | N/A |
| allocator cycle latency/huge_2m | 22.182 | 22.209 | 22.413 | 7585.736 | 8478.198 | N/A | 115.016 | 0.00x | 0.00x | N/A | 0.19x | 1.00x | 1.01x |
| allocator cycle latency/large_8192 | 3.761 | 2.913 | 3.257 | 20.200 | 16.783 | 17.269 | 15.418 | 0.19x | 0.22x | 0.22x | 0.24x | 0.77x | 0.87x |
| allocator cycle latency/medium_1024 | 3.776 | 2.904 | 3.242 | 20.424 | 5.613 | 16.608 | 7.242 | 0.18x | 0.67x | 0.23x | 0.52x | 0.77x | 0.86x |
| allocator cycle latency/small_32 | 2.965 | 2.906 | 3.244 | 19.995 | 2.729 | 15.089 | 6.815 | 0.15x | 1.09x | 0.20x | 0.44x | 0.98x | 1.09x |
| allocator deallocation latency/huge_2m | 995.904 | N/A | N/A | 5041.871 | 5309.503 | N/A | 3030.860 | 0.20x | 0.19x | N/A | 0.33x | N/A | N/A |
| allocator deallocation latency/large_8192 | 70.923 | N/A | N/A | 99.628 | 496.492 | 146.691 | 46.348 | 0.71x | 0.14x | 0.48x | 1.53x | N/A | N/A |
| allocator deallocation latency/medium_1024 | 11.096 | N/A | N/A | 84.694 | 77.792 | 57.541 | 16.857 | 0.13x | 0.14x | 0.19x | 0.66x | N/A | N/A |
| allocator deallocation latency/small_32 | 3.236 | N/A | N/A | 15.002 | 5.308 | 9.832 | 6.596 | 0.22x | 0.61x | 0.33x | 0.49x | N/A | N/A |
| cross-thread free handoff/huge_2m | 1514.396 | N/A | N/A | 77368.994 | 94163.818 | N/A | 7228.476 | 0.02x | 0.02x | N/A | 0.21x | N/A | N/A |
| cross-thread free handoff/large_8192 | 31244.531 | N/A | N/A | 51703.174 | 840294.141 | 92174.902 | 77239.485 | 0.60x | 0.04x | 0.34x | 0.40x | N/A | N/A |
| cross-thread free handoff/medium_1024 | 17845.941 | N/A | N/A | 31094.519 | 150083.594 | 36270.129 | 39076.890 | 0.57x | 0.12x | 0.49x | 0.46x | N/A | N/A |
| cross-thread free handoff/small_32 | 15459.070 | N/A | N/A | 28769.556 | 16485.254 | 19237.708 | 27554.410 | 0.54x | 0.94x | 0.80x | 0.56x | N/A | N/A |
| realloc latency/cross_class_32_to_64 | 8.002 | N/A | N/A | 42.676 | 10.793 | 32.927 | 16.928 | 0.19x | 0.74x | 0.24x | 0.47x | N/A | N/A |
| realloc latency/cross_class_8k_to_16k | 48.761 | N/A | N/A | 130.269 | 66.998 | 131.006 | 57.544 | 0.37x | 0.73x | 0.37x | 0.85x | N/A | N/A |
| realloc latency/huge_shrink_4m_to_2m | 22.171 | N/A | N/A | 936170.312 | 9011.768 | 1018235.156 | 248.343 | 0.00x | 0.00x | 0.00x | 0.09x | N/A | N/A |
| realloc latency/within_class_24_to_32 | 3.120 | N/A | N/A | 43.058 | 5.161 | 17.058 | 15.253 | 0.07x | 0.60x | 0.18x | 0.20x | N/A | N/A |
| realloc latency/within_class_6k_to_8k | 27.836 | N/A | N/A | 102.257 | 55.687 | 94.132 | 52.248 | 0.27x | 0.50x | 0.30x | 0.53x | N/A | N/A |
| segment cache eviction | 209658.984 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 13950.500 | N/A | N/A | 29519.531 | 15710.291 | 25660.205 | 18411.964 | 0.47x | 0.89x | 0.54x | 0.76x | N/A | N/A |
| threaded saturated small allocation cycles | 80181.513 | N/A | N/A | 343480.078 | 78583.969 | 262777.344 | 128288.271 | 0.23x | 1.02x | 0.31x | 0.63x | N/A | N/A |
| threaded small allocation cycles | 6052.256 | N/A | N/A | 31665.747 | 6577.694 | 25395.715 | 17486.846 | 0.19x | 0.92x | 0.24x | 0.35x | N/A | N/A |
| usable size latency/huge_2m | 22.199 | N/A | N/A | N/A | 9071.136 | N/A | 116.998 | N/A | 0.00x | N/A | 0.19x | N/A | N/A |
| usable size latency/large_8192 | 4.002 | N/A | N/A | N/A | 16.409 | 17.502 | 17.806 | N/A | 0.24x | 0.23x | 0.22x | N/A | N/A |
| usable size latency/medium_1024 | 4.487 | N/A | N/A | N/A | 5.946 | 16.929 | 10.254 | N/A | 0.75x | 0.27x | 0.44x | N/A | N/A |
| usable size latency/small_32 | 3.094 | N/A | N/A | N/A | 3.651 | 16.340 | 9.912 | N/A | 0.85x | 0.19x | 0.31x | N/A | N/A |
| usable size query latency/huge_2m | 0.342 | N/A | N/A | N/A | 0.531 | N/A | 3.199 | N/A | 0.65x | N/A | 0.11x | N/A | N/A |
| usable size query latency/large_8192 | 0.266 | N/A | N/A | N/A | 0.529 | 0.453 | 3.198 | N/A | 0.50x | 0.59x | 0.08x | N/A | N/A |
| usable size query latency/medium_1024 | 0.262 | N/A | N/A | N/A | 0.546 | 0.450 | 3.200 | N/A | 0.48x | 0.58x | 0.08x | N/A | N/A |
| usable size query latency/small_32 | 0.265 | N/A | N/A | N/A | 0.529 | 0.449 | 3.212 | N/A | 0.50x | 0.59x | 0.08x | N/A | N/A |
