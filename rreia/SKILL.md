---
name: rreia
description: "RREIA (Resource-Responsible Edge AI): use when building any system that runs or calls AI models and has hardware limits, actions, rules or laws, permissions between people or agents, priorities, or audit needs. It checks the target hardware against minimums (memory, bandwidth, context), interviews the developer about the device and OS when unknown, prefers free local models (Llama, Qwen, Gemma) and asks before using paid online APIs, then designs actions, permissions, laws and an audit trail, and can bring up Jev Local for typed decisions. Trigger on /rreia, edge AI, local LLM, on-device AI, Raspberry Pi, Jetson, llama.cpp, Ollama, GGUF, 'will this model run on my machine', AI cost or budget, IA de borda, IA local, hardware mínimo para IA, permissões, prioridade de IA."
license: MIT
compatibility: "Any coding agent that reads Agent Skills (Claude Code, Claude CLI, OpenAI Codex). scripts/probe_hardware.py needs Python 3.8+; the optional Jev Local server needs a Rust toolchain."
metadata:
  author: Gabriel Onezio Ferreira
  version: "0.1.0"
---

# RREIA: Resource-Responsible Edge AI

Most "the AI is bad" complaints are really resource problems: a model that does not fit in memory, a memory bus too slow for the model, a context that was silently truncated, a timeout shorter than the generation. RREIA treats hardware, models, runtime, cost, data and legal duties as resources with a budget, an owner and a priority, the way HR treats people. The job of this skill is to make that engineering happen *before* the code is written, and to keep it enforced in the code afterwards.

Paths below are relative to this skill's folder. Talk to the developer in their own language. Keep questions few and concrete. Never run anything on their machine without asking first.

## The flow

Work through these steps in order. Skip a step only when its answer is already known from the conversation or the repo.

### 1. Find out where the system will run

Ask one question first: **"Will this system run on this device (the one I am working on)?"** Offer the options: yes, no, or not decided yet.

- **Yes.** Ask permission to run the read-only probe (it reads RAM, CPU, GPU and OS, uses no network, writes nothing): `python scripts/probe_hardware.py`. If the developer declines or you have no shell, ask the hardware questions below instead.
- **No, or not decided.** Do not guess. Interview the developer. Ask in small groups (two to four questions at a time), only what you still lack:
  1. Device type and model (laptop, desktop, Raspberry Pi, Jetson, Mac, server, phone).
  2. Total memory: RAM, or VRAM for a discrete GPU, or unified memory.
  3. Memory type and speed if known (DDR4-3200, DDR5, LPDDR5). It sets the speed ceiling (law L2). If unknown, use the closest typical value from `references/hardware-tiers.md` and say it is an estimate.
  4. GPU or accelerator, if any.
  5. Operating system and version, and whether the device is dedicated or shared with other programs.
  6. Must it work offline? Power or heat limits? (battery, fanless case)
  7. How many users or requests at the same time, and what response time is acceptable.

Write the answers down in `RREIA.md` at the project root (template in `references/rreia-md-template.md`) so the decisions survive the session.

### 2. Ask the model and cost preference

The default is a **free local model** (Llama, Qwen, Gemma, Mistral under their licenses): it costs no money per request, keeps data on the device and works offline. A paid online model or API is allowed, but it is the developer's decision, so ask:

- Local only, local first with a paid API as fallback, or paid API first?
- If a paid API is involved: monthly budget cap, which provider, and whether the data may leave the device (personal data, trade secrets).
- Is the license acceptable for the use (commercial use of Llama and Gemma has their own terms; check the model repository).

Record the answer as law L8 (local first) and L9 (cost budget). Details and the routing policy are in `references/cost-and-fallback.md`.

### 3. Check compatibility

Compare the device with the model using the three laws below. Run `python scripts/probe_hardware.py --model llama3.1-8b --ram-gb <N> --memory-kind ddr4-3200-dual` or do the arithmetic by hand with `references/laws.md`. For a model not in the script, read its `config.json` for `num_hidden_layers`, `num_key_value_heads` and `head_dim` (or `hidden_size / num_attention_heads`), and take the file size of the quantized weights from the model card.

- **L1 memory:** weights + KV cache + reserve must fit in 75% of physical memory.
- **L2 bandwidth:** `0.5 × bandwidth ÷ bytes read per token` must reach the task's minimum tokens per second.
- **L3 context:** the context limit is what the leftover memory holds (`131,072 bytes per token` for Llama 3.1 8B in FP16 cache).

Report the verdict plainly, with numbers:

