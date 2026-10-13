# Local first, paid API on request

## Default policy (L8)

1. Use a free local model whenever L1 to L3 accept it for the task.
2. Go remote only when one of these holds: admission fails (the model does not fit or is too slow), the task needs more capability than the local tier offers, or the developer asked for remote.
3. Keep both behind one client interface so switching is a configuration change, not a rewrite.

## Questions to ask the developer

- Local only, local first with a paid fallback, or paid first?
- Which provider and model, if any?
- Monthly spending cap? What happens when it is reached (fall back to local, queue, or refuse)?
- May the data leave the device? If it contains personal data, which legal basis applies (LGPD / GDPR)? Do not send personal data to a remote API unless the developer has confirmed this.
- Is the task latency-sensitive? A remote call adds network latency and fails when offline.

## What to implement

- A `Router` that takes the task, its class, and the policy and returns `Local` or `Remote(provider)`, with the reason recorded in the audit log.
- Budget tracking (L9): tokens in and out per call, price per token from configuration (prices change, so never hard-code them), running total per period, hard stop at the cap.
- Keys in environment variables or a secret store; never in source or logs.
- Retries with a limit and a timeout per class; a remote outage degrades to local (L5).
- The audit record states which route served each request and the cost, but not the content.

## What to tell the developer

Local models cost no per-request money but cost hardware, electricity and engineering time; remote models cost per token and add privacy and availability considerations. Present these as trade-offs, not as a verdict, and let the developer decide.
