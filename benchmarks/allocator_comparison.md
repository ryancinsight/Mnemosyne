# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MiMalloc (ns) | SnMalloc (ns) | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc |
| :--- | :---: | :---: | :---: | :---: | :---: |
| allocator burst retention/large_8192 | 9743.380 | 431082.690 | 19010.596 | 0.02x | 0.51x |
| allocator burst retention/medium_1024 | 3374.161 | 78352.393 | 7374.597 | 0.04x | 0.46x |
| allocator burst retention/small_32 | 3215.472 | 847.898 | 4267.267 | 3.79x | 0.75x |
| allocator cycle latency/large_8192 | 13.654 | 16.839 | 17.296 | 0.81x | 0.79x |
| allocator cycle latency/medium_1024 | 13.481 | 5.624 | 16.837 | 2.40x | 0.80x |
| allocator cycle latency/small_32 | 13.113 | 2.782 | 15.073 | 4.71x | 0.87x |
| cross-thread free handoff/medium_1024 | 28359.410 | 184750.684 | 40470.825 | 0.15x | 0.70x |
| cross-thread free handoff/small_32 | 23683.130 | 7965.961 | 25235.522 | 2.97x | 0.94x |
| segment cache eviction | 67057.739 | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 217090.430 | 81022.852 | 294141.211 | 2.68x | 0.74x |
| threaded small allocation cycles | 21665.950 | 5919.934 | 26209.000 | 3.66x | 0.83x |
