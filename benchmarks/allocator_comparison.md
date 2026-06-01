# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MnemosyneHeap (ns) | BrandedHeap (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc | MnemosyneHeap vs Mnemosyne | BrandedHeap vs Mnemosyne |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 740.880 | N/A | N/A | 3196.944 | 4907.639 | N/A | 1732.458 | 0.23x | 0.15x | N/A | 0.43x | N/A | N/A |
| allocator allocation latency/large_8192 | 21.916 | N/A | N/A | 287.490 | 1300.092 | 409.163 | 75.731 | 0.08x | 0.02x | 0.05x | 0.29x | N/A | N/A |
| allocator allocation latency/medium_1024 | 16.693 | N/A | N/A | 77.009 | 318.824 | 65.361 | 29.162 | 0.22x | 0.05x | 0.26x | 0.57x | N/A | N/A |
| allocator allocation latency/small_32 | 10.346 | N/A | N/A | 27.721 | 20.263 | 16.390 | 12.898 | 0.37x | 0.51x | 0.63x | 0.80x | N/A | N/A |
| allocator burst retention/large_8192 | 2071.875 | N/A | N/A | 10932.256 | 512811.693 | 17965.917 | 26336.016 | 0.19x | 0.00x | 0.12x | 0.08x | N/A | N/A |
| allocator burst retention/medium_1024 | 1175.475 | N/A | N/A | 9003.423 | 132451.564 | 7927.791 | 8737.742 | 0.13x | 0.01x | 0.15x | 0.13x | N/A | N/A |
| allocator burst retention/small_32 | 631.534 | N/A | N/A | 6868.554 | 1176.768 | 4036.007 | 2615.959 | 0.09x | 0.54x | 0.16x | 0.24x | N/A | N/A |
| allocator cycle latency/huge_2m | 22.583 | 22.387 | 24.229 | 10795.547 | 12794.170 | N/A | 115.016 | 0.00x | 0.00x | N/A | 0.20x | 0.99x | 1.07x |
| allocator cycle latency/large_8192 | 2.392 | 2.280 | 2.362 | 22.288 | 16.844 | 18.932 | 15.418 | 0.11x | 0.14x | 0.13x | 0.16x | 0.95x | 0.99x |
| allocator cycle latency/medium_1024 | 2.328 | 2.153 | 2.376 | 23.162 | 6.968 | 18.965 | 7.242 | 0.10x | 0.33x | 0.12x | 0.32x | 0.92x | 1.02x |
| allocator cycle latency/small_32 | 2.336 | 2.174 | 2.367 | 26.522 | 3.852 | 17.736 | 6.815 | 0.09x | 0.61x | 0.13x | 0.34x | 0.93x | 1.01x |
| allocator deallocation latency/huge_2m | 1150.337 | N/A | N/A | 5347.154 | 5361.130 | N/A | 3030.860 | 0.22x | 0.21x | N/A | 0.38x | N/A | N/A |
| allocator deallocation latency/large_8192 | 18.837 | N/A | N/A | 148.423 | 731.293 | 164.312 | 46.348 | 0.13x | 0.03x | 0.11x | 0.41x | N/A | N/A |
| allocator deallocation latency/medium_1024 | 8.671 | N/A | N/A | 25.070 | 99.664 | 46.913 | 16.857 | 0.35x | 0.09x | 0.18x | 0.51x | N/A | N/A |
| allocator deallocation latency/small_32 | 3.210 | N/A | N/A | 16.819 | 5.810 | 9.594 | 6.596 | 0.19x | 0.55x | 0.33x | 0.49x | N/A | N/A |
| cross-thread free handoff/huge_2m | 989.983 | N/A | N/A | 113443.265 | 116248.056 | N/A | 7228.476 | 0.01x | 0.01x | N/A | 0.14x | N/A | N/A |
| cross-thread free handoff/large_8192 | 33233.421 | N/A | N/A | 66266.052 | 1121739.809 | 101438.985 | 77239.485 | 0.50x | 0.03x | 0.33x | 0.43x | N/A | N/A |
| cross-thread free handoff/medium_1024 | 10296.648 | N/A | N/A | 33050.322 | 184423.302 | 38910.544 | 39076.890 | 0.31x | 0.06x | 0.26x | 0.26x | N/A | N/A |
| cross-thread free handoff/small_32 | 4632.893 | N/A | N/A | 30347.303 | 8779.679 | 21633.216 | 27554.410 | 0.15x | 0.53x | 0.21x | 0.17x | N/A | N/A |
| realloc latency/cross_class_32_to_64 | 11.545 | N/A | N/A | 53.591 | 12.799 | 32.857 | 16.928 | 0.22x | 0.90x | 0.35x | 0.68x | N/A | N/A |
| realloc latency/cross_class_8k_to_16k | 56.402 | N/A | N/A | 158.932 | 114.372 | 147.412 | 57.544 | 0.35x | 0.49x | 0.38x | 0.98x | N/A | N/A |
| realloc latency/huge_shrink_4m_to_2m | 20.882 | N/A | N/A | 1032810.837 | 8330.715 | 1244878.910 | 248.343 | 0.00x | 0.00x | 0.00x | 0.08x | N/A | N/A |
| realloc latency/within_class_24_to_32 | 4.415 | N/A | N/A | 53.701 | 6.679 | 17.835 | 15.253 | 0.08x | 0.66x | 0.25x | 0.29x | N/A | N/A |
| realloc latency/within_class_6k_to_8k | 37.045 | N/A | N/A | 111.130 | 66.018 | 103.591 | 52.248 | 0.33x | 0.56x | 0.36x | 0.71x | N/A | N/A |
| segment cache eviction | 231990.190 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 5230.070 | N/A | N/A | 34475.933 | 16674.075 | 25519.450 | 18411.964 | 0.15x | 0.31x | 0.20x | 0.28x | N/A | N/A |
| threaded saturated small allocation cycles | 66242.742 | N/A | N/A | 407793.015 | 76967.781 | 274229.171 | 128288.271 | 0.16x | 0.86x | 0.24x | 0.52x | N/A | N/A |
| threaded small allocation cycles | 5552.537 | N/A | N/A | 32202.291 | 5914.932 | 26008.371 | 17486.846 | 0.17x | 0.94x | 0.21x | 0.32x | N/A | N/A |
| usable size latency/huge_2m | 22.103 | N/A | N/A | N/A | 8593.999 | N/A | 116.998 | N/A | 0.00x | N/A | 0.19x | N/A | N/A |
| usable size latency/large_8192 | 2.624 | N/A | N/A | N/A | 16.097 | 17.526 | 17.806 | N/A | 0.16x | 0.15x | 0.15x | N/A | N/A |
| usable size latency/medium_1024 | 3.443 | N/A | N/A | N/A | 6.834 | 17.220 | 10.254 | N/A | 0.50x | 0.20x | 0.34x | N/A | N/A |
| usable size latency/small_32 | 2.450 | N/A | N/A | N/A | 3.342 | 16.573 | 9.912 | N/A | 0.73x | 0.15x | 0.25x | N/A | N/A |
| usable size query latency/huge_2m | 0.428 | N/A | N/A | N/A | 0.716 | N/A | 3.199 | N/A | 0.60x | N/A | 0.13x | N/A | N/A |
| usable size query latency/large_8192 | 0.282 | N/A | N/A | N/A | 0.644 | 0.551 | 3.198 | N/A | 0.44x | 0.51x | 0.09x | N/A | N/A |
| usable size query latency/medium_1024 | 0.397 | N/A | N/A | N/A | 0.924 | 0.625 | 3.200 | N/A | 0.43x | 0.63x | 0.12x | N/A | N/A |
| usable size query latency/small_32 | 0.362 | N/A | N/A | N/A | 0.747 | 0.645 | 3.212 | N/A | 0.48x | 0.56x | 0.11x | N/A | N/A |
