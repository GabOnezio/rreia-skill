# Hardware tiers

Use these to place a device and to fill in unknowns. Speeds are orders of magnitude from the cited sources, not measurements on the developer's machine.

| Tier | Example device | Memory | Bandwidth | Viable models (Q4) | Generation speed | Origin of the number |
|---|---|---|---|---|---|---|
| R0 | Raspberry Pi 5 | 4 to 8 GB | not published in the sources; LPDDR4X-4267 on a 32-bit bus gives about 17 GB/s by calculation | 1B to 2B (Gemma 3 1B, Llama 3.2 1B, Qwen2.5 1.5B); 3B only for non-interactive work | 1B to 1.7B: 8.2 to 11.5 tok/s; 3B: 4.7 to 5.2 tok/s | Measured by Stratosphere (Ollama) |
| R1 | PC or laptop without GPU | 16 GB | about 51 GB/s (DDR4-3200 dual) to 90 GB/s (DDR5-5600 dual), theoretical | 4B to 8B | 8B Q4: 5 to 8 tok/s (DDR4), 9 to 14 tok/s (DDR5) | Calculated from the formula, not measured |
| R2 | NVIDIA Jetson Orin Nano 8 GB (Super mode) | 8 GB unified | 102 GB/s | 3B to 8B | Llama 3.2 3B 43.1; Qwen 2.5 7B 21.8; Llama 3.1 8B 19.1 tok/s (INT4, MLC) | Vendor figures (NVIDIA) |
| R3 | Apple M4 Max or RTX 4090 | up to 128 GB unified; 24 GB VRAM | 410 to 546 GB/s; about 1,008 GB/s | 8B to 32B; 70B needs about 40 GB free | 8B Q4: 40 to 80 tok/s (M4 Max); 90 to 140 tok/s (RTX 4090) | Blog ranges (InventiveHQ) |

Practical minimum for a useful chat assistant today: a 4B to 8B model in Q4 with 16 GB. Below that, 1B to 2B models for short tasks (classification, extraction, short commands).

## Model sizes (Q4_K_M files, approximate)

Llama 3.2 1B 0.77 GB; Llama 3.2 3B 1.93 GB; Gemma 3 1B 0.78 GB; Llama 3.1 8B 4.4 to 4.9 GB. Always confirm the file size on the model card and the license in the repository before commercial use (Llama and Gemma have their own terms; Qwen3 and Mistral 7B are listed as Apache 2.0 in the sources, but check).

## Architecture values for KV cache

Llama 3.1 8B: 32 layers, 8 KV heads, head size 128 (131,072 bytes per token in FP16). For any other model read `config.json`: `num_hidden_layers`, `num_key_value_heads`, and `head_dim` (or `hidden_size ÷ num_attention_heads`).

## Typical memory bandwidths for estimates

DDR4-3200 dual channel 51.2 GB/s; DDR5-5600 dual channel 89.6 GB/s; Jetson Orin Nano Super 102 GB/s. These are theoretical peaks; say so when you use them.

## Runtimes

llama.cpp (`llama-server`), Ollama, mistral.rs (Rust, OpenAI-compatible server) and candle (Rust library) run quantized models locally. Different runtimes give different speeds on the same hardware; measure with the one the project will use.
