# JSON output schema (`--output json`)

Holdon emits **newline-delimited JSON** to stdout when invoked with
`--output json`. Each line is a single self-contained event object.

## Stability

- Every event carries `v`, the schema version.
- For a given `v`, field semantics are frozen for the lifetime of that
  version. New fields may be added; consumers MUST ignore unknown fields.
- Breaking changes bump `v` and ship in a new major release.

Current version: **`1`**.

## Common fields

Every event has:

| Field         | Type     | Description                                   |
| ------------- | -------- | --------------------------------------------- |
| `v`           | integer  | Schema version (`1`).                         |
| `ts_unix_ms`  | integer  | Event timestamp, Unix epoch milliseconds, UTC.|
| `event`       | string   | Event discriminator (see below).              |

## Event: `start`

Emitted once at run start.

```json
{ "v": 1, "ts_unix_ms": 1715300000000, "event": "start",
  "targets": ["tcp://db:5432", "https://api.local/health"] }
```

## Event: `attempt`

Emitted after every probe attempt (success or failure).

| Field        | Type    | Description                                  |
| ------------ | ------- | -------------------------------------------- |
| `target`     | string  | Target URL (passwords redacted).             |
| `attempt`    | integer | 1-based attempt counter.                     |
| `latency_ms` | integer | Wall time of this attempt, milliseconds.     |
| `ready`      | boolean | Whether this attempt's outcome was ready.    |

## Event: `target`

Emitted once per target at the end of a run.

| Field         | Type     | Description                                   |
| ------------- | -------- | --------------------------------------------- |
| `target`      | string   | Target URL.                                   |
| `satisfied`   | boolean  | Whether the readiness condition was met.      |
| `ready`       | boolean  | Whether the final probe was ready.            |
| `attempts`    | integer  | Total attempts made.                          |
| `elapsed_ms`  | integer  | Total wall time for this target.              |
| `stages`      | array    | Ordered list of stages (DNS, TCP, …).         |

Each `stages[i]` object:

| Field      | Type             | Description                              |
| ---------- | ---------------- | ---------------------------------------- |
| `kind`     | string           | `dns`, `tcp`, `http`, `file`, `postgres`, `redis`, `mysql`, `exec`. |
| `status`   | string           | `"ok"` or `"err"`.                       |
| `took_ms`  | integer          | Stage wall time, milliseconds.           |
| `message`  | string \| null   | Error message (only when `status=err`).  |
| `hint`     | string \| null   | Operator-facing fix hint, if available.  |

## Event: `end`

Emitted once after the last `target` event.

| Field             | Type      | Description                                |
| ----------------- | --------- | ------------------------------------------ |
| `ready`           | boolean   | All targets satisfied.                     |
| `elapsed_ms`      | integer   | Total wall time of the run.                |
| `total`           | integer   | Target count.                              |
| `ready_targets`   | string[]  | URLs satisfied.                            |
| `failed_targets`  | string[]  | URLs not satisfied.                        |

## Exit codes

| Code | Meaning                                            |
| ---- | -------------------------------------------------- |
| 0    | Ready (all targets satisfied).                     |
| 2    | CLI misuse / parse error.                          |
| 124  | Overall timeout elapsed before readiness.          |
| 126  | Exec'd child existed but was not executable.       |
| 127  | Exec'd child binary not found.                     |
| 130  | Interrupted by SIGINT (Ctrl-C).                    |
| 143  | Interrupted by SIGTERM.                            |
