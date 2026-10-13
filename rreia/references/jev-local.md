# Jev Local

## What Jev is

Jev is a "System One" model from TypeSafe AI: it does not write text. The application sends its **state** and a set of **typed questions**; the model answers each with an option or a number plus a probability or confidence. The point is to make bounded software decisions (routing, triage, scoring) with a typed result the program can branch on, instead of parsing free text.

Source and limits: this description and the endpoint format come from third-party pages about Jev and its open-source bridge LocalJev (TypeScript on Bun), not from a specification the RREIA author controls. Jev Local here is an independent implementation in Rust, written to accept the same kind of request. Compare with the real Jev documentation before claiming exact compatibility.

## What Jev Local adds (RREIA)

- Hardware admission at startup (L1 to L3): the server refuses to start when the model does not fit or is too slow, and derives the context limit.
- Priority queue (L4) with a single worker: one generation at a time, next one chosen by class, then by age.
- Permissions: each API key has a maximum class; the effective class is `min(requested, key maximum)`.
- Audit trail (L6): one JSON line per decision on stdout, without the state content.
- Retry of malformed model output (up to 3 attempts), confidence from entropy.

Probabilities are produced by the model as text, not read from its logits, so they are self-reported. Check calibration on your own data before trusting a confidence number.

## Request

`POST /v1/systemone`, headers: `Authorization: Bearer <key>` (if keys are configured), `X-RREIA-Class: P0|P1|P2|P3` (default P2).

```json
{
  "state": { "assunto": "Cobrança duplicada no cartão", "cliente": "prime" },
  "questions": {
    "rota":      { "type": "choice", "instructions": "Qual fila atende?",
                   "criteria": { "financeiro": "cobranças", "suporte": "dúvidas técnicas" } },
    "urgencia":  { "type": "score",  "instructions": "Quão urgente?",
                   "criteria": ["baixa", "media", "alta"] },
    "reembolso": { "type": "noul",   "instructions": "O cliente pede reembolso?" }
  }
}
```

- `choice`: `criteria` is an object with 2 or more options (name → description).
- `score`: `criteria` is an ordered list of 2 or more levels; the score is the probability-weighted level index.
- `noul`: a statement to judge; the answer is the probability that it is true.

## Response

```json
{ "model": "llama-3.1-8b-q4_k_m",
  "answers": {
    "rota": { "type": "choice", "choice": "financeiro",
              "probabilities": { "financeiro": 0.9, "suporte": 0.1 }, "confidence": 0.531 },
    "urgencia": { "type": "score", "score": 1.5, "legend": {"0":"baixa","1":"media","2":"alta"},
                  "probabilities": {"0":0.1,"1":0.3,"2":0.6}, "confidence": 0.1827 },
    "reembolso": { "type": "noul", "noul": 0.8 } },
  "usage": { "input_tokens": 100, "output_tokens": 20 } }
```

Response headers: `X-RREIA-Class`, `X-RREIA-Queue-Ms`, `X-RREIA-Attempts`.

Errors: 400 invalid request, 401 bad key, 413 input over the context limit (L3), 502 model unreachable or no valid answer after 3 tries, 503 queue full, 504 timeout. `GET /ready` returns status, model, context limit and queue length.

## Run it

```bash
# 1. a local model behind an OpenAI-compatible server, for example:
llama-server -m model.gguf --port 8000

# 2. Jev Local (needs Rust)
cd assets/rreia-jev
RREIA_RAM_GB=16 RREIA_BANDWIDTH_GBS=51.2 RREIA_WEIGHTS_GB=4.9 \
RREIA_KEYS="chave-ops:P0,chave-lote:P3" RREIA_UPSTREAM=http://127.0.0.1:8000 \
cargo run --release
```

Settings (environment variables, defaults in parentheses): `RREIA_RAM_GB` (16), `RREIA_BANDWIDTH_GBS` (51.2), `RREIA_RESERVE_GB` (2), `RREIA_WEIGHTS_GB` (4.9), `RREIA_ACTIVE_GB` (= weights), `RREIA_LAYERS` (32), `RREIA_KV_HEADS` (8), `RREIA_HEAD_DIM` (128), `RREIA_MODEL`, `RREIA_MEM_FRACTION` (0.75), `RREIA_ETA` (0.5), `RREIA_MIN_TOK_S` (5), `RREIA_KEYS`, `RREIA_BIND` (127.0.0.1:8080), `RREIA_UPSTREAM` (http://127.0.0.1:8000), `RREIA_MAX_QUEUE` (64). Exit code 2 means admission refused the model (L1 or L2).

Run `cargo test` to check the build: 20 tests against a simulated model server. It has not been run against a real model unless the developer did so; say which when you report.
