# Live Log Streaming

[日本語版](live-log-streaming.ja.md)

## Background

When a CircleCI step is still running, fetching its log in one shot shows only
the output produced so far.  For long-running steps (deploys, integration tests,
Docker builds) this forces the user to repeatedly re-run the command and re-read
the entire log just to see new output.

Live log streaming solves this by polling the CircleCI private output API
incrementally, printing new bytes as they arrive — similar to `tail -f` for
remote CI logs.

## How it works

```
show_log()
  │
  ├── action.status is "running"?
  │     ├── yes ─→ stream_log()    (live streaming loop)
  │     └── no  ─→ fetch full log  (one-shot, same as before)
  │
  stream_log()
  │
  ├── Print header (workflow, job, step, "streaming...")
  ├── Enable raw mode (RawModeGuard)
  │
  └── loop
        ├── event::poll(50ms)         ← check for Esc / q / Ctrl+C
        ├── fetch_private_output_range(byte_offset)
        │     └── write new bytes to stdout (with CRLF conversion)
        ├── every 5 polls: re-fetch job detail
        │     └── is_step_finished()?
        │           ├── yes ─→ final fetch, print status, break
        │           └── no  ─→ continue
        └── sleep(poll_interval_ms)   ← normally 950ms
```

## Incremental fetch

Each poll calls `fetch_private_output_range` with an HTTP `Range` header to
request only the bytes beyond the current `byte_offset`.

| HTTP status | Meaning                         | `new_offset` calculation           |
|-------------|----------------------------------|-------------------------------------|
| 200         | Full body (Range ignored/first)  | `body.len()`                        |
| 206         | Partial content                  | `byte_offset + body.len()`          |
| 204         | No new output yet                | unchanged                           |
| 416         | Range not satisfiable            | unchanged                           |

### Why not vt100 rendering?

During streaming the raw bytes from the API are written directly to the
terminal.  The vt100 rendering pass (used for completed-log display) resolves
cursor-control sequences to a final screen state, which would destroy the
incremental "append-only" nature of streaming.  By passing raw bytes through,
the user's terminal handles escape sequences natively — including animated
progress indicators like Docker buildx output — exactly as they would in a
real terminal session.

## Polling strategy

| Timer       | Value   | Rationale                                                       |
|-------------|---------|------------------------------------------------------------------|
| Key poll    | 50 ms   | Below human-perceptible latency (~100 ms). Key presses feel instant. |
| Fetch sleep | 950 ms  | 50 ms + 950 ms ≈ 1 second/cycle. Balances real-time feel vs API load. |
| Status check| 5 polls | ~5 seconds. Keeps completion-detection delay within acceptable bounds. |

## Rate limiting

The private output API enforces rate limits and returns HTTP 429 when exceeded.
`fetch_private_output_range` retries up to 3 times, honoring the `Retry-After`
header (defaults to 1 second if the header is missing).

In `stream_log`, transient fetch errors trigger adaptive backoff:

1. First error: `poll_interval_ms` jumps from 950 ms to 3,000 ms
2. Subsequent errors: increases to 5,000 ms
3. After 30 seconds of consecutive errors: streaming stops with a message

When a successful fetch occurs, the interval and error timer reset.

## Raw mode

### What raw mode does

`crossterm::terminal::enable_raw_mode()` reconfigures the terminal driver:

- Disables line buffering — each key press is delivered immediately
- Disables local echo — typed characters are not printed
- Disables signal processing for Ctrl+C (we handle it manually)
- **Disables automatic LF → CRLF translation**

### RawModeGuard (RAII pattern)

`RawModeGuard` wraps `enable_raw_mode()` / `disable_raw_mode()` in a
constructor / `Drop` pair.  This guarantees the terminal is restored even if
the streaming loop exits via `?`, an error, or a panic — without requiring
manual cleanup at every exit point.

### CRLF conversion

With LF → CRLF translation disabled, a bare `\n` moves the cursor down one
line without returning to column 0, producing "staircase" output:

```
line 1
      line 2
            line 3
```

`write_with_crlf` replaces each `\n` (0x0A) with `\r\n` before writing.

This byte-level replacement is safe for ANSI escape sequences: the ECMA-48
standard reserves bytes 0x20–0x7E for parameter and intermediate bytes within
escape sequences, so 0x0A (LF) never appears inside a sequence.

## Edge cases

| Condition                     | Behavior                                        |
|-------------------------------|-------------------------------------------------|
| Step finishes during stream   | Final fetch, print status line, exit loop        |
| 30s of consecutive fetch errors | Print error message, exit loop                 |
| User presses Esc or q         | Exit stream, show post-log menu                  |
| User presses Ctrl+C           | Exit stream and program immediately              |
| `build_num` is `None`         | Falls back to job number 0 (defensive)           |
| Step already finished         | `show_log` takes the one-shot path, not streaming|

## Key bindings

| Key     | Action                                                 |
|---------|--------------------------------------------------------|
| Esc     | Stop streaming, return to post-log menu (Back / Exit)  |
| q       | Same as Esc                                            |
| Ctrl+C  | Stop streaming and exit the program                    |
