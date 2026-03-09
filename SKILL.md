---
name: circleci-logs
description: Fetch CircleCI job logs, test results, workflow status, and pipeline info from the CLI. Use when CI fails, when you need to read build logs, check test failures, list workflow jobs, or inspect pipeline status. Accepts job numbers, workflow UUIDs, pipeline numbers, or CircleCI URLs directly.
---

## Prerequisites

- `CIRCLE_TOKEN` environment variable must be set
- Run inside a git repository (project auto-detected from remote URL)

## Quick Reference

> **WARNING**: Job logs can be thousands of lines. **Always** use `--errors-only` or `--grep` with `-j` to avoid flooding the context window. Never fetch full logs without filtering.

Always use `--json` for machine-readable output.

| Goal                          | Command                                                |
|-------------------------------|--------------------------------------------------------|
| View failed steps in a job    | `circleci-logs -j JOB --errors-only --json`            |
| Search job logs               | `circleci-logs -j JOB --grep "PATTERN" --json`         |
| Get test results              | `circleci-logs -j JOB --tests --json`                  |
| Get failed tests only         | `circleci-logs -j JOB --tests --failed-only --json`    |
| List jobs in a workflow       | `circleci-logs -w WORKFLOW_UUID --json`                 |
| List workflows in a pipeline  | `circleci-logs -p PIPELINE_NUMBER --json`               |
| Use a CircleCI URL directly   | `circleci-logs "https://app.circleci.com/..." --json`  |

## Flag Semantics

- `--errors-only` — **Step-level filter**. Returns only failed steps and their logs. The `steps` array contains only failed steps (empty array = all steps passed).
- `--grep "PATTERN"` — **Line-level filter**. Fetches all steps but returns only matching log lines. Supports regex.
- These two flags are **mutually exclusive**. Use `--errors-only` first; use `--grep` when you need a specific pattern across all steps.

## Investigate a CI Failure

**If you have a CircleCI URL** — use it directly (skip drill-down):

```bash
circleci-logs "URL" --errors-only --json
```

**If you only have a pipeline number** — drill down:

```bash
# 1. Pipeline -> workflows
circleci-logs -p PIPELINE_NUMBER --json
# 2. Find failed workflow UUID -> jobs
circleci-logs -w "UUID" --json
# 3. Find failed job number -> error logs
circleci-logs -j JOB --errors-only --json
# 4. (Optional) Search for specific patterns
circleci-logs -j JOB --grep "error|panic|FAILED" --json
```

## Check Test Results

```bash
# Failed tests only (most useful)
circleci-logs -j JOB --tests --failed-only --json
# All test results
circleci-logs -j JOB --tests --json
```

Note: The job must use CircleCI's `store_test_results` step.

## JSON Schemas

### Job logs (`-j JOB --json`)

`{"build_num": 456, "status": "failed", "steps": [{"name": "Run tests", "actions": [{"name": "Run tests", "status": "failed", "run_time_millis": 15000}]}], "logs": [{"step": "Run tests", "output": "..."}]}`

Fields: `build_num` (number|null), `status` (string|null), `steps` (array|null), `logs` (array).
With `--errors-only`: `steps` contains only failed steps (empty array = all passed); `logs` filtered to match.
Action `status` values: `"success"`, `"failed"`, `"timedout"`, `"infrastructure_fail"`, `"canceled"`, `"running"`.

### Workflow jobs (`-w UUID --json`)

`[{"id": "job-uuid", "name": "build", "status": "success", "job_number": 456, "type": "build", "started_at": "2025-01-15T10:00:00Z", "stopped_at": "2025-01-15T10:02:30Z"}]`

Fields: `id` (string), `name` (string), `status` (string), `job_number` (number|null), `type` (string|null), `started_at` (string|null), `stopped_at` (string|null).

### Pipeline workflows (`-p NUMBER --json`)

`[{"id": "workflow-uuid", "name": "build-and-test", "status": "failed", "created_at": "2025-01-15T10:00:00Z", "stopped_at": "2025-01-15T10:05:00Z", "pipeline_number": 142}]`

Fields: `id` (string), `name` (string), `status` (string), `created_at` (string|null), `stopped_at` (string|null), `pipeline_number` (number|null).

### Test results (`-j JOB --tests --json`)

`[{"name": "test_login", "classname": "AuthSpec", "result": "failure", "message": "Expected true got false", "run_time": 0.437, "source": "rspec", "file": "spec/auth_spec.rb"}]`

Fields: all optional (string|null except `run_time` number|null). `result` values: `"success"`, `"failure"`, `"skipped"`.

## Exit Codes

- Default: always exits 0
- With `--fail-on-error`: exits 1 when the job status is not `"success"` (logs mode) or when any test has `"failure"`/`"failed"` result (test mode)

## Constraints

- `-j`, `-w`, `-p` are mutually exclusive
- `--errors-only`, `--grep`, `--fail-on-error`, `--tests` require `-j`
- `--failed-only` requires `--tests`
- `--tests` cannot be combined with `--errors-only` or `--grep`
- URL cannot be combined with `-j`, `-w`, or `-p`
