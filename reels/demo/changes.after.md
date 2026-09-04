# Admission control

A proposal for turning ingest away before the queue is full, rather than after.

## Why

Backpressure works, but it works late. By the time the slots are gone the
customer has already paid for the upload — the bytes are across the wire, the
document is parsed, and the only thing left to tell them is no. Rejecting at
ingest costs them one round trip instead of a whole upload, and it costs us
nothing we were going to keep.

## The rule

Reject at ingest when the projected depth exceeds the ceiling.

| Setting | Default |
| --- | --- |
| Projection window | 60s |
| Depth ceiling | 2000 |
| Reject status | 429 |

## Projection

```rust
fn projected(depth: usize, rate: f64, window: Duration) -> f64 {
    // Rate is measured over the same window it projects, so a burst that
    // started ten seconds ago is already in it.
    depth as f64 + rate.max(0.0) * window.as_secs_f64()
}
```

## Fairness

A global ceiling lets one tenant's burst reject another tenant's single
document, which is the failure round-robin exists to prevent. The ceiling is
per-tenant, and the global limit stays where it is: in the slots.

## Open questions

None outstanding.
