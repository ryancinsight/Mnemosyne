# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator burst retention/large_8192 | 8558.102 | 523983.206 | 19989.143 | N/A | 0.02x | 0.43x | N/A |
| allocator burst retention/medium_1024 | 3844.775 | 102133.846 | 7607.013 | N/A | 0.04x | 0.51x | N/A |
| allocator burst retention/small_32 | 3444.365 | 1076.564 | 4388.343 | N/A | 3.20x | 0.78x | N/A |
| allocator cycle latency/large_8192 | 14.166 | 15.157 | 17.213 | N/A | 0.93x | 0.82x | N/A |
| allocator cycle latency/medium_1024 | 13.633 | 6.109 | 16.410 | N/A | 2.23x | 0.83x | N/A |
| allocator cycle latency/small_32 | 12.975 | 2.781 | 16.407 | N/A | 4.67x | 0.79x | N/A |
| cross-thread free handoff/medium_1024 | 29595.096 | 189914.770 | 42633.831 | N/A | 0.16x | 0.69x | N/A |
| cross-thread free handoff/small_32 | 23387.667 | 21452.626 | 25740.203 | N/A | 1.09x | 0.91x | N/A |
| realloc latency/cross_class_32_to_64 | 36.456 | 13.219 | 50.455 | N/A | 2.76x | 0.72x | N/A |
| realloc latency/within_class_24_to_32 | 13.831 | 5.508 | 21.696 | N/A | 2.51x | 0.64x | N/A |
| segment cache eviction | 59078.183 | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 201364.191 | 55540.651 | 264325.501 | N/A | 3.63x | 0.76x | N/A |
| threaded small allocation cycles | 39867.045 | 7434.712 | 26918.384 | N/A | 5.36x | 1.48x | N/A |
| usable size latency/medium_1024 | 15.593 | 8.373 | 22.415 | N/A | 1.86x | 0.70x | N/A |
| usable size latency/small_32 | 16.077 | 3.047 | 26.209 | N/A | 5.28x | 0.61x | N/A |
| usable size query latency/medium_1024 | 0.383 | 0.544 | 0.520 | N/A | 0.70x | 0.74x | N/A |
| usable size query latency/small_32 | 0.411 | 0.589 | 0.497 | N/A | 0.70x | 0.83x | N/A |
