---
name: circleci-logs
description: CLI tool to fetch job logs, workflow info, and pipeline info from CircleCI. Use for investigating CI failures, searching logs, and checking test results.
---

# circleci-logs

Fetch CircleCI build information from the command line.

## When to use

- Investigating why CI failed
- Searching or filtering job logs
- Checking test results
- Listing workflow or pipeline status

## Prerequisites

- Environment variable `CIRCLE_TOKEN` must be set
- Run inside a GitHub/Bitbucket git repository (project is auto-detected)

## Usage

### CI failed → View failure logs

```bash
# When you know the job number
circleci-logs -j <JOB_NUMBER> --errors-only

# Pass a CircleCI URL directly
circleci-logs "https://app.circleci.com/pipelines/github/org/repo/123/workflows/UUID/jobs/456" --errors-only
```

### Search logs for errors

```bash
circleci-logs -j <JOB_NUMBER> --grep "error|Error|ERROR"
```

### Check test results

```bash
# Show only failed tests
circleci-logs -j <JOB_NUMBER> --tests --failed-only
```

### List jobs in a workflow

```bash
circleci-logs -w <WORKFLOW_UUID>
```

### List workflows in a pipeline

```bash
circleci-logs -p <PIPELINE_NUMBER>
```

## JSON output

Use `--json` for JSON output, useful for piping to other tools.

```bash
circleci-logs -j <JOB_NUMBER> --json
circleci-logs -w <WORKFLOW_UUID> --json
```

## Exit code control

`--fail-on-error` returns exit code 1 when the job has errors.

```bash
circleci-logs -j <JOB_NUMBER> --fail-on-error
```

## CircleCI hierarchy

```
Pipeline (number)  → Triggered by a git push or schedule
 └─ Workflow (UUID) → Defines job execution order and dependencies
     └─ Job (number)    → Runs steps in an execution environment
         └─ Step        → Actual command execution; logs live here
```

The mode is automatically selected based on URL depth:
- `/jobs/N` → Show job logs
- `/workflows/UUID` → List jobs
- Pipeline number only → List workflows
