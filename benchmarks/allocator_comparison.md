# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MiMalloc (ns) | SnMalloc (ns) | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc |
| :--- | :---: | :---: | :---: | :---: | :---: |
| allocator burst retention/large_8192 | 9743.380 | 431082.690 | 19010.596 | 0.02x | 0.51x |
| allocator burst retention/medium_1024 | 3538.742 | 99008.887 | 7277.435 | 0.04x | 0.49x |
| allocator burst retention/small_32 | 3478.737 | 930.297 | 4136.514 | 3.74x | 0.84x |
| allocator cycle latency/large_8192 | 13.554 | 16.815 | 17.514 | 0.81x | 0.77x |
| allocator cycle latency/medium_1024 | 13.575 | 5.679 | 16.822 | 2.39x | 0.81x |
| allocator cycle latency/small_32 | 13.089 | 2.795 | 15.061 | 4.68x | 0.87x |
| cross-thread free handoff/medium_1024 | 28359.410 | 184750.684 | 40470.825 | 0.15x | 0.70x |
| cross-thread free handoff/small_32 | 23683.130 | 7965.961 | 25235.522 | 2.97x | 0.94x |
| segment cache eviction | 67057.739 | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 217090.430 | 81022.852 | 294141.211 | 2.68x | 0.74x |
| threaded small allocation cycles | 21665.950 | 5919.934 | 26209.000 | 3.66x | 0.83x |
