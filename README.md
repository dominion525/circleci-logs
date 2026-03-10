# circleci-logs

**[日本語版 (Japanese)](README.ja.md)**

A CLI tool for fetching CircleCI job logs, workflow info, and pipeline info from the command line.

## Quick Start

Install:

```
cargo install circleci-logs
```

Fetch job logs (passing the token inline):

```
CIRCLE_TOKEN=xxx circleci-logs -j 12345
```

For repeated use, export the token as an environment variable:

```
export CIRCLE_TOKEN="your-circleci-token"
circleci-logs -j 12345
```

Run inside a GitHub or Bitbucket git repository and the project is auto-detected from the remote URL.
`CIRCLE_TOKEN` can be created at [CircleCI Personal API Tokens](https://app.circleci.com/settings/user/tokens).

## Usage

CircleCI organizes builds in the following hierarchy:

```
Pipeline (123)         Triggered by a git push or scheduled run
 └─ Workflow (uuid)    Defines job execution order and dependencies
     └─ Job (456)      A set of steps running in an execution environment
         └─ Step       An individual command execution. Logs live here
```

This tool provides three modes corresponding to each level. Specify one at a time:

| What you want               | Command                          | Where to find the ID (CircleCI Web UI)  |
|-----------------------------|----------------------------------|-----------------------------------------|
| Job logs                    | `circleci-logs -j <number>`      | `.../jobs/456` at end of URL            |
| Jobs in a workflow          | `circleci-logs -w <UUID>`        | `.../workflows/<UUID>` in URL           |
| Workflows in a pipeline     | `circleci-logs -p <number>`      | `.../pipelines/.../123` at end of URL   |

You can also pass a CircleCI Web UI URL directly. The mode is auto-detected by URL depth:

```
# URL ends with /jobs/N → show job logs
circleci-logs "https://app.circleci.com/pipelines/github/org/repo/123/workflows/UUID/jobs/456"

# URL ends with /workflows/UUID → list jobs
circleci-logs "https://app.circleci.com/pipelines/github/org/repo/123/workflows/UUID"

# URL ends with pipeline number → list workflows
circleci-logs "https://app.circleci.com/pipelines/github/org/repo/123"
```

If the URL contains a different project than the one detected from config or git remote, the URL takes precedence (with a warning). Options like `--tests`, `--json`, etc. work with URLs too.

See all options with `circleci-logs --help`.

### Job Logs (`-j` / `--jid`)

Specify a job number to display its steps and logs.

```
circleci-logs -j <JOB_NUMBER>
```

Example output:

```
$ circleci-logs -j 4504
Workflow: build-and-test  Job: test
Status: failed

[success] Spin up environment (2s)
[success] Checkout code (1s)
[failed]  Run tests (15s)

--- Run tests ---
FAILED src/app.test.ts:42
  Expected: 200
  Received: 500
```

Options:

- `--errors-only` — Show only failed steps
  ```
  circleci-logs -j 4504 --errors-only
  ```
- `--grep <PATTERN>` — Filter log lines by regex
  ```
  circleci-logs -j 4504 --grep "error"
  ```
- `--json` — Output in JSON format
- `--fail-on-error` — Exit with code 1 if the job has errors
  ```
  circleci-logs -j 4504 --fail-on-error
  ```
- `--tests` — Show test results (for jobs using `store_test_results`)
  ```
  circleci-logs -j 4504 --tests
  ```
- `--failed-only` — Show only failed tests (use with `--tests`)
  ```
  circleci-logs -j 4504 --tests --failed-only
  ```

`--tests` example output:

```
$ circleci-logs -j 4504 --tests
Test Results: Job #4504

RESULT     TIME       FILE                           NAME
------------------------------------------------------------------------------------------
success    0.437s     tests/user.rb                  User.create validates email
failure    0.052s     tests/auth.rb                  Auth.login rejects invalid token
skipped    -          tests/legacy.rb                Legacy.import is deprecated

--- Failed Tests ---

[failure] Auth.login rejects invalid token
  File:  tests/auth.rb
  Class: spec.auth

  Expected true but got false
  at line 42 in auth_spec.rb

Summary: 1 passed, 1 failed, 1 skipped (0.489s)
```

`--tests` is mutually exclusive with `--errors-only` and `--grep`.

### Workflow Jobs (`-w` / `--wid`)

Specify a workflow ID to list its jobs.

```
circleci-logs -w <WORKFLOW_ID>
```

Example output:

```
$ circleci-logs -w a1b2c3d4-5678-90ab-cdef-1234567890ab
JOB#     NAME                           STATUS       STARTED              STOPPED
------------------------------------------------------------------------------------------
4501     lint                           success      2025-01-15 19:00:05  2025-01-15 19:00:38
4502     build                          success      2025-01-15 19:00:06  2025-01-15 19:00:30
4503     unit-test                      success      2025-01-15 19:00:32  2025-01-15 19:04:15
4504     integration-test               failed       2025-01-15 19:04:18  2025-01-15 19:08:42
4505     deploy                         canceled     -                    -
```

Options:

- `--json` — Output in JSON format

### Interactive Mode (`-i` / `--interactive`)

A TUI mode for drilling down through Pipeline → Workflow → Job → Step → Log. No need to look up numbers or UUIDs — just select from the list.

```
circleci-logs -i
```

#### Pipeline → Workflow → Job

```
? Select a pipeline
> #1042     main                           created      2025-03-08 14:32:01
  #1041     feat/new-feature               created      2025-03-08 13:15:42
  #1040     main                           created      2025-03-07 22:08:11

? Select a job
  .. (back to workflows)
  #5678     rspec                success      2025-03-08 14:32:10
  #5679     lint                 success      2025-03-08 14:32:08
> #5680     e2e                  failed       2025-03-08 14:33:01
  .. (back to workflows)
```

#### Parallel Job Node Selection

For parallel jobs (`parallelism > 1`), a node list is displayed similar to the CircleCI Web UI. Each node shows its aggregated status and total run time.

```
? Select a node
  .. (back to jobs)
> node 0    [success] 2m30s
  node 1    [failed ] 1m45s
  node 2    [success] 2m12s
  .. (back to jobs)
```

Selecting a node shows its steps:

```
? Select a step
  .. (back to nodes)
  [success] Spin up environment              5s
  [success] Checkout code                    2s
> [failed ] RSpec                            1m38s
  [success] Upload results                   3s
  .. (back to nodes)
```

For non-parallel jobs, node selection is skipped and steps are shown directly.

#### Log View

Selecting a step displays its log output.

```
Node: 1  Step: RSpec
  [failed ] (1m38s)

FAILED spec/models/user_spec.rb:42
  Expected: 200
  Received: 500

? Log view
> Back to steps
  Exit
```

#### Controls

- Up/Down arrow keys to navigate, Enter to select
- Each list has a back option at top and bottom (e.g. `.. (back to jobs)`)
- `▼ Load more...` to fetch additional data
- Esc to exit

#### Starting from a URL

Pass a URL to start from a specific level:

```
circleci-logs -i "https://app.circleci.com/pipelines/github/org/repo/123"
circleci-logs -i "https://app.circleci.com/pipelines/github/org/repo/123/workflows/UUID"
```

Notes:
- `-i` is mutually exclusive with `-j`/`-w`/`-p` and `--json`/`--errors-only`/`--grep`/`--tests`/`--failed-only`/`--fail-on-error`
- Requires a terminal (TTY). Does not work with piped input

### Pipeline Workflows (`-p` / `--pid`)

Specify a pipeline number to list its workflows.

```
circleci-logs -p <PIPELINE_NUMBER>
```

Example output:

```
$ circleci-logs -p 142
WORKFLOW ID                            NAME                      STATUS       CREATED              STOPPED
-------------------------------------------------------------------------------------------------------------------
a1b2c3d4-5678-90ab-cdef-1234567890ab   build-and-test            failed       2025-01-15 19:00:01  2025-01-15 19:08:42
b2c3d4e5-6789-01bc-defa-234567890abc   deploy                    canceled     2025-01-15 19:00:01  2025-01-15 19:08:45
```

Options:

- `--json` — Output in JSON format

## Project Resolution

The project (`vcs_type/org/repo`) is resolved in the following order:

1. `project` field in the config file
2. Auto-detected from `git remote get-url origin` (GitHub / Bitbucket)
3. Error if neither is available

In most cases, simply running inside a GitHub/Bitbucket repository is enough — no explicit configuration needed.

## Config File (Optional)

Create `.circleci-logs.toml` in your project root to explicitly set the project or token:

```toml
project = "github/your-org/your-repo"   # optional (auto-detected from git remote)
token = "your-circleci-token"            # optional (environment variable recommended)
```

### Token Priority

1. `CIRCLE_TOKEN` environment variable
2. `token` field in config file

If neither is set, an error is returned.

### Config File Discovery

The tool searches for `.circleci-logs.toml` from the current directory upward. The first file found is used.

```
/home/user/projects/myapp/src/   ← running here
/home/user/projects/myapp/       ← .circleci-logs.toml found here, used
/home/user/projects/
/home/user/
...
```

This means a single config file at the repository root works from any subdirectory.

### Permissions

If you store a token in the config file, restrict permissions and add it to `.gitignore`:

```
chmod 600 .circleci-logs.toml
echo '.circleci-logs.toml' >> .gitignore
```

## Log Rendering

Log output from CircleCI contains terminal escape sequences (ANSI colors, cursor control). The CLI processes these through a vt100 terminal emulator to produce clean output — matching what the CircleCI Web UI displays. When writing to a terminal, colors are preserved; when piped or redirected, plain text is emitted. See [docs/log-rendering.md](docs/log-rendering.md) for technical details.

## AI Agent Integration

Register as a skill for AI agents like Claude Code:

```bash
mkdir -p ~/.claude/skills/circleci-logs && circleci-logs --help-skill > ~/.claude/skills/circleci-logs/SKILL.md
```

## License

[MIT](LICENSE)
