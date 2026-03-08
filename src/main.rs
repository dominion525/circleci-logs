mod api;
mod config;
mod models;
mod output;

use anyhow::{Context, Result};
use clap::{ArgGroup, Parser};
use regex::Regex;

use api::CircleCiClient;
use config::Config;

#[derive(Parser)]
#[command(
    name = "circleci-logs",
    about = "Fetch job logs and workflow info from CircleCI",
    arg_required_else_help = true,
    group = ArgGroup::new("target").required(true)
)]
struct Cli {
    /// Fetch job log by job number
    #[arg(short = 'j', long = "jid", group = "target")]
    job_number: Option<u64>,

    /// List jobs in a workflow by workflow ID
    #[arg(short = 'w', long = "wid", group = "target")]
    workflow_id: Option<String>,

    /// List workflows in a pipeline by pipeline number
    #[arg(short = 'p', long = "pid", group = "target")]
    pipeline_number: Option<u64>,

    /// Output in JSON format
    #[arg(long)]
    json: bool,

    /// Show only failed steps (requires -j)
    #[arg(long)]
    errors_only: bool,

    /// Filter log lines by regex pattern (requires -j)
    #[arg(long)]
    grep: Option<String>,

    /// Exit with code 1 if the job has errors (requires -j)
    #[arg(long)]
    fail_on_error: bool,

    /// Show test results (requires -j)
    #[arg(long)]
    tests: bool,

    /// Show only failed tests (requires --tests)
    #[arg(long)]
    failed_only: bool,

    /// CircleCI URL (e.g. https://app.circleci.com/pipelines/github/org/repo/123/workflows/UUID/jobs/456)
    #[arg(group = "target")]
    url: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Resolve target from URL or flags
    let (job_number, workflow_id, pipeline_number, url_config) = if let Some(ref url) = cli.url {
        let parsed = parse_circleci_url(url)?;
        (
            parsed.job_number,
            parsed.workflow_id,
            parsed.pipeline_number,
            Some((parsed.vcs_type, parsed.org, parsed.repo)),
        )
    } else {
        (cli.job_number, cli.workflow_id.clone(), cli.pipeline_number, None)
    };

    // Validate -j dependent options
    if job_number.is_none()
        && (cli.errors_only || cli.grep.is_some() || cli.fail_on_error || cli.tests || cli.failed_only)
    {
        let flag = if cli.errors_only {
            "--errors-only"
        } else if cli.grep.is_some() {
            "--grep"
        } else if cli.fail_on_error {
            "--fail-on-error"
        } else if cli.tests {
            "--tests"
        } else {
            "--failed-only"
        };
        anyhow::bail!("{} can only be used with -j/--jid (or a URL ending in /jobs/N)", flag);
    }

    if cli.failed_only && !cli.tests {
        anyhow::bail!("--failed-only can only be used with --tests");
    }

    if cli.tests && (cli.errors_only || cli.grep.is_some()) {
        let flag = if cli.errors_only {
            "--errors-only"
        } else {
            "--grep"
        };
        anyhow::bail!("--tests cannot be used with {}", flag);
    }

    let mut config = Config::load()?;

    // If URL provided, override project with URL's project
    if let Some((vcs_type, org, repo)) = url_config {
        if config.vcs_type != vcs_type || config.org != org || config.repo != repo {
            eprintln!(
                "Note: URL project ({}/{}/{}) differs from config ({}/{}/{}), using URL",
                vcs_type, org, repo, config.vcs_type, config.org, config.repo
            );
        }
        config.vcs_type = vcs_type;
        config.org = org;
        config.repo = repo;
    }

    let client = CircleCiClient::new(config);

    if let Some(job_number) = job_number {
        if cli.tests {
            let has_error = run_job_tests(
                &client,
                job_number,
                cli.failed_only,
                cli.json,
                cli.fail_on_error,
            )
            .await?;
            if has_error {
                std::process::exit(1);
            }
        } else {
            let has_error = run_job_log(
                &client,
                job_number,
                cli.errors_only,
                cli.grep.as_deref(),
                cli.json,
                cli.fail_on_error,
            )
            .await?;
            if has_error {
                std::process::exit(1);
            }
        }
    } else if let Some(ref workflow_id) = workflow_id {
        run_workflow_jobs(&client, workflow_id, cli.json).await?;
    } else if let Some(pipeline_number) = pipeline_number {
        run_pipeline_workflows(&client, pipeline_number, cli.json).await?;
    }

