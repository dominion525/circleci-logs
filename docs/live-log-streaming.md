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
        │     └── vt100 parser → rows_formatted → print new rows
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

### vt100 incremental rendering

Streaming uses a `vt100::Parser` with a very large virtual screen (`u16::MAX`
rows) to process incoming bytes.  Each chunk is fed into the parser via
`parser.process(data)`, then `render_vt100_rows` prints only the new rows
(from `last_printed_row` to `cursor_position().0`) using `rows_formatted()`.

Because the virtual screen has effectively unlimited rows, content never
scrolls off the top.  The `last_printed_row` tracker ensures each row is
printed to the real terminal exactly once.  This means output goes directly
into the terminal's normal scrollback buffer — the user can scroll up during
streaming, just like `tail -f`.

This approach correctly handles cursor-control sequences used by tools like
Docker BuildKit (cursor-up, erase-line, carriage-return-based progress bars).
Without vt100 rendering, these sequences would pass through raw and produce
corrupted output — e.g., 91 lines of expected output could balloon to 14,000+
lines.

The terminal width is obtained via `crossterm::terminal::size()` (with an
`(80, 24)` fallback) so the virtual screen columns match the real terminal.

NUL bytes (`\x00`), which appear in some CI log data, are filtered both at
input (before feeding to the parser) and at output (from `rows_formatted`).

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
