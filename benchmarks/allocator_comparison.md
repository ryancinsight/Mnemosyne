# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MnemosyneHeap (ns) | BrandedHeap (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc | MnemosyneHeap vs Mnemosyne | BrandedHeap vs Mnemosyne |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 715.101 | N/A | N/A | 2787.810 | 4399.481 | N/A | 1732.458 | 0.26x | 0.16x | N/A | 0.41x | N/A | N/A |
| allocator allocation latency/large_8192 | 70.813 | N/A | N/A | 226.518 | 1267.346 | 395.632 | 75.731 | 0.31x | 0.06x | 0.18x | 0.94x | N/A | N/A |
| allocator allocation latency/medium_1024 | 14.370 | N/A | N/A | 58.763 | 269.951 | 89.746 | 29.162 | 0.24x | 0.05x | 0.16x | 0.49x | N/A | N/A |
| allocator allocation latency/small_32 | 11.164 | N/A | N/A | 21.869 | 17.913 | 15.180 | 12.898 | 0.51x | 0.62x | 0.74x | 0.87x | N/A | N/A |
| allocator burst retention/large_8192 | 2980.393 | N/A | N/A | 8872.552 | 394061.238 | 20404.701 | 26336.016 | 0.34x | 0.01x | 0.15x | 0.11x | N/A | N/A |
| allocator burst retention/medium_1024 | 1026.977 | N/A | N/A | 6973.715 | 78792.745 | 7777.536 | 8737.742 | 0.15x | 0.01x | 0.13x | 0.12x | N/A | N/A |
| allocator burst retention/small_32 | 619.569 | N/A | N/A | 6306.882 | 803.694 | 4189.314 | 2615.959 | 0.10x | 0.77x | 0.15x | 0.24x | N/A | N/A |
| allocator cycle latency/huge_2m | 22.139 | 22.214 | 22.297 | 8270.352 | 9021.250 | N/A | 115.016 | 0.00x | 0.00x | N/A | 0.19x | 1.00x | 1.01x |
| allocator cycle latency/large_8192 | 2.829 | 2.750 | 2.876 | 21.247 | 16.849 | 17.267 | 15.418 | 0.13x | 0.17x | 0.16x | 0.18x | 0.97x | 1.02x |
| allocator cycle latency/medium_1024 | 2.831 | 2.763 | 2.860 | 20.098 | 5.978 | 16.724 | 7.242 | 0.14x | 0.47x | 0.17x | 0.39x | 0.98x | 1.01x |
| allocator cycle latency/small_32 | 2.820 | 2.743 | 2.879 | 19.916 | 2.780 | 16.267 | 6.815 | 0.14x | 1.01x | 0.17x | 0.41x | 0.97x | 1.02x |
| allocator deallocation latency/huge_2m | 1200.755 | N/A | N/A | 4229.516 | 4577.309 | N/A | 3030.860 | 0.28x | 0.26x | N/A | 0.40x | N/A | N/A |
| allocator deallocation latency/large_8192 | 30.664 | N/A | N/A | 76.644 | 498.081 | 158.704 | 46.348 | 0.40x | 0.06x | 0.19x | 0.66x | N/A | N/A |
| allocator deallocation latency/medium_1024 | 10.045 | N/A | N/A | 21.784 | 87.693 | 51.991 | 16.857 | 0.46x | 0.11x | 0.19x | 0.60x | N/A | N/A |
| allocator deallocation latency/small_32 | 3.349 | N/A | N/A | 11.892 | 5.523 | 9.704 | 6.596 | 0.28x | 0.61x | 0.35x | 0.51x | N/A | N/A |
| cross-thread free handoff/huge_2m | 1536.845 | N/A | N/A | 85179.822 | 94809.194 | N/A | 7228.476 | 0.02x | 0.02x | N/A | 0.21x | N/A | N/A |
| cross-thread free handoff/large_8192 | 22873.359 | N/A | N/A | 52467.826 | 839613.537 | 92735.166 | 77239.485 | 0.44x | 0.03x | 0.25x | 0.30x | N/A | N/A |
| cross-thread free handoff/medium_1024 | 17552.130 | N/A | N/A | 31995.644 | 142546.591 | 35670.937 | 39076.890 | 0.55x | 0.12x | 0.49x | 0.45x | N/A | N/A |
| cross-thread free handoff/small_32 | 14888.852 | N/A | N/A | 29027.019 | 16267.429 | 18546.481 | 27554.410 | 0.51x | 0.92x | 0.80x | 0.54x | N/A | N/A |
| realloc latency/cross_class_32_to_64 | 7.941 | N/A | N/A | 43.385 | 7.677 | 33.071 | 16.928 | 0.18x | 1.03x | 0.24x | 0.47x | N/A | N/A |
| realloc latency/cross_class_8k_to_16k | 47.651 | N/A | N/A | 136.390 | 67.113 | 130.970 | 57.544 | 0.35x | 0.71x | 0.36x | 0.83x | N/A | N/A |
| realloc latency/huge_shrink_4m_to_2m | 22.716 | N/A | N/A | 992582.437 | 7238.210 | 1030577.725 | 248.343 | 0.00x | 0.00x | 0.00x | 0.09x | N/A | N/A |
| realloc latency/within_class_24_to_32 | 4.373 | N/A | N/A | 43.593 | 4.389 | 17.211 | 15.253 | 0.10x | 1.00x | 0.25x | 0.29x | N/A | N/A |
| realloc latency/within_class_6k_to_8k | 26.733 | N/A | N/A | 101.730 | 55.857 | 94.775 | 52.248 | 0.26x | 0.48x | 0.28x | 0.51x | N/A | N/A |
| segment cache eviction | 226356.668 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 8728.473 | N/A | N/A | 30122.294 | 15420.784 | 35028.314 | 18411.964 | 0.29x | 0.57x | 0.25x | 0.47x | N/A | N/A |
| threaded saturated small allocation cycles | 64294.298 | N/A | N/A | 361851.590 | 76591.886 | 260740.204 | 128288.271 | 0.18x | 0.84x | 0.25x | 0.50x | N/A | N/A |
| threaded small allocation cycles | 10512.881 | N/A | N/A | 31196.264 | 6076.770 | 25259.486 | 17486.846 | 0.34x | 1.73x | 0.42x | 0.60x | N/A | N/A |
| usable size latency/huge_2m | 22.347 | N/A | N/A | N/A | 6614.424 | N/A | 116.998 | N/A | 0.00x | N/A | 0.19x | N/A | N/A |
| usable size latency/large_8192 | 2.899 | N/A | N/A | N/A | 16.381 | 17.725 | 17.806 | N/A | 0.18x | 0.16x | 0.16x | N/A | N/A |
| usable size latency/medium_1024 | 4.349 | N/A | N/A | N/A | 6.158 | 16.833 | 10.254 | N/A | 0.71x | 0.26x | 0.42x | N/A | N/A |
| usable size latency/small_32 | 4.308 | N/A | N/A | N/A | 2.913 | 16.537 | 9.912 | N/A | 1.48x | 0.26x | 0.43x | N/A | N/A |
| usable size query latency/huge_2m | 0.351 | N/A | N/A | N/A | 0.549 | N/A | 3.199 | N/A | 0.64x | N/A | 0.11x | N/A | N/A |
| usable size query latency/large_8192 | 0.294 | N/A | N/A | N/A | 0.531 | 0.462 | 3.198 | N/A | 0.55x | 0.64x | 0.09x | N/A | N/A |
| usable size query latency/medium_1024 | 0.297 | N/A | N/A | N/A | 0.529 | 0.458 | 3.200 | N/A | 0.56x | 0.65x | 0.09x | N/A | N/A |
| usable size query latency/small_32 | 0.290 | N/A | N/A | N/A | 0.522 | 0.456 | 3.212 | N/A | 0.56x | 0.64x | 0.09x | N/A | N/A |
