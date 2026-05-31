# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 2435.076 | 2652.795 | 4008.340 | N/A | 1811.642 | 0.92x | 0.61x | N/A | 1.34x |
| allocator allocation latency/large_8192 | 22.570 | 725.579 | 1061.288 | 822.631 | 116.025 | 0.03x | 0.02x | 0.03x | 0.19x |
| allocator allocation latency/medium_1024 | 10.988 | 165.931 | 247.731 | 170.026 | 32.537 | 0.07x | 0.04x | 0.06x | 0.34x |
| allocator allocation latency/small_32 | 9.301 | 31.657 | 14.549 | 46.756 | 12.835 | 0.29x | 0.64x | 0.20x | 0.72x |
| allocator burst retention/large_8192 | 2637.437 | 8778.595 | 390084.766 | 163148.926 | 26516.699 | 0.30x | 0.01x | 0.02x | 0.10x |
| allocator burst retention/medium_1024 | 905.558 | 6657.764 | 75994.434 | 45714.233 | 9110.614 | 0.14x | 0.01x | 0.02x | 0.10x |
| allocator burst retention/small_32 | 452.121 | 6374.658 | 816.062 | 13172.791 | 2565.501 | 0.07x | 0.55x | 0.03x | 0.18x |
| allocator cycle latency/huge_2m | 31.309 | 7595.001 | 8463.000 | N/A | 121.328 | 0.00x | 0.00x | N/A | 0.26x |
| allocator cycle latency/large_8192 | 1.838 | 20.203 | 16.320 | 73.005 | 15.363 | 0.09x | 0.11x | 0.03x | 0.12x |
| allocator cycle latency/medium_1024 | 1.794 | 20.197 | 5.612 | 56.154 | 7.265 | 0.09x | 0.32x | 0.03x | 0.25x |
| allocator cycle latency/small_32 | 1.714 | 20.147 | 2.771 | 48.880 | 6.886 | 0.09x | 0.62x | 0.04x | 0.25x |
| allocator deallocation latency/huge_2m | 4562.897 | 3967.719 | 4323.737 | N/A | 2998.712 | 1.15x | 1.06x | N/A | 1.52x |
| allocator deallocation latency/large_8192 | 191.907 | 260.049 | 501.227 | 785.566 | 50.245 | 0.74x | 0.38x | 0.24x | 3.82x |
| allocator deallocation latency/medium_1024 | 13.123 | 56.625 | 93.151 | 195.888 | 17.194 | 0.23x | 0.14x | 0.07x | 0.76x |
| allocator deallocation latency/small_32 | 3.303 | 15.218 | 5.020 | 28.202 | 6.534 | 0.22x | 0.66x | 0.12x | 0.51x |
| cross-thread free handoff/huge_2m | 2178.661 | 82309.619 | 89530.029 | N/A | 6901.072 | 0.03x | 0.02x | N/A | 0.32x |
| cross-thread free handoff/large_8192 | 29657.520 | 51974.023 | 838837.305 | 691561.328 | 79256.201 | 0.57x | 0.04x | 0.04x | 0.37x |
| cross-thread free handoff/medium_1024 | 17558.337 | 31196.118 | 140325.977 | 153990.430 | 36385.547 | 0.56x | 0.13x | 0.11x | 0.48x |
| cross-thread free handoff/small_32 | 14965.796 | 28230.884 | 17015.845 | 44442.017 | 26023.438 | 0.53x | 0.88x | 0.34x | 0.58x |
| realloc latency/cross_class_32_to_64 | 7.156 | 42.552 | 7.423 | 110.368 | 19.170 | 0.17x | 0.96x | 0.06x | 0.37x |
| realloc latency/cross_class_8k_to_16k | 57.974 | 129.272 | 66.665 | 260.169 | 55.501 | 0.45x | 0.87x | 0.22x | 1.04x |
| realloc latency/huge_shrink_4m_to_2m | 34.157 | 934056.250 | 7442.456 | 1023842.969 | 252.761 | 0.00x | 0.00x | 0.00x | 0.14x |
| realloc latency/within_class_24_to_32 | 3.062 | 43.336 | 4.322 | 61.317 | 18.018 | 0.07x | 0.71x | 0.05x | 0.17x |
| realloc latency/within_class_6k_to_8k | 23.421 | 97.236 | 55.835 | 225.965 | 50.567 | 0.24x | 0.42x | 0.10x | 0.46x |
| segment cache eviction | 204581.445 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 41238.062 | 347259.766 | 60099.414 | 841988.281 | 134120.703 | 0.12x | 0.69x | 0.05x | 0.31x |
| threaded small allocation cycles | 4334.851 | 30962.524 | 6417.020 | 60331.042 | 15637.463 | 0.14x | 0.68x | 0.07x | 0.28x |
| usable size latency/huge_2m | 31.380 | N/A | 6135.449 | N/A | 121.774 | N/A | 0.01x | N/A | 0.26x |
| usable size latency/large_8192 | 1.910 | N/A | 16.372 | 84.578 | 18.398 | N/A | 0.12x | 0.02x | 0.10x |
| usable size latency/medium_1024 | 3.055 | N/A | 6.016 | 69.616 | 11.904 | N/A | 0.51x | 0.04x | 0.26x |
| usable size latency/small_32 | 2.966 | N/A | 2.792 | 61.886 | 11.380 | N/A | 1.06x | 0.05x | 0.26x |
| usable size query latency/huge_2m | 0.409 | N/A | 0.525 | N/A | 3.211 | N/A | 0.78x | N/A | 0.13x |
| usable size query latency/large_8192 | 0.283 | N/A | 0.529 | 12.271 | 3.185 | N/A | 0.53x | 0.02x | 0.09x |
| usable size query latency/medium_1024 | 0.267 | N/A | 0.521 | 12.346 | 3.180 | N/A | 0.51x | 0.02x | 0.08x |
| usable size query latency/small_32 | 0.270 | N/A | 0.525 | 12.197 | 3.199 | N/A | 0.52x | 0.02x | 0.08x |
