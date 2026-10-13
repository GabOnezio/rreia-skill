# RREIA laws

Proposed by the author as design rules and starting parameters. They are not experimental results; calibrate them with measurements on the real device.

## Contents
- Formulas
- L1 to L9
- Worked example

## Formulas

- Weights: `M_pesos ≈ P × b` (P in billions of parameters, b bytes per weight; Q4_K_M ≈ 0.55, Q8_0 ≈ 1.06, FP16 = 2).
- KV cache: `M_KV = 2 × L × n_kv × d_head × T × s` (L layers, n_kv key/value heads, d_head head size, T context tokens, s = 2 bytes in FP16).
- Speed ceiling: `v_teto = B ÷ M_ativa` (B memory bandwidth in GB/s, M_ativa GB of weights read per token). Real speed ≈ `η × v_teto`, with η between 0.5 and 0.8 in the published sources, and 0.82 to 0.92 in the NVIDIA Jetson figures checked.
- MoE models: memory counts total parameters, speed counts active parameters.

## The laws

| Law | Rule | Starting value |
|---|---|---|
| L1 Memory | Peak memory (weights + KV cache + reserve) stays below a fraction of physical memory. | f = 0.75 |
| L2 Bandwidth | Accept a model only if `η × B ÷ M_ativa` reaches the task's minimum tokens per second. | η = 0.5; minimum set per task (5 tok/s for chat is a reasonable start) |
| L3 Context | Cap the context at what the memory budget holds: `T_max = (f × M_fisica − M_pesos − M_reserva) ÷ bytes_per_token`. | derived from L1 |
| L4 Priority | Queue by class P0 to P3. Preempt between requests only, never in the middle of a generation. Effective class = min(requested, key maximum). | 4 classes |
| L5 Degradation | Under heat, memory pressure or load, step down one level (smaller model or lower quantization) before refusing. | device thermal limit |
| L6 Audit trail | Every decision (model chosen, budget, refusal, action taken) produces an auditable record without private content. | 100% of decisions |
| L7 Cause | Classify a bad answer by cause (memory, bandwidth, context, integration, model) before changing the model. | cause table |
| L8 Local first | Use a free local model when L1 to L3 accept it; go remote only if admission fails, the task exceeds the local tier's capability, or the developer chose remote. | developer's choice |
| L9 Cost budget | Remote calls stop when the spending cap for the period is reached, and the system falls back to local or to a clear refusal. | cap set by the developer |

L8 and L9 are the planned extension for developers who allow paid APIs. L1 to L4 and L6 are implemented and tested in `assets/rreia-jev` (admission, priority queue, key permissions, audit line). L5 has no code yet, and L7 exists there only as distinct error codes (admission exit code, 413, 502, 504), not as a cause record; both are for the application to implement. L8 and L9 are policy for the application to implement (see `cost-and-fallback.md`).

## Worked example

PC with 16 GB, DDR4-3200 dual channel (51.2 GB/s), Llama 3.1 8B in Q4_K_M (4.9 GB, 32 layers, 8 KV heads, head size 128), 2 GB reserve:

- L1/L3 budget: `0.75 × 16 − 4.9 − 2 = 5.1 GB` for the KV cache.
- KV per token: `2 × 32 × 8 × 128 × 2 = 131,072 bytes`, so `T_max ≈ 38,909 tokens`.
- L2: ceiling `51.2 ÷ 4.9 ≈ 10.4 tok/s`; floor `0.5 × 10.4 ≈ 5.2 tok/s`. Accepted for a 5 tok/s task.
- Same model on an 8 GB PC: `0.75 × 8 − 4.9 − 2 = −0.9 GB`. Rejected by L1.
