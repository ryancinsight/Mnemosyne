# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator burst retention/large_8192 | 8558.102 | 523983.206 | 19989.143 | N/A | 0.02x | 0.43x | N/A |
| allocator burst retention/medium_1024 | 3844.775 | 102133.846 | 7607.013 | N/A | 0.04x | 0.51x | N/A |
| allocator burst retention/small_32 | 3444.365 | 1076.564 | 4388.343 | N/A | 3.20x | 0.78x | N/A |
| allocator cycle latency/large_8192 | 13.455 | 15.766 | 18.256 | N/A | 0.85x | 0.74x | N/A |
| allocator cycle latency/medium_1024 | 13.217 | 6.439 | 17.041 | N/A | 2.05x | 0.78x | N/A |
| allocator cycle latency/small_32 | 13.394 | 3.031 | 17.085 | N/A | 4.42x | 0.78x | N/A |
| cross-thread free handoff/medium_1024 | 29595.096 | 189914.770 | 42633.831 | N/A | 0.16x | 0.69x | N/A |
| cross-thread free handoff/small_32 | 23387.667 | 21452.626 | 25740.203 | N/A | 1.09x | 0.91x | N/A |
| segment cache eviction | 59078.183 | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 213081.821 | 151758.326 | 454925.538 | N/A | 1.40x | 0.47x | N/A |
| threaded small allocation cycles | 39867.045 | 7434.712 | 26918.384 | N/A | 5.36x | 1.48x | N/A |
