# Scheduler design

The scheduler owns retry timing and worker admission.

## Retry policy

Retry requests with bounded exponential backoff. Stop after five attempts and keep the final error with the job.

### Jitter

Add jitter before scheduling each retry so workers do not wake together.

## Capacity

Admission remains bounded independently of retry timing.
