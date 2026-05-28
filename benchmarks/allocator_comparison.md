# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MiMalloc (ns) | SnMalloc (ns) | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc |
| :--- | :---: | :---: | :---: | :---: | :---: |
| allocator burst retention/large_8192 | 9743.380 | 431082.690 | 19010.596 | 0.02x | 0.51x |
| allocator burst retention/medium_1024 | 3391.495 | 99008.887 | 7277.435 | 0.03x | 0.47x |
| allocator burst retention/small_32 | 3177.884 | 930.297 | 4136.514 | 3.42x | 0.77x |
| allocator cycle latency/large_8192 | 13.178 | 16.815 | 17.514 | 0.78x | 0.75x |
| allocator cycle latency/medium_1024 | 13.118 | 5.679 | 16.822 | 2.31x | 0.78x |
| allocator cycle latency/small_32 | 12.851 | 2.795 | 15.061 | 4.60x | 0.85x |
| cross-thread free handoff/medium_1024 | 28359.410 | 184750.684 | 40470.825 | 0.15x | 0.70x |
| cross-thread free handoff/small_32 | 19314.853 | 7965.961 | 25235.522 | 2.42x | 0.77x |
| segment cache eviction | 56975.928 | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 206781.836 | 81022.852 | 294141.211 | 2.55x | 0.70x |
| threaded small allocation cycles | 21665.950 | 5919.934 | 26209.000 | 3.66x | 0.83x |
