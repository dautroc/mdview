# Service architecture

The scheduler follows the [retry policy](links-target.md#retry-policy) when a request fails.

The retired [legacy queue](missing-queue.md) is intentionally absent so the broken-link state is visible.

## Dispatch

Requests enter a bounded queue before a worker claims them.
