# RREIA.md template

Create this file at the project root after steps 1 to 4 and keep it updated.

```markdown
# RREIA record

## Device
- Type / model:
- OS:
- Memory (RAM / VRAM / unified):
- Memory type and bandwidth (estimated or measured):
- GPU / accelerator:
- Tier guess (R0 to R3):
- Offline required: yes / no
- Concurrency and latency target:

## Model and cost policy
- Local model and quantization:
- Source of weights and license:
- Remote fallback: none / provider + model
- Monthly cap and behavior at the cap:
- May data leave the device: yes / no

## Compatibility check
- L1 memory budget (GB):
- L2 speed floor (tok/s) and task minimum:
- L3 maximum context (tokens):
- Verdict:
- Numbers that are estimates:
- Measured on the real device: not yet / tok/s, time to first token

## Actions, principals, permissions
| Action | Reversibility | Who may request | Max class | Needs human approval |
|---|---|---|---|---|

## Laws enforced in code
(list each law and the file and test that enforce it)

## Audit trail
- Where records go, what is logged, what is deliberately not logged:

## Not yet tested
-
```
