# Log Rendering

## Background

CircleCI job logs contain terminal escape sequences — both ANSI color codes and
cursor control sequences. These come from CI tools that produce rich terminal
output (e.g. Docker buildx progress indicators, colored test output).

Without processing, these raw sequences cause problems:

- Redirecting to a file leaves raw escape bytes in the output
- JSON output contains embedded escape sequences
- Cursor-up sequences (`\e[1A`) can overwrite the CLI's own header lines

## Escape sequence types found in CircleCI logs

| Type              | Example          | Purpose                       |
|-------------------|------------------|-------------------------------|
| SGR (color)       | `\e[34m`, `\e[0m`| Blue text, reset attributes   |
| Cursor up         | `\e[1A`          | Move cursor up N lines        |
| Erase line        | `\e[2K`          | Clear entire current line     |
| Cursor column     | `\e[0G`          | Move cursor to column 0       |
| Cursor visibility | `\e[?25l/h`      | Hide/show cursor              |

Docker buildx uses cursor-up + erase-line to create animated progress displays.
In a terminal this looks like a single updating block; in raw bytes it produces
thousands of overwrite cycles.

## Data sources

Both `output_url` (S3 signed URL from v1.1 API) and the private raw output API
return data with escape sequences. The difference is sampling granularity:

- **Private API**: raw byte stream as written to the terminal — every update
  captured (e.g. 14,000 lines for a Docker build step)
- **output_url**: same stream batched into chunks — fewer intermediate states
  (e.g. 4,300 lines for the same step)

Both produce the same final rendered result.

## Rendering approach

We use the [vt100](https://crates.io/crates/vt100) crate to emulate a terminal.
The parser processes the raw byte stream and resolves all cursor movements,
producing the final screen state — equivalent to what the CircleCI Web UI shows.

```rust
let mut parser = vt100::Parser::new(u16::MAX, 200, 0);
parser.process(raw_bytes);

// Plain text (no escape sequences)
let plain = parser.screen().contents();

// Text with ANSI color codes preserved
let colored = parser.screen().contents_formatted();
```

Key design choice: we use `u16::MAX` (65,535) rows as the terminal height.
This is the screen height, not input size — the parser can process arbitrarily
large input. This ensures cursor-overwrite operations stay within the screen
bounds and resolve correctly.

## Output-destination-aware formatting

The CLI detects whether stdout is a terminal (`IsTerminal` trait):

| Destination        | Method                  | Result                    |
|--------------------|-------------------------|---------------------------|
| Terminal (tty)     | `contents_formatted()`  | Colors preserved, clean   |
| File / pipe / JSON | `contents()`            | Plain text, no escapes    |

This matches the behavior of standard CLI tools like `ls` and `grep --color=auto`.

## Comparison with CircleCI Web UI

For a Docker build step (job #61123, step 106):

| Source           | Raw lines | After vt100 rendering |
|------------------|-----------|-----------------------|
| Private API      | 14,127    | 91                    |
| output_url       | 4,360     | 91                    |
| CircleCI Web UI  | —         | 91                    |

The vt100 rendering produces output identical to the CircleCI Web UI.
