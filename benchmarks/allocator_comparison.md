# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 21.291 | 53.248 | 277.632 | 90.642 | N/A | 0.40x | 0.08x | 0.23x | N/A |
| allocator allocation latency/small_32 | 10.769 | 29.382 | 16.568 | 14.961 | N/A | 0.37x | 0.65x | 0.72x | N/A |
| allocator burst retention/large_8192 | 4049.197 | 12801.344 | 512118.592 | 22160.609 | N/A | 0.32x | 0.01x | 0.18x | N/A |
| allocator burst retention/medium_1024 | 1675.375 | 8006.828 | 103007.531 | 8032.315 | N/A | 0.21x | 0.02x | 0.21x | N/A |
| allocator burst retention/small_32 | 646.699 | 7426.376 | 1194.490 | 4373.438 | N/A | 0.09x | 0.54x | 0.15x | N/A |
| allocator cycle latency/large_8192 | 2.541 | 24.795 | 16.358 | 18.199 | N/A | 0.10x | 0.16x | 0.14x | N/A |
| allocator cycle latency/medium_1024 | 2.501 | 22.292 | 6.228 | 17.501 | N/A | 0.11x | 0.40x | 0.14x | N/A |
| allocator cycle latency/small_32 | 2.021 | 20.958 | 2.887 | 16.733 | N/A | 0.10x | 0.70x | 0.12x | N/A |
| allocator deallocation latency/medium_1024 | 28.825 | 42.781 | 102.739 | 66.327 | N/A | 0.67x | 0.28x | 0.43x | N/A |
| allocator deallocation latency/small_32 | 3.668 | 12.201 | 5.831 | 10.734 | N/A | 0.30x | 0.63x | 0.34x | N/A |
| cross-thread free handoff/medium_1024 | 18213.022 | 37059.338 | 184358.372 | 44623.642 | N/A | 0.49x | 0.10x | 0.41x | N/A |
| cross-thread free handoff/small_32 | 6285.503 | 37795.225 | 12224.602 | 25168.704 | N/A | 0.17x | 0.51x | 0.25x | N/A |
| realloc latency/cross_class_32_to_64 | 6.994 | 51.277 | 10.401 | 35.154 | N/A | 0.14x | 0.67x | 0.20x | N/A |
| realloc latency/within_class_24_to_32 | 2.633 | 50.047 | 5.921 | 17.697 | N/A | 0.05x | 0.44x | 0.15x | N/A |
| segment cache eviction | 185276.716 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 63106.640 | 469318.862 | 92385.119 | 301684.698 | N/A | 0.13x | 0.68x | 0.21x | N/A |
| threaded small allocation cycles | 4272.768 | 35520.120 | 6311.891 | 30022.707 | N/A | 0.12x | 0.68x | 0.14x | N/A |
| usable size latency/medium_1024 | 3.417 | N/A | 6.836 | 17.736 | N/A | N/A | 0.50x | 0.19x | N/A |
| usable size latency/small_32 | 2.773 | N/A | 3.893 | 16.670 | N/A | N/A | 0.71x | 0.17x | N/A |
| usable size query latency/medium_1024 | 0.375 | N/A | 0.758 | 0.567 | N/A | N/A | 0.49x | 0.66x | N/A |
| usable size query latency/small_32 | 0.407 | N/A | 0.655 | 0.551 | N/A | N/A | 0.62x | 0.74x | N/A |