    Ok(())
}

async fn run_job_log(
    client: &CircleCiClient,
    job_number: u64,
    errors_only: bool,
    grep: Option<&str>,
    json: bool,
    fail_on_error: bool,
) -> Result<bool> {
    let grep_re = grep
        .map(|pattern| Regex::new(pattern).context("Invalid regex pattern"))
        .transpose()?;

    let detail = client.fetch_job_detail(job_number).await?;
    let logs = fetch_step_logs(client, &detail, errors_only).await;

    output::print_job_log(&detail, &logs, errors_only, grep_re.as_ref(), json)?;

    let has_error = fail_on_error
        && detail.status.as_deref().is_some_and(|s| s != "success");
    Ok(has_error)
}

async fn fetch_step_logs(
    client: &CircleCiClient,
    detail: &models::JobDetail,
    errors_only: bool,
) -> Vec<(String, String)> {
    let mut targets = Vec::new();
    if let Some(ref steps) = detail.steps {
        for step in steps {
            for action in &step.actions {
                if errors_only && action.status == "success" {
                    continue;
                }
                if let Some(ref url) = action.output_url {
                    targets.push((step.name.clone(), url.clone()));
                }
            }
        }
    }

    let mut logs = Vec::new();
    for (step_name, url) in &targets {
        let content = match client.fetch_action_output(url).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Warning: failed to fetch log for '{}': {}", step_name, e);
                String::new()
            }
        };
        logs.push((step_name.clone(), content));
    }
    logs
}

async fn run_job_tests(
    client: &CircleCiClient,
    job_number: u64,
    failed_only: bool,
    json: bool,
    fail_on_error: bool,
) -> Result<bool> {
    let tests = client.fetch_job_tests(job_number).await?;

    if tests.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No test results found. (Does the job use store_test_results?)");
        }
        return Ok(false);
    }

    output::print_test_results(&tests, job_number, failed_only, json)?;

    let has_error = fail_on_error
        && tests.iter().any(|t| {
            matches!(t.result.as_deref(), Some("failure") | Some("failed"))
        });
    Ok(has_error)
}

#[derive(Debug)]
struct ParsedUrl {
    vcs_type: String,
    org: String,
    repo: String,
    job_number: Option<u64>,
    workflow_id: Option<String>,
    pipeline_number: Option<u64>,
}

fn parse_circleci_url(url: &str) -> Result<ParsedUrl> {
    let url = url.trim();
    let path = url
        .strip_prefix("https://app.circleci.com/")
        .or_else(|| url.strip_prefix("https://circleci.com/"))
        .with_context(|| format!("Not a CircleCI URL: {}", url))?;

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    // Expected: pipelines/{vcs}/{org}/{repo}/{pipeline_number}[/workflows/{wf_id}[/jobs/{job_number}]]
    if segments.len() < 5 || segments[0] != "pipelines" {
        anyhow::bail!("Invalid CircleCI URL format: expected /pipelines/{{vcs}}/{{org}}/{{repo}}/{{number}}/...");
    }

    let vcs_type = config::normalize_vcs_type(segments[1])?;
    let org = segments[2].to_string();
    let repo = segments[3].to_string();
    let pipeline_number: u64 = segments[4]
        .parse()
        .with_context(|| format!("Invalid pipeline number: {}", segments[4]))?;

    let mut workflow_id = None;
    let mut job_number = None;

    if segments.len() >= 7 && segments[5] == "workflows" {
        workflow_id = Some(segments[6].to_string());
    }
    if segments.len() >= 9 && segments[7] == "jobs" {
        job_number = Some(
            segments[8]
                .parse()
                .with_context(|| format!("Invalid job number: {}", segments[8]))?,
        );
    }

    Ok(ParsedUrl {
        vcs_type,
        org,
        repo,
        job_number,
        workflow_id,
        pipeline_number: Some(pipeline_number),
    })
}

