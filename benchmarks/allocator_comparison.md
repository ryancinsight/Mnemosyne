# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MnemosyneHeap (ns) | BrandedHeap (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc | MnemosyneHeap vs Mnemosyne | BrandedHeap vs Mnemosyne |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 523.478 | N/A | N/A | 4252.651 | 5127.208 | N/A | 1732.458 | 0.12x | 0.10x | N/A | 0.30x | N/A | N/A |
| allocator allocation latency/large_8192 | 68.781 | N/A | N/A | 284.575 | 1384.427 | 407.080 | 75.731 | 0.24x | 0.05x | 0.17x | 0.91x | N/A | N/A |
| allocator allocation latency/medium_1024 | 14.180 | N/A | N/A | 55.027 | 339.721 | 101.268 | 29.162 | 0.26x | 0.04x | 0.14x | 0.49x | N/A | N/A |
| allocator allocation latency/small_32 | 7.857 | N/A | N/A | 22.172 | 16.090 | 12.773 | 12.898 | 0.35x | 0.49x | 0.62x | 0.61x | N/A | N/A |
| allocator burst retention/large_8192 | 2278.326 | N/A | N/A | 10797.141 | 498256.076 | 20755.323 | 26336.016 | 0.21x | 0.00x | 0.11x | 0.09x | N/A | N/A |
| allocator burst retention/medium_1024 | 1075.788 | N/A | N/A | 7811.932 | 93105.558 | 7909.205 | 8737.742 | 0.14x | 0.01x | 0.14x | 0.12x | N/A | N/A |
| allocator burst retention/small_32 | 729.734 | N/A | N/A | 6939.406 | 892.025 | 4183.832 | 2615.959 | 0.11x | 0.82x | 0.17x | 0.28x | N/A | N/A |
| allocator cycle latency/huge_2m | 22.077 | 22.100 | 21.880 | 9171.264 | 9544.597 | N/A | 115.016 | 0.00x | 0.00x | N/A | 0.19x | 1.00x | 0.99x |
| allocator cycle latency/large_8192 | 3.006 | 3.063 | 3.329 | 20.539 | 16.250 | 17.241 | 15.418 | 0.15x | 0.19x | 0.17x | 0.19x | 1.02x | 1.11x |
| allocator cycle latency/medium_1024 | 3.315 | 3.609 | 3.858 | 21.568 | 6.056 | 16.319 | 7.242 | 0.15x | 0.55x | 0.20x | 0.46x | 1.09x | 1.16x |
| allocator cycle latency/small_32 | 2.807 | 2.780 | 2.895 | 21.408 | 3.837 | 15.393 | 6.815 | 0.13x | 0.73x | 0.18x | 0.41x | 0.99x | 1.03x |
| allocator deallocation latency/huge_2m | 971.385 | N/A | N/A | 4679.447 | 5196.121 | N/A | 3030.860 | 0.21x | 0.19x | N/A | 0.32x | N/A | N/A |
| allocator deallocation latency/large_8192 | 32.739 | N/A | N/A | 63.194 | 588.544 | 169.438 | 46.348 | 0.52x | 0.06x | 0.19x | 0.71x | N/A | N/A |
| allocator deallocation latency/medium_1024 | 10.835 | N/A | N/A | 27.182 | 101.010 | 55.346 | 16.857 | 0.40x | 0.11x | 0.20x | 0.64x | N/A | N/A |
| allocator deallocation latency/small_32 | 3.514 | N/A | N/A | 13.979 | 6.259 | 9.787 | 6.596 | 0.25x | 0.56x | 0.36x | 0.53x | N/A | N/A |
| cross-thread free handoff/huge_2m | 824.243 | N/A | N/A | 110736.005 | 123286.819 | N/A | 7228.476 | 0.01x | 0.01x | N/A | 0.11x | N/A | N/A |
| cross-thread free handoff/large_8192 | 22541.719 | N/A | N/A | 57097.250 | 1010608.492 | 88430.725 | 77239.485 | 0.39x | 0.02x | 0.25x | 0.29x | N/A | N/A |
| cross-thread free handoff/medium_1024 | 11031.047 | N/A | N/A | 30084.544 | 173370.472 | 35686.810 | 39076.890 | 0.37x | 0.06x | 0.31x | 0.28x | N/A | N/A |
| cross-thread free handoff/small_32 | 5278.376 | N/A | N/A | 29318.375 | 9732.736 | 20037.575 | 27554.410 | 0.18x | 0.54x | 0.26x | 0.19x | N/A | N/A |
| realloc latency/cross_class_32_to_64 | 11.511 | N/A | N/A | 59.241 | 10.697 | 33.077 | 16.928 | 0.19x | 1.08x | 0.35x | 0.68x | N/A | N/A |
| realloc latency/cross_class_8k_to_16k | 70.797 | N/A | N/A | 136.854 | 93.202 | 137.674 | 57.544 | 0.52x | 0.76x | 0.51x | 1.23x | N/A | N/A |
| realloc latency/huge_shrink_4m_to_2m | 21.179 | N/A | N/A | 1000676.046 | 10321.503 | 1022992.799 | 248.343 | 0.00x | 0.00x | 0.00x | 0.09x | N/A | N/A |
| realloc latency/within_class_24_to_32 | 5.446 | N/A | N/A | 55.477 | 6.852 | 17.202 | 15.253 | 0.10x | 0.79x | 0.32x | 0.36x | N/A | N/A |
| realloc latency/within_class_6k_to_8k | 32.243 | N/A | N/A | 116.440 | 66.561 | 106.878 | 52.248 | 0.28x | 0.48x | 0.30x | 0.62x | N/A | N/A |
| segment cache eviction | 225310.139 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 6404.029 | N/A | N/A | 33926.404 | 16064.214 | 28369.238 | 18411.964 | 0.19x | 0.40x | 0.23x | 0.35x | N/A | N/A |
| threaded saturated small allocation cycles | 81920.786 | N/A | N/A | 411919.680 | 79981.763 | 279050.116 | 128288.271 | 0.20x | 1.02x | 0.29x | 0.64x | N/A | N/A |
| threaded small allocation cycles | 6814.730 | N/A | N/A | 33181.661 | 6064.588 | 29582.454 | 17486.846 | 0.21x | 1.12x | 0.23x | 0.39x | N/A | N/A |
| usable size latency/huge_2m | 20.846 | N/A | N/A | N/A | 8687.124 | N/A | 116.998 | N/A | 0.00x | N/A | 0.18x | N/A | N/A |
| usable size latency/large_8192 | 3.204 | N/A | N/A | N/A | 15.775 | 17.214 | 17.806 | N/A | 0.20x | 0.19x | 0.18x | N/A | N/A |
| usable size latency/medium_1024 | 4.416 | N/A | N/A | N/A | 6.474 | 16.971 | 10.254 | N/A | 0.68x | 0.26x | 0.43x | N/A | N/A |
| usable size latency/small_32 | 4.569 | N/A | N/A | N/A | 3.431 | 16.386 | 9.912 | N/A | 1.33x | 0.28x | 0.46x | N/A | N/A |
| usable size query latency/huge_2m | 0.432 | N/A | N/A | N/A | 0.741 | N/A | 3.199 | N/A | 0.58x | N/A | 0.13x | N/A | N/A |
| usable size query latency/large_8192 | 0.379 | N/A | N/A | N/A | 0.688 | 0.602 | 3.198 | N/A | 0.55x | 0.63x | 0.12x | N/A | N/A |
| usable size query latency/medium_1024 | 0.336 | N/A | N/A | N/A | 0.725 | 0.583 | 3.200 | N/A | 0.46x | 0.58x | 0.10x | N/A | N/A |
| usable size query latency/small_32 | 0.396 | N/A | N/A | N/A | 0.642 | 0.680 | 3.212 | N/A | 0.62x | 0.58x | 0.12x | N/A | N/A |
