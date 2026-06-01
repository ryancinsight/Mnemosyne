# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MnemosyneHeap (ns) | BrandedHeap (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc | MnemosyneHeap vs Mnemosyne | BrandedHeap vs Mnemosyne |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/huge_2m | 523.836 | N/A | N/A | 4829.731 | 6114.065 | N/A | 1732.458 | 0.11x | 0.09x | N/A | 0.30x | N/A | N/A |
| allocator allocation latency/large_8192 | 68.795 | N/A | N/A | 217.618 | 1356.979 | 437.205 | 75.731 | 0.32x | 0.05x | 0.16x | 0.91x | N/A | N/A |
| allocator allocation latency/medium_1024 | 14.440 | N/A | N/A | 105.751 | 292.788 | 82.327 | 29.162 | 0.14x | 0.05x | 0.18x | 0.50x | N/A | N/A |
| allocator allocation latency/small_32 | 9.624 | N/A | N/A | 22.330 | 20.449 | 15.467 | 12.898 | 0.43x | 0.47x | 0.62x | 0.75x | N/A | N/A |
| allocator burst retention/large_8192 | 2049.845 | N/A | N/A | 10908.898 | 578576.035 | 22125.821 | 26336.016 | 0.19x | 0.00x | 0.09x | 0.08x | N/A | N/A |
| allocator burst retention/medium_1024 | 1287.651 | N/A | N/A | 8244.165 | 100150.241 | 7214.522 | 8737.742 | 0.16x | 0.01x | 0.18x | 0.15x | N/A | N/A |
| allocator burst retention/small_32 | 1297.065 | N/A | N/A | 10803.595 | 1196.785 | 5458.874 | 2615.959 | 0.12x | 1.08x | 0.24x | 0.50x | N/A | N/A |
| allocator cycle latency/huge_2m | 19.829 | 21.653 | 21.342 | 12142.163 | 13801.346 | N/A | 115.016 | 0.00x | 0.00x | N/A | 0.17x | 1.09x | 1.08x |
| allocator cycle latency/large_8192 | 2.926 | 2.836 | 3.580 | 23.215 | 14.894 | 16.745 | 15.418 | 0.13x | 0.20x | 0.17x | 0.19x | 0.97x | 1.22x |
| allocator cycle latency/medium_1024 | 3.445 | 3.391 | 3.389 | 21.350 | 6.203 | 16.221 | 7.242 | 0.16x | 0.56x | 0.21x | 0.48x | 0.98x | 0.98x |
| allocator cycle latency/small_32 | 2.840 | 2.760 | 2.900 | 23.227 | 3.755 | 15.304 | 6.815 | 0.12x | 0.76x | 0.19x | 0.42x | 0.97x | 1.02x |
| allocator deallocation latency/huge_2m | 868.371 | N/A | N/A | 6451.183 | 8578.884 | N/A | 3030.860 | 0.13x | 0.10x | N/A | 0.29x | N/A | N/A |
| allocator deallocation latency/large_8192 | 50.318 | N/A | N/A | 160.760 | 961.203 | 290.220 | 46.348 | 0.31x | 0.05x | 0.17x | 1.09x | N/A | N/A |
| allocator deallocation latency/medium_1024 | 13.887 | N/A | N/A | 46.752 | 146.091 | 68.785 | 16.857 | 0.30x | 0.10x | 0.20x | 0.82x | N/A | N/A |
| allocator deallocation latency/small_32 | 3.561 | N/A | N/A | 15.242 | 6.440 | 9.418 | 6.596 | 0.23x | 0.55x | 0.38x | 0.54x | N/A | N/A |
| cross-thread free handoff/huge_2m | 1202.257 | N/A | N/A | 127309.125 | 127768.494 | N/A | 7228.476 | 0.01x | 0.01x | N/A | 0.17x | N/A | N/A |
| cross-thread free handoff/large_8192 | 32352.992 | N/A | N/A | 67043.863 | 1090517.018 | 119067.248 | 77239.485 | 0.48x | 0.03x | 0.27x | 0.42x | N/A | N/A |
| cross-thread free handoff/medium_1024 | 10299.083 | N/A | N/A | 35010.951 | 179230.748 | 44466.487 | 39076.890 | 0.29x | 0.06x | 0.23x | 0.26x | N/A | N/A |
| cross-thread free handoff/small_32 | 5227.804 | N/A | N/A | 35722.945 | 8851.024 | 24340.609 | 27554.410 | 0.15x | 0.59x | 0.21x | 0.19x | N/A | N/A |
| realloc latency/cross_class_32_to_64 | 10.852 | N/A | N/A | 55.559 | 12.577 | 31.730 | 16.928 | 0.20x | 0.86x | 0.34x | 0.64x | N/A | N/A |
| realloc latency/cross_class_8k_to_16k | 67.829 | N/A | N/A | 146.582 | 137.074 | 143.255 | 57.544 | 0.46x | 0.49x | 0.47x | 1.18x | N/A | N/A |
| realloc latency/huge_shrink_4m_to_2m | 20.324 | N/A | N/A | 1256585.440 | 10212.899 | 1241518.824 | 248.343 | 0.00x | 0.00x | 0.00x | 0.08x | N/A | N/A |
| realloc latency/within_class_24_to_32 | 5.607 | N/A | N/A | 54.874 | 7.127 | 16.570 | 15.253 | 0.10x | 0.79x | 0.34x | 0.37x | N/A | N/A |
| realloc latency/within_class_6k_to_8k | 40.823 | N/A | N/A | 121.094 | 68.678 | 122.534 | 52.248 | 0.34x | 0.59x | 0.33x | 0.78x | N/A | N/A |
| segment cache eviction | 283206.688 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded medium allocation cycles | 7032.567 | N/A | N/A | 173061.651 | 20557.631 | 30563.869 | 18411.964 | 0.04x | 0.34x | 0.23x | 0.38x | N/A | N/A |
| threaded saturated small allocation cycles | 87652.912 | N/A | N/A | 404684.345 | 80049.113 | 267208.403 | 128288.271 | 0.22x | 1.09x | 0.33x | 0.68x | N/A | N/A |
| threaded small allocation cycles | 7012.698 | N/A | N/A | 38777.552 | 6726.160 | 26226.822 | 17486.846 | 0.18x | 1.04x | 0.27x | 0.40x | N/A | N/A |
| usable size latency/huge_2m | 21.342 | N/A | N/A | N/A | 11266.519 | N/A | 116.998 | N/A | 0.00x | N/A | 0.18x | N/A | N/A |
| usable size latency/large_8192 | 3.292 | N/A | N/A | N/A | 14.988 | 24.482 | 17.806 | N/A | 0.22x | 0.13x | 0.18x | N/A | N/A |
| usable size latency/medium_1024 | 5.251 | N/A | N/A | N/A | 6.816 | 16.363 | 10.254 | N/A | 0.77x | 0.32x | 0.51x | N/A | N/A |
| usable size latency/small_32 | 5.226 | N/A | N/A | N/A | 4.437 | 17.380 | 9.912 | N/A | 1.18x | 0.30x | 0.53x | N/A | N/A |
| usable size query latency/huge_2m | 0.533 | N/A | N/A | N/A | 0.903 | N/A | 3.199 | N/A | 0.59x | N/A | 0.17x | N/A | N/A |
| usable size query latency/large_8192 | 0.430 | N/A | N/A | N/A | 1.040 | 0.695 | 3.198 | N/A | 0.41x | 0.62x | 0.13x | N/A | N/A |
| usable size query latency/medium_1024 | 0.394 | N/A | N/A | N/A | 0.825 | 0.679 | 3.200 | N/A | 0.48x | 0.58x | 0.12x | N/A | N/A |
| usable size query latency/small_32 | 0.427 | N/A | N/A | N/A | 1.009 | 0.707 | 3.212 | N/A | 0.42x | 0.60x | 0.13x | N/A | N/A |