async fn run_workflow_jobs(client: &CircleCiClient, workflow_id: &str, json: bool) -> Result<()> {
    let jobs = client.fetch_workflow_jobs(workflow_id).await?;
    output::print_workflow_jobs(&jobs, json)?;
    Ok(())
}

async fn run_pipeline_workflows(
    client: &CircleCiClient,
    pipeline_number: u64,
    json: bool,
) -> Result<()> {
    let workflows = client.fetch_pipeline_workflows(pipeline_number).await?;
    output::print_pipeline_workflows(&workflows, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config() -> Config {
        Config {
            token: "test-token".into(),
            vcs_type: "gh".into(),
            org: "test-org".into(),
            repo: "test-repo".into(),
        }
    }

    fn make_action(name: &str, status: &str, output_url: Option<&str>) -> models::Action {
        models::Action {
            name: name.to_string(),
            status: status.to_string(),
            run_time_millis: None,
            output_url: output_url.map(|s| s.to_string()),
            step: None,
            index: None,
        }
    }

    fn make_step(name: &str, actions: Vec<models::Action>) -> models::Step {
        models::Step {
            name: name.to_string(),
            actions,
        }
    }

    fn make_detail(steps: Option<Vec<models::Step>>) -> models::JobDetail {
        models::JobDetail {
            steps,
            status: Some("success".to_string()),
            build_num: Some(42),
            workflows: None,
        }
    }

    // --- fetch_step_logs tests ---

    #[tokio::test]
    async fn fetch_step_logs_with_output_urls() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/output/step1"))
            .and(header("Circle-Token", "test-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([{"message": "log1\n", "type": "out"}])),
            )
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/output/step2"))
            .and(header("Circle-Token", "test-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([{"message": "log2\n", "type": "out"}])),
            )
            .mount(&server)
            .await;

        let url1 = format!("{}/output/step1", server.uri());
        let url2 = format!("{}/output/step2", server.uri());
        let detail = make_detail(Some(vec![
            make_step("step1", vec![make_action("a1", "success", Some(&url1))]),
            make_step("step2", vec![make_action("a2", "failed", Some(&url2))]),
        ]));

        let logs = fetch_step_logs(&client, &detail, false).await;
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].0, "step1");
        assert_eq!(logs[0].1, "log1\n");
        assert_eq!(logs[1].0, "step2");
        assert_eq!(logs[1].1, "log2\n");
    }

    #[tokio::test]
    async fn fetch_step_logs_errors_only() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/output/failed"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!([{"message": "error output\n", "type": "out"}]),
                ),
            )
            .mount(&server)
            .await;

        let fail_url = format!("{}/output/failed", server.uri());
        let pass_url = format!("{}/output/passing", server.uri());
        let detail = make_detail(Some(vec![
            make_step(
                "passing",
                vec![make_action("a1", "success", Some(&pass_url))],
            ),
            make_step(
                "failing",
                vec![make_action("a2", "failed", Some(&fail_url))],
            ),
        ]));

        let logs = fetch_step_logs(&client, &detail, true).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].0, "failing");
        assert_eq!(logs[0].1, "error output\n");
    }

    #[tokio::test]
    async fn fetch_step_logs_no_steps() {
        let client = CircleCiClient::with_base_url(test_config(), "http://unused:9999".to_string());
        let detail = make_detail(None);

        let logs = fetch_step_logs(&client, &detail, false).await;
        assert!(logs.is_empty());
    }

    #[tokio::test]
    async fn fetch_step_logs_fetch_failure() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/output/broken"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
            .mount(&server)
            .await;

        let url = format!("{}/output/broken", server.uri());
        let detail = make_detail(Some(vec![make_step(
            "broken",
            vec![make_action("a1", "failed", Some(&url))],
        )]));

        let logs = fetch_step_logs(&client, &detail, false).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].0, "broken");
        assert_eq!(logs[0].1, "");
    }

    #[tokio::test]
    async fn fetch_step_logs_no_output_url() {
        let client = CircleCiClient::with_base_url(test_config(), "http://unused:9999".to_string());
        let detail = make_detail(Some(vec![make_step(
            "step1",
            vec![make_action("a1", "success", None)],
        )]));

        let logs = fetch_step_logs(&client, &detail, false).await;
        assert!(logs.is_empty());
    }

    // --- run_job_log tests ---

    #[tokio::test]
    async fn run_job_log_success() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        let output_url = format!("{}/output/1", server.uri());
        let job_detail = serde_json::json!({
            "steps": [{
                "name": "build",
                "actions": [{
                    "name": "compile",
                    "status": "success",
                    "run_time_millis": 1000,
                    "output_url": output_url,
                    "step": 0,
                    "index": 0
                }]
            }],
            "status": "success",
            "build_num": 42,
            "workflows": {"workflow_name": "main", "job_name": "build"}
        });

        Mock::given(method("GET"))
            .and(path("/api/v1.1/project/gh/test-org/test-repo/42"))
            .and(header("Circle-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&job_detail))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/output/1"))
            .and(header("Circle-Token", "test-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([{"message": "built ok\n", "type": "out"}])),
            )
            .mount(&server)
            .await;

        let result = run_job_log(&client, 42, false, None, false, false).await;
        assert_eq!(result.unwrap(), false);
    }

    #[tokio::test]
    async fn run_job_log_fail_on_error_success_status() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        let job_detail = serde_json::json!({
            "steps": [],
            "status": "success",
            "build_num": 10
        });

        Mock::given(method("GET"))
            .and(path("/api/v1.1/project/gh/test-org/test-repo/10"))
            .and(header("Circle-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&job_detail))
            .mount(&server)
            .await;

        let result = run_job_log(&client, 10, false, None, false, true).await;
        assert_eq!(result.unwrap(), false);
    }

    #[tokio::test]
    async fn run_job_log_fail_on_error_failed_status() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        let job_detail = serde_json::json!({
            "steps": [],
            "status": "failed",
            "build_num": 11
        });

        Mock::given(method("GET"))
            .and(path("/api/v1.1/project/gh/test-org/test-repo/11"))
            .and(header("Circle-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&job_detail))
            .mount(&server)
            .await;

        let result = run_job_log(&client, 11, false, None, false, true).await;
        assert_eq!(result.unwrap(), true);
    }

    #[tokio::test]
    async fn run_job_log_no_fail_on_error_failed_status() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        let job_detail = serde_json::json!({
            "steps": [],
            "status": "failed",
            "build_num": 12
        });

        Mock::given(method("GET"))
            .and(path("/api/v1.1/project/gh/test-org/test-repo/12"))
            .and(header("Circle-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&job_detail))
            .mount(&server)
            .await;

        let result = run_job_log(&client, 12, false, None, false, false).await;
        assert_eq!(result.unwrap(), false);
    }

    // --- run_job_tests tests ---

    #[tokio::test]
    async fn run_job_tests_with_results() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/api/v2/project/gh/test-org/test-repo/42/tests"))
            .and(header("Circle-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [
                    {"name": "t1", "classname": null, "result": "success", "message": null, "run_time": 0.5, "source": null, "file": null},
                    {"name": "t2", "classname": null, "result": "failure", "message": "bad", "run_time": 0.1, "source": null, "file": null}
                ],
                "next_page_token": null
            })))
            .mount(&server)
            .await;

        let result = run_job_tests(&client, 42, false, false, false).await;
        assert_eq!(result.unwrap(), false);
    }

    #[tokio::test]
    async fn run_job_tests_empty() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/api/v2/project/gh/test-org/test-repo/99/tests"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [],
                "next_page_token": null
            })))
            .mount(&server)
            .await;

        let result = run_job_tests(&client, 99, false, false, false).await;
        assert_eq!(result.unwrap(), false);
    }

    #[tokio::test]
    async fn run_job_tests_fail_on_error_with_failure() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/api/v2/project/gh/test-org/test-repo/10/tests"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [
                    {"name": "t1", "classname": null, "result": "failure", "message": "err", "run_time": 0.1, "source": null, "file": null}
                ],
                "next_page_token": null
            })))
            .mount(&server)
            .await;

        let result = run_job_tests(&client, 10, false, false, true).await;
        assert_eq!(result.unwrap(), true);
    }

    #[tokio::test]
    async fn run_job_tests_fail_on_error_all_pass() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/api/v2/project/gh/test-org/test-repo/10/tests"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [
                    {"name": "t1", "classname": null, "result": "success", "message": null, "run_time": 0.5, "source": null, "file": null}
                ],
                "next_page_token": null
            })))
            .mount(&server)
            .await;

        let result = run_job_tests(&client, 10, false, false, true).await;
        assert_eq!(result.unwrap(), false);
    }

    // --- parse_circleci_url tests ---

    #[test]
    fn parse_url_full_jobs() {
        let url = "https://app.circleci.com/pipelines/github/co-labo-maker/co-labo-maker/10731/workflows/95c17b45-ef51-4f57-8aeb-9e247126c5a1/jobs/61012";
        let parsed = parse_circleci_url(url).unwrap();
        assert_eq!(parsed.vcs_type, "gh");
        assert_eq!(parsed.org, "co-labo-maker");
        assert_eq!(parsed.repo, "co-labo-maker");
        assert_eq!(parsed.pipeline_number, Some(10731));
        assert_eq!(
            parsed.workflow_id.as_deref(),
            Some("95c17b45-ef51-4f57-8aeb-9e247126c5a1")
        );
        assert_eq!(parsed.job_number, Some(61012));
    }

    #[test]
    fn parse_url_workflows_only() {
        let url = "https://app.circleci.com/pipelines/github/org/repo/100/workflows/abc-def";
        let parsed = parse_circleci_url(url).unwrap();
        assert_eq!(parsed.pipeline_number, Some(100));
        assert_eq!(parsed.workflow_id.as_deref(), Some("abc-def"));
        assert!(parsed.job_number.is_none());
    }

    #[test]
    fn parse_url_pipelines_only() {
        let url = "https://app.circleci.com/pipelines/github/org/repo/42";
        let parsed = parse_circleci_url(url).unwrap();
        assert_eq!(parsed.pipeline_number, Some(42));
        assert!(parsed.workflow_id.is_none());
        assert!(parsed.job_number.is_none());
    }

    #[test]
    fn parse_url_circleci_com_domain() {
        let url = "https://circleci.com/pipelines/github/org/repo/10/workflows/wf-id/jobs/99";
        let parsed = parse_circleci_url(url).unwrap();
        assert_eq!(parsed.job_number, Some(99));
    }

    #[test]
    fn parse_url_bitbucket() {
        let url = "https://app.circleci.com/pipelines/bitbucket/org/repo/5";
        let parsed = parse_circleci_url(url).unwrap();
        assert_eq!(parsed.vcs_type, "bb");
    }

    #[test]
    fn parse_url_invalid_not_circleci() {
        let err = parse_circleci_url("https://github.com/org/repo").unwrap_err();
        assert!(err.to_string().contains("Not a CircleCI URL"));
    }

    #[test]
    fn parse_url_invalid_too_short() {
        let err =
            parse_circleci_url("https://app.circleci.com/pipelines/github/org").unwrap_err();
        assert!(err.to_string().contains("Invalid CircleCI URL format"));
    }

    #[test]
    fn parse_url_invalid_pipeline_number() {
        let err = parse_circleci_url(
            "https://app.circleci.com/pipelines/github/org/repo/notanumber",
        )
        .unwrap_err();
        assert!(err.to_string().contains("Invalid pipeline number"));
    }

    #[test]
    fn parse_url_trailing_slash() {
        let url = "https://app.circleci.com/pipelines/github/org/repo/42/";
        let parsed = parse_circleci_url(url).unwrap();
        assert_eq!(parsed.pipeline_number, Some(42));
    }

    #[tokio::test]
    async fn run_job_log_invalid_grep() {
        let client = CircleCiClient::with_base_url(test_config(), "http://unused:9999".to_string());

        let result = run_job_log(&client, 42, false, Some("[invalid"), false, false).await;
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid regex pattern"));
    }
}
