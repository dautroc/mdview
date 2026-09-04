# Lease protocol

The wire format between a worker and the queue. One table per message, because
the messages are the protocol and prose about them goes stale.

## Claim

| Field | Type | Notes |
| --- | --- | --- |
| worker | uuid | Stable for the process lifetime |
| slots | u8 | At most the per-worker ceiling |
| tenant | string? | Absent means any |

## Heartbeat

| Field | Type | Notes |
| --- | --- | --- |
| lease | uuid | From the claim |
| deadline | instant | Advisory; the queue decides |

## Release

| Field | Type | Notes |
| --- | --- | --- |
| lease | uuid | From the claim |
| outcome | enum | `ok`, `failed`, `abandoned` |
| error | string? | Required when `failed` |

## Ordering

A heartbeat after a release is not an error; it is a race, and the queue
answers it with the release it already has. A claim after a release with the
same lease id is an error, and the queue says so.
