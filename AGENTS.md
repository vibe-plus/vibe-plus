<!--VITE PLUS START-->

# Using Vite+, the Unified Toolchain for the Web

This project is using Vite+, a unified toolchain built on top of Vite, Rolldown, Vitest, tsdown, Oxlint, Oxfmt, and Vite Task. Vite+ wraps runtime management, package management, and frontend tooling in a single global CLI called `vp`. Vite+ is distinct from Vite, and it invokes Vite through `vp dev` and `vp build`. Run `vp help` to print a list of commands and `vp <command> --help` for information about a specific command.

Docs are local at `node_modules/vite-plus/docs` or online at https://viteplus.dev/guide/.

## Review Checklist

- [ ] Run `vp install` after pulling remote changes and before getting started.
- [ ] Run `vp check` and `vp test` to format, lint, type check and test changes.
- [ ] Check if there are `vite.config.ts` tasks or `package.json` scripts necessary for validation, run via `vp run <script>`.

<!--VITE PLUS END-->

---

# Codex Protocol — Terminology & Invariants

> Source of truth derived from real traffic captured in `~/.vibe/vibe.db`.
> Use these terms exactly in code, comments, and variable names.

## Hierarchy

```
Session
└── Thread (1..N per session)
    └── Turn  (1..N per thread)
        └── Request (1..N per turn)
```

### Session

- Lifetime: one open Codex Desktop window.
- Identifier: `session_id` header (UUIDv7). **Never changes within a window.**
- Normal case: `session_id == thread_id` (main user thread).

### Thread

- Identifier: `thread_id` header (UUIDv7).
- `thread_source` from `x-codex-turn-metadata`:
  - `"user"` — the main user-facing conversation.
  - `"subagent"` — a parallel agent spawned by the model. Gets a **new, distinct `thread_id`** but keeps the same `session_id` as its parent.
  - `null/absent` — legacy or non-Codex-Desktop clients.
- Multiple subagent threads can run **in parallel** under a single session.
- Re-edits and rollbacks do **not** reuse a thread; they produce a new `thread_id`.

### Turn

- Identifier: `turn_id` inside `x-codex-turn-metadata` JSON.
- **Changes on every user message.** One turn = one complete user-message → model-reply cycle.
- A single turn can contain many requests (tool-call loop).

### Request

- One HTTP/WebSocket round-trip to an upstream model API.
- Corresponds to one row in `request_logs`.
- Within a turn, the model may send multiple requests:
  each tool call result triggers a new request with a growing context.

## Token Accounting per Turn

Within a single turn the upstream model is called N times (once per tool call loop).
Each `response.completed` event carries **that request's** token counts:

| Field                   | Per-request semantics                                                           | Correct turn aggregate                            |
| ----------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------- |
| `input_tokens`          | All tokens sent (prompt + full history + tool results so far). Grows each call. | **MAX** — shows final context window size.        |
| `output_tokens`         | Tokens generated in **this response only**.                                     | **SUM** — total generation across all tool loops. |
| `cache_read_tokens`     | Cache hit tokens for this request.                                              | **MAX** (monotonically non-decreasing).           |
| `cache_creation_tokens` | New cache entries created.                                                      | **SUM** — total cache writes this turn.           |

## USD Cost Formula

### Turn cost (displayed at turn end)

```
turn_cost = (SUM(input_tokens across all N requests) × input_price
           + SUM(output_tokens across all N requests) × output_price)
           / 1_000_000
```

Rationale: you are billed for every API call including repeated context.

### Thread cost (cumulative, displayed alongside turn cost)

```
thread_cost = SUM(turn_cost for every completed turn in this thread)
```

Resets when the thread is garbage-collected from AppState (TTL: 30 min).
Subagent threads track their own thread cost independently.

## Header Reference

| Header                      | Where   | Meaning                                                                                    |
| --------------------------- | ------- | ------------------------------------------------------------------------------------------ |
| `session_id` / `session-id` | request | Session UUID, stable for window lifetime                                                   |
| `thread_id` / `thread-id`   | request | Thread UUID; ≠ session_id for subagents                                                    |
| `x-codex-turn-metadata`     | request | JSON with `session_id`, `thread_id`, `turn_id`, `thread_source`, `turn_started_at_unix_ms` |
| `x-codex-window-id`         | request | `<session_id>:<window_index>`                                                              |
| `x-client-request-id`       | request | Dedup ID for this HTTP call (often = session_id)                                           |
| `chatgpt-account-id`        | request | ChatGPT account UUID for OAuth flows                                                       |
