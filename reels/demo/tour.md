# Scheduler design notes

Notes on the work queue that sits between the ingest workers and the render
farm. Written down because the same three questions keep being asked in
review, and answering them in a document is cheaper than answering them again.

## The problem

Ingest produces work faster than the farm can retire it, and it does so in
bursts: a customer uploads four hundred documents at nine in the morning and
nothing at all until lunch. A queue that is sized for the average is empty for
most of the day and hopelessly behind for twenty minutes of it, and a queue
sized for the peak costs four times what the work is worth.

So the queue has to absorb the burst without pretending the farm is faster
than it is. That is the whole design. Everything below follows from it.

## The invariant

There is exactly one invariant, and every other rule in this document exists
to keep it:

> A job that has been accepted is either in the queue, in flight, or in the
> dead letter table. Never none of those, and never two of them at once.

The invariant is what lets the ingest workers answer a customer immediately.
Accepting a job is a promise that it will be retired or explained, and the
invariant is that promise written down in a form a test can check.

## Backpressure

Backpressure is the mechanism, and it is deliberately dull. Each worker holds
a lease on a slot. When the slots are gone, ingest blocks rather than
buffering, and the block propagates back to the upload endpoint, which answers
with a retry-after rather than a timeout.

The alternative — an unbounded buffer — moves the failure from a place where
it is visible to a place where it is not. A queue that never says no is a
queue that eventually says nothing at all, and by then the only evidence is a
memory graph. Backpressure makes the limit a number somebody chose.

| Setting | Default | Notes |
| --- | --- | --- |
| Slots per worker | 4 | Raise only with evidence from the farm |
| Lease duration | 30s | Two missed heartbeats |
| Retry ceiling | 5 | Then the dead letter table |
| Batch size | 64 | Larger batches starve small tenants |

## Leases and expiry

A lease is a row, not a lock. The worker writes its identity and a deadline;
the sweeper reclaims anything past its deadline. This means a worker that dies
mid-job costs one lease duration and nothing else, and it means the sweeper
needs no knowledge of workers at all.

```rust
pub fn claim(&self, worker: WorkerId, now: Instant) -> Option<Lease> {
    let slot = self.free.pop()?;
    Some(Lease {
        slot,
        worker,
        deadline: now + self.lease_duration,
    })
}
```

Expiry is checked on read rather than on a timer. A timer would need to be
right about the clock; a read already has one.

## Fairness

Round-robin across tenants, not first-in-first-out. The 9 a.m. burst is one
tenant, and a strict queue would let it hold the farm for twenty minutes while
every other customer waits behind it.

The cost of fairness is that the burst finishes later than it would have. That
is the correct trade: the tenant who uploaded four hundred documents is not
watching any single one of them, and the tenant who uploaded one is.

- [x] Round-robin selection
- [x] Per-tenant slot ceiling
- [ ] Weighted shares for paid tiers
- [ ] Admission control at ingest

## Retries

Retries are bounded and they back off. The ceiling is five, and the interval
is $t_n = 2^n \cdot 500\,\text{ms}$ with jitter, which puts the last attempt
about eight seconds after the first.

Jitter is not decoration. Without it, a farm that drops fifty jobs at once
retries all fifty at the same instant, and the retry is indistinguishable from
the burst that caused the failure.

## The dead letter table

Anything past the retry ceiling lands in the dead letter table with the last
error attached. Nothing is deleted, and nothing is retried automatically out of
it — a job that failed five times has a reason, and the reason is usually not
going to change on its own.

```bash
# What is stuck, and why
scheduler dead-letters --since 24h --group-by reason
```

## Metrics worth having

Three, and no more than three on the dashboard:

1. **Queue depth**, which says whether the burst is being absorbed.
2. **Oldest in-flight lease**, which says whether a worker has wandered off.
3. **Dead letters per hour**, which says whether the farm is actually broken.

Everything else is a query, not a graph. A dashboard with thirty panels is a
dashboard nobody reads, and the invariant above is not visible on any of them.

## Open questions

Whether admission control belongs at ingest or at the queue. Putting it at
ingest means the customer hears about it sooner, which is kinder; putting it
at the queue means there is one place that knows the real limit, which is
correct. These are not the same thing and the document does not resolve them.
