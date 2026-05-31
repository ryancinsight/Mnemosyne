# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 2459.367 | 2648.407 | 4052.344 | N/A | 1800.339 | 0.93x | 0.61x | N/A | 1.37x |
| allocator allocation latency/large_8192 | 22.819 | 673.286 | 1138.339 | 896.865 | 112.831 | 0.03x | 0.02x | 0.03x | 0.20x |
| allocator allocation latency/medium_1024 | 11.040 | 156.732 | 246.719 | 171.556 | 32.793 | 0.07x | 0.04x | 0.06x | 0.34x |
| allocator allocation latency/small_32 | 9.139 | 30.333 | 14.196 | 46.994 | 12.946 | 0.30x | 0.64x | 0.19x | 0.71x |
| allocator burst retention/large_8192 | 2631.375 | 9332.458 | 385137.891 | 182359.277 | 26750.684 | 0.28x | 0.01x | 0.01x | 0.10x |
| allocator burst retention/medium_1024 | 904.373 | 6802.557 | 77654.785 | 45097.729 | 8808.118 | 0.13x | 0.01x | 0.02x | 0.10x |
| allocator burst retention/small_32 | 451.283 | 6302.997 | 827.267 | 13141.003 | 2618.198 | 0.07x | 0.55x | 0.03x | 0.17x |
| allocator cycle latency/huge_2m | 22.783 | 9581.323 | 9933.795 | N/A | 113.550 | 0.00x | 0.00x | N/A | 0.20x |
| allocator cycle latency/large_8192 | 8.531 | 20.201 | 16.719 | 73.200 | 15.161 | 0.42x | 0.51x | 0.12x | 0.56x |
| allocator cycle latency/medium_1024 | 7.633 | 20.237 | 5.631 | 56.393 | 7.309 | 0.38x | 1.36x | 0.14x | 1.04x |
| allocator cycle latency/small_32 | 7.495 | 20.145 | 2.869 | 48.581 | 6.934 | 0.37x | 2.61x | 0.15x | 1.08x |
| allocator deallocation latency/huge_2m | 5161.642 | 5943.231 | 6634.882 | N/A | 3060.953 | 0.87x | 0.78x | N/A | 1.69x |
| allocator deallocation latency/large_8192 | 15.138 | 247.740 | 492.323 | 770.339 | 54.383 | 0.06x | 0.03x | 0.02x | 0.28x |
| allocator deallocation latency/medium_1024 | 12.518 | 90.814 | 87.704 | 194.880 | 17.473 | 0.14x | 0.14x | 0.06x | 0.72x |
| allocator deallocation latency/small_32 | 3.338 | 17.889 | 5.102 | 28.732 | 6.562 | 0.19x | 0.65x | 0.12x | 0.51x |
| cross-thread free handoff/huge_2m | 1509.021 | 87807.568 | 95103.906 | N/A | 7234.668 | 0.02x | 0.02x | N/A | 0.21x |
| cross-thread free handoff/large_8192 | 28255.054 | 47980.249 | 845111.719 | 666066.016 | 74331.982 | 0.59x | 0.03x | 0.04x | 0.38x |
| cross-thread free handoff/medium_1024 | 17322.827 | 31259.497 | 136395.166 | 151323.047 | 36473.462 | 0.55x | 0.13x | 0.11x | 0.47x |
| cross-thread free handoff/small_32 | 15508.594 | 27711.377 | 16154.541 | 43690.674 | 25533.179 | 0.56x | 0.96x | 0.35x | 0.61x |
| realloc latency/cross_class_32_to_64 | 7.196 | 42.666 | 7.305 | 110.573 | 16.875 | 0.17x | 0.99x | 0.07x | 0.43x |
| realloc latency/cross_class_8k_to_16k | 47.863 | 135.261 | 66.927 | 260.388 | 53.869 | 0.35x | 0.72x | 0.18x | 0.89x |
| realloc latency/huge_shrink_4m_to_2m | 22.182 | 985723.438 | 7333.221 | 1028580.469 | 241.678 | 0.00x | 0.00x | 0.00x | 0.09x |
| realloc latency/within_class_24_to_32 | 3.067 | 42.726 | 4.424 | 61.805 | 15.268 | 0.07x | 0.69x | 0.05x | 0.20x |
| realloc latency/within_class_6k_to_8k | 23.177 | 100.772 | 55.827 | 230.308 | 51.092 | 0.23x | 0.42x | 0.10x | 0.45x |
| segment cache eviction | 232129.883 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 37993.872 | 349609.375 | 65721.680 | 838777.344 | 138819.043 | 0.11x | 0.58x | 0.05x | 0.27x |
| threaded small allocation cycles | 3613.867 | 30873.022 | 6409.074 | 72983.496 | 16011.267 | 0.12x | 0.56x | 0.05x | 0.23x |
| usable size latency/huge_2m | 23.131 | N/A | 9785.974 | N/A | 114.333 | N/A | 0.00x | N/A | 0.20x |
| usable size latency/large_8192 | 1.917 | N/A | 16.523 | 84.826 | 17.658 | N/A | 0.12x | 0.02x | 0.11x |
| usable size latency/medium_1024 | 2.872 | N/A | 5.955 | 68.051 | 10.450 | N/A | 0.48x | 0.04x | 0.27x |
| usable size latency/small_32 | 2.801 | N/A | 2.982 | 61.468 | 9.891 | N/A | 0.94x | 0.05x | 0.28x |
| usable size query latency/huge_2m | 0.409 | N/A | 0.521 | N/A | 3.186 | N/A | 0.79x | N/A | 0.13x |
| usable size query latency/large_8192 | 0.308 | N/A | 0.522 | 12.231 | 3.182 | N/A | 0.59x | 0.03x | 0.10x |
| usable size query latency/medium_1024 | 0.308 | N/A | 0.522 | 12.355 | 3.264 | N/A | 0.59x | 0.02x | 0.09x |
| usable size query latency/small_32 | 0.327 | N/A | 0.523 | 12.307 | 3.202 | N/A | 0.63x | 0.03x | 0.10x |
