# Admission control

A proposal for turning ingest away before the queue is full, rather than after.

## Why

Backpressure works, but it works late. By the time the slots are gone the
customer has already waited for the upload to finish, and the retry-after
lands on a request that has already cost them time.

## The rule

Reject at ingest when the projected depth exceeds the ceiling.

| Setting | Default |
| --- | --- |
| Projection window | 30s |
| Depth ceiling | 2000 |
| Reject status | 429 |

## Projection

```rust
fn projected(depth: usize, rate: f64, window: Duration) -> f64 {
    depth as f64 + rate * window.as_secs_f64()
}
```

## Open questions

Whether the ceiling is per-tenant or global.