- **Fits:** continue to step 4.
- **Does not fit:** do not write the program as if it did. Offer options and let the developer pick: a smaller model or a more aggressive quantization, less context, more RAM or a faster memory kit, a different device tier, or the paid API route. State what each costs in quality or money.
- **Unknown numbers:** say which numbers are estimates. The speed figures are orders of magnitude from published sources, not measurements on this machine. Step 6 measures.

Everything above is a design aid, not a guarantee. The thresholds (f = 0.75, η = 0.5) are the author's starting values to calibrate.

### 4. Model the system as actions, principals, permissions, laws and activities

Before code, write this table in `RREIA.md` and confirm it with the developer. It is what makes a system with collaboration and rules safe to hand to an AI:

| Piece | Question to answer | Example |
|---|---|---|
| Actions | What can the system do? Mark each as read-only, reversible, or irreversible. | `classify_ticket` (read-only), `issue_refund` (irreversible) |
| Principals | Who asks: humans, services, other agents? | support agent, billing service, batch indexer |
| Permissions | Which principal may request which action, and up to which priority class? | billing service: `issue_refund`, max P1 |
| Laws | Which rules must always hold? | refund above R$ 500 needs human approval |
| Activities | What gets recorded about each decision? | who, which action, which class, outcome, never the private content |

Principles that apply to every row:

- **Laws live in code, not in the prompt.** A prompt is a request the model may ignore; a check in code is a guarantee. The model proposes; deterministic code decides and enforces.
- **Irreversible or high-impact actions need a human confirmation step**, and the model's output is a proposal with a confidence, never a command. This also fits LGPD art. 20 and the EU AI Act human-oversight idea; it is design guidance, not legal advice, so tell the developer to confirm with a lawyer.
- **Priority classes (L4):** P0 operational commands, P1 customer-facing, P2 on-demand internal, P3 background batch. The effective class is `min(requested, the maximum the caller's key allows)`. Preempt between requests, never in the middle of a generation.
- **Audit trail (L6):** one record per decision with timestamp, class, outcome, queue time, latency, tokens. Leave the private content out of the log (data minimization).
- **Degrade before refusing (L5):** under heat, memory pressure or load, step down to a smaller model or shorter context before rejecting work.
- **Classify failures by cause (L7)** (memory, bandwidth, context, integration, model) before blaming or swapping the model.

### 5. Build

Match the project: keep its language and framework. When a new service is needed and speed or footprint matters, prefer Rust for the control layer (admission, queue, permissions, audit); that preference is a hypothesis of the RREIA design, not a measured result, so do not rewrite existing code for it.

- **Inference:** run the model with an OpenAI-compatible server: `llama-server` (llama.cpp), Ollama, or mistral.rs. Talk to it over `/v1/chat/completions`. This keeps the model swappable, including swapping to a paid API behind the same interface.
- **Typed decisions:** when the system needs a classification, routing, score or yes/no, do not parse free text. Use Jev Local (`assets/rreia-jev/`, how-to in `references/jev-local.md`): the application sends state plus typed questions (`choice`, `score`, `noul`) and receives an option with a probability and a confidence.
- **Remote fallback:** put it behind the same client interface and the L8/L9 policy. Keys come from environment variables, never from source. Count tokens and cost per call.
- **Limits in the config, not scattered:** one place for memory fraction, minimum speed, context limit, queue size, timeouts and budget.

### 6. Verify and report

- Write tests for the laws themselves: a model that does not fit is rejected, a P0 request jumps a P3 backlog, a key capped at P3 cannot get P0, an input over the context limit is refused, the budget cutoff stops remote calls, an action without permission is denied.
- If you have the real device and permission, measure tokens per second and time to first token with the real model, and compare with the L2 floor. If the measured efficiency η is far from 0.5, tell the developer and propose a recalibration.
- Finish with a short report: the device and model chosen, the verdict with numbers, which numbers were estimated versus measured, what is enforced in code, and what was not tested (for example, no run against a real model).

## Reference files (read when needed)

- `references/laws.md`: laws L1 to L9 with formulas and worked examples.
- `references/hardware-tiers.md`: tiers R0 to R3, typical bandwidths, model sizes, where each number came from.
- `references/jev-local.md`: what Jev is, the request and response format, how to run and call Jev Local.
- `references/cost-and-fallback.md`: local-first routing, budget, privacy questions.
- `references/rreia-md-template.md`: the project record to fill in.
- `references/install.md`: install in Claude Code, Claude CLI and Codex.
- `scripts/probe_hardware.py`: read-only hardware probe and L1 to L3 checker.
- `assets/rreia-jev/`: Rust source of Jev Local (20 tests).
