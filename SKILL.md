---
name: circleci-logs
description: Fetch CircleCI job logs, test results, workflow status, and pipeline info from the CLI. Use when CI fails, when you need to read build logs, check test failures, list workflow jobs, or inspect pipeline status. Accepts job numbers, workflow UUIDs, pipeline numbers, or CircleCI URLs directly.
---

## Prerequisites

- `CIRCLE_TOKEN` environment variable must be set
- Run inside a git repository (project auto-detected from remote URL)
- Optional: `.circleci-logs.toml` in repo root to override project settings

## Quick Reference

Always use `--json` for machine-readable output.

| Goal                          | Command                                                |
|-------------------------------|--------------------------------------------------------|
| View failed steps in a job    | `circleci-logs -j JOB --errors-only --json`            |
| Search job logs               | `circleci-logs -j JOB --grep "PATTERN" --json`         |
| Get test results              | `circleci-logs -j JOB --tests --json`                  |
| Get failed tests only         | `circleci-logs -j JOB --tests --failed-only --json`    |
| List jobs in a workflow       | `circleci-logs -w WORKFLOW_UUID --json`                 |
| List workflows in a pipeline  | `circleci-logs -p PIPELINE_NUMBER --json`               |
| Use a CircleCI URL directly   | `circleci-logs "https://app.circleci.com/..." --json`   |

## Investigate a CI Failure

Typical drill-down flow:

```bash
# 1. Start from pipeline number — list workflows
circleci-logs -p 142 --json

# 2. Find the failed workflow UUID → list its jobs
circleci-logs -w "UUID" --json

# 3. Find the failed job number → view error logs
circleci-logs -j 456 --errors-only --json

# 4. Search for specific patterns in the full log
circleci-logs -j 456 --grep "error|panic|FAILED" --json
```

If you have a CircleCI URL, skip the drill-down:

```bash
# URL with /jobs/N → shows job logs directly
circleci-logs "https://app.circleci.com/pipelines/github/org/repo/142/workflows/UUID/jobs/456" --errors-only --json
```

## Check Test Results

```bash
# All test results
circleci-logs -j JOB --tests --json

# Failed tests only (most useful)
circleci-logs -j JOB --tests --failed-only --json
```

Note: The job must use CircleCI's `store_test_results` step.

## Use CircleCI URLs Directly

Pass any CircleCI URL as a positional argument. The mode is auto-detected by URL depth:

| URL ends with            | Mode                    |
|--------------------------|-------------------------|
| `/jobs/N`                | Show job logs           |
| `/workflows/UUID`        | List jobs in workflow   |
| `/pipelines/.../NUMBER`  | List workflows          |

```bash
circleci-logs "https://app.circleci.com/pipelines/github/org/repo/142/workflows/UUID/jobs/456" --json
```

## JSON Output Schemas

### Job logs (`-j JOB --json`)

```json
{
  "build_num": 456,
  "status": "failed",
  "steps": [
    {
      "name": "Run tests",
      "actions": [
        {
          "name": "Run tests",
          "status": "failed",
          "run_time_millis": 15000
        }
      ]
    }
  ],
  "logs": [
    {
      "step": "Run tests",
      "output": "... full log text ..."
    }
  ]
}
```

Fields: `build_num` (number|null), `status` (string|null), `steps` (array|null), `logs` (array).
Action status values: `"success"`, `"failed"`, `"timedout"`, `"infrastructure_fail"`, `"canceled"`, `"running"`.

### Workflow jobs (`-w UUID --json`)

```json
[
  {
    "id": "job-uuid",
    "name": "build",
    "status": "success",
    "job_number": 456,
    "type": "build",
    "started_at": "2025-01-15T10:00:00Z",
    "stopped_at": "2025-01-15T10:02:30Z"
  }
]
```

Fields: `id` (string), `name` (string), `status` (string), `job_number` (number|null), `type` (string|null), `started_at` (string|null), `stopped_at` (string|null).

### Pipeline workflows (`-p NUMBER --json`)

```json
[
  {
    "id": "workflow-uuid",
    "name": "build-and-test",
    "status": "failed",
    "created_at": "2025-01-15T10:00:00Z",
    "stopped_at": "2025-01-15T10:05:00Z",
    "pipeline_number": 142
  }
]
```

Fields: `id` (string), `name` (string), `status` (string), `created_at` (string|null), `stopped_at` (string|null), `pipeline_number` (number|null).

### Test results (`-j JOB --tests --json`)

```json
[
  {
    "name": "test_login",
    "classname": "AuthSpec",
    "result": "failure",
    "message": "Expected true got false",
    "run_time": 0.437,
    "source": "rspec",
    "file": "spec/auth_spec.rb"
  }
]
```

Fields: all optional (string|null except `run_time` which is number|null). `result` values: `"success"`, `"failure"`, `"skipped"`.

## Exit Codes

- Default: always exits 0
- With `--fail-on-error`: exits 1 when the job status is not `"success"` (logs mode) or when any test has `"failure"`/`"failed"` result (test mode)

## Constraints

- `-j`, `-w`, `-p` are mutually exclusive
- `--errors-only`, `--grep`, `--fail-on-error`, `--tests` require `-j`
- `--failed-only` requires `--tests`
- `--tests` cannot be combined with `--errors-only` or `--grep`
- URL cannot be combined with `-j`, `-w`, or `-p`
