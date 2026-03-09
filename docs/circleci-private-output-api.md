# CircleCI Private Output API

[日本語版](circleci-private-output-api.ja.md)

Unofficial / undocumented API for fetching raw step output from CircleCI jobs,
including jobs that are still running.

> **Warning**: This is a private, undocumented API. It may change or break
> without notice. Use at your own risk.

## Endpoints

### Step stdout

```
GET /api/private/output/raw/{vcs}/{org}/{repo}/{job_number}/output/{task_index}/{step_id}
```

### Step stderr

```
GET /api/private/output/raw/{vcs}/{org}/{repo}/{job_number}/error/{task_index}/{step_id}
```

## Path Parameters

| Parameter    | Type   | Description                                                        |
|-------------|--------|--------------------------------------------------------------------|
| vcs         | string | VCS type (`gh` for GitHub, `bb` for Bitbucket)                     |
| org         | string | Organization or user name                                          |
| repo        | string | Repository name                                                    |
| job_number  | int    | CircleCI job number                                                |
| task_index  | int    | Action `index` field from v1.1 API (parallel node index, 0-based)  |
| step_id     | int    | Action `step` field from v1.1 API (NOT the array index of the step)|

### About step_id

The `step_id` parameter corresponds to the `step` field in the v1.1 API
action object, **not** the array index of the step in `steps[]`.
These values are typically non-sequential (e.g., 0, 99, 101, 102, ...).

To obtain the correct `step_id`, first fetch the job detail via:

```
GET /api/v1.1/project/{vcs}/{org}/{repo}/{job_number}
```

Then use `action.step` (not the step array index) as `step_id`.

## Authentication

```
Circle-Token: <your-api-token>
```

Unauthenticated requests return `404 {"message": "Build not found"}`.

## Response

### Content-Type

`application/octet-stream`

### Body

Raw text output of the step, including ANSI escape sequences for terminal
colors. For the `error` endpoint, the response body is stderr output.

### Notable Response Headers

| Header                 | Example   | Description                              |
|-----------------------|-----------|------------------------------------------|
| X-Terminal            | true      | Indicates output contains terminal codes |
| X-RateLimit-Limit     | 300       | Rate limit ceiling                       |
| X-RateLimit-Remaining | 299       | Remaining requests in window             |
| X-RateLimit-Reset     | 1         | Seconds until rate limit resets          |
| Cache-Control         | private, max-age=3600 | Caching policy                |

## HTTP Status Codes

| Code | Condition                                    |
|------|----------------------------------------------|
| 200  | Success (body contains output text)          |
| 204  | No content (e.g., stderr is empty)           |
| 404  | Build not found, or authentication failure   |

Note: An invalid `step_id` returns 200 with an empty body, not 404.

## Examples

```sh
# Fetch stdout for step_id=106, node 0
curl -H "Circle-Token: $CIRCLE_TOKEN" \
  "https://circleci.com/api/private/output/raw/gh/myorg/myrepo/12345/output/0/106"

# Fetch stderr for the same step
curl -H "Circle-Token: $CIRCLE_TOKEN" \
  "https://circleci.com/api/private/output/raw/gh/myorg/myrepo/12345/error/0/106"
```

## Reference Implementations

- [CircleCI MCP Server](https://github.com/CircleCI-Public/mcp-server-circleci) -
  `src/clients/circleci-private/jobsPrivate.ts`
- [CircleCI Kotlin Client](https://github.com/unhappychoice/circleci) -
  API client library (uses v1.1 output_url, not private API)
