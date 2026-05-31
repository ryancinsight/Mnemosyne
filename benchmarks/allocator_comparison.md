# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 733.932 | 2802.269 | 4164.965 | N/A | 1732.458 | 0.26x | 0.18x | N/A | 0.42x |
| allocator allocation latency/large_8192 | 21.546 | 248.958 | 1186.102 | 378.819 | 75.731 | 0.09x | 0.02x | 0.06x | 0.28x |
| allocator allocation latency/medium_1024 | 11.153 | 69.388 | 237.607 | 69.744 | 29.162 | 0.16x | 0.05x | 0.16x | 0.38x |
| allocator allocation latency/small_32 | 9.641 | 21.447 | 14.672 | 13.799 | 12.898 | 0.45x | 0.66x | 0.70x | 0.75x |
| allocator burst retention/large_8192 | 2873.339 | 9008.247 | 430028.745 | 19117.001 | 26336.016 | 0.32x | 0.01x | 0.15x | 0.11x |
| allocator burst retention/medium_1024 | 1042.276 | 6624.733 | 85887.095 | 8114.229 | 8737.742 | 0.16x | 0.01x | 0.13x | 0.12x |
| allocator burst retention/small_32 | 587.422 | 6687.105 | 795.228 | 4257.355 | 2615.959 | 0.09x | 0.74x | 0.14x | 0.22x |
| allocator cycle latency/huge_2m | 21.517 | 9424.468 | 10119.182 | N/A | 115.016 | 0.00x | 0.00x | N/A | 0.19x |
| allocator cycle latency/large_8192 | 2.562 | 19.938 | 16.311 | 19.378 | 15.418 | 0.13x | 0.16x | 0.13x | 0.17x |
| allocator cycle latency/medium_1024 | 2.344 | 20.944 | 5.975 | 17.465 | 7.242 | 0.11x | 0.39x | 0.13x | 0.32x |
| allocator cycle latency/small_32 | 2.346 | 21.551 | 2.743 | 17.028 | 6.815 | 0.11x | 0.86x | 0.14x | 0.34x |
| allocator deallocation latency/huge_2m | 1162.704 | 4138.440 | 4435.339 | N/A | 3030.860 | 0.28x | 0.26x | N/A | 0.38x |
| allocator deallocation latency/large_8192 | 16.048 | 147.991 | 490.699 | 149.951 | 46.348 | 0.11x | 0.03x | 0.11x | 0.35x |
| allocator deallocation latency/medium_1024 | 8.733 | 18.782 | 77.985 | 44.349 | 16.857 | 0.46x | 0.11x | 0.20x | 0.52x |
| allocator deallocation latency/small_32 | 2.972 | 10.038 | 4.902 | 10.045 | 6.596 | 0.30x | 0.61x | 0.30x | 0.45x |
| cross-thread free handoff/huge_2m | 1221.059 | 87020.501 | 94428.908 | N/A | 7228.476 | 0.01x | 0.01x | N/A | 0.17x |
| cross-thread free handoff/large_8192 | 28363.575 | 56114.413 | 994238.678 | 96723.477 | 77239.485 | 0.51x | 0.03x | 0.29x | 0.37x |
| cross-thread free handoff/medium_1024 | 14423.675 | 31867.993 | 173398.209 | 37594.546 | 39076.890 | 0.45x | 0.08x | 0.38x | 0.37x |
| cross-thread free handoff/small_32 | 9964.332 | 30898.187 | 13858.555 | 21506.789 | 27554.410 | 0.32x | 0.72x | 0.46x | 0.36x |
| realloc latency/cross_class_32_to_64 | 6.821 | 48.654 | 9.171 | 33.681 | 16.928 | 0.14x | 0.74x | 0.20x | 0.40x |
| realloc latency/cross_class_8k_to_16k | 48.130 | 148.995 | 67.447 | 139.712 | 57.544 | 0.32x | 0.71x | 0.34x | 0.84x |
| realloc latency/huge_shrink_4m_to_2m | 21.798 | 1179967.360 | 7201.033 | 1115674.261 | 248.343 | 0.00x | 0.00x | 0.00x | 0.09x |
| realloc latency/within_class_24_to_32 | 3.250 | 48.118 | 4.563 | 17.770 | 15.253 | 0.07x | 0.71x | 0.18x | 0.21x |
| realloc latency/within_class_6k_to_8k | 28.537 | 108.112 | 68.950 | 110.463 | 52.248 | 0.26x | 0.41x | 0.26x | 0.55x |
| segment cache eviction | 225540.202 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 58767.302 | 400365.338 | 78548.862 | 277800.828 | 128288.271 | 0.15x | 0.75x | 0.21x | 0.46x |
| threaded small allocation cycles | 9315.645 | 34465.234 | 12720.131 | 27651.728 | 17486.846 | 0.27x | 0.73x | 0.34x | 0.53x |
| usable size latency/huge_2m | 21.994 | N/A | 8298.572 | N/A | 116.998 | N/A | 0.00x | N/A | 0.19x |
| usable size latency/large_8192 | 2.485 | N/A | 16.436 | 19.931 | 17.806 | N/A | 0.15x | 0.12x | 0.14x |
| usable size latency/medium_1024 | 3.398 | N/A | 6.190 | 17.762 | 10.254 | N/A | 0.55x | 0.19x | 0.33x |
| usable size latency/small_32 | 3.373 | N/A | 3.675 | 16.646 | 9.912 | N/A | 0.92x | 0.20x | 0.34x |
| usable size query latency/huge_2m | 0.397 | N/A | 0.579 | N/A | 3.199 | N/A | 0.68x | N/A | 0.12x |
| usable size query latency/large_8192 | 0.317 | N/A | 0.601 | 0.680 | 3.198 | N/A | 0.53x | 0.47x | 0.10x |
| usable size query latency/medium_1024 | 0.294 | N/A | 0.565 | 0.549 | 3.200 | N/A | 0.52x | 0.54x | 0.09x |
| usable size query latency/small_32 | 0.296 | N/A | 0.527 | 0.468 | 3.212 | N/A | 0.56x | 0.63x | 0.09x |
