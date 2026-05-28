# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MiMalloc (ns) | SnMalloc (ns) | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc |
| :--- | :---: | :---: | :---: | :---: | :---: |
| allocator burst retention/large_8192 | 8138.002 | 504206.596 | 19199.061 | 0.02x | 0.42x |
| allocator burst retention/medium_1024 | 3657.976 | 89825.526 | 7289.730 | 0.04x | 0.50x |
| allocator burst retention/small_32 | 3420.556 | 1232.755 | 4060.147 | 2.77x | 0.84x |
| allocator cycle latency/large_8192 | 13.785 | 14.954 | 17.209 | 0.92x | 0.80x |
| allocator cycle latency/medium_1024 | 13.865 | 6.164 | 16.394 | 2.25x | 0.85x |
| allocator cycle latency/small_32 | 12.961 | 2.815 | 16.348 | 4.60x | 0.79x |
| cross-thread free handoff/medium_1024 | 27726.782 | 177615.830 | 36294.049 | 0.16x | 0.76x |
| cross-thread free handoff/small_32 | 21623.121 | 20723.864 | 22583.750 | 1.04x | 0.96x |
| segment cache eviction | 70720.456 | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 207070.840 | 75726.595 | 283351.290 | 2.73x | 0.73x |
| threaded small allocation cycles | 36367.103 | 5943.689 | 23178.870 | 6.12x | 1.57x |
