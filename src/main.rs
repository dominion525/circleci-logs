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

    /// Show only failed steps (use with -j)
    #[arg(long)]
    errors_only: bool,

    /// Filter log lines by regex pattern (use with -j)
    #[arg(long)]
    grep: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = Config::load()?;
    let client = CircleCiClient::new(config);

    if let Some(job_number) = cli.job_number {
        run_job_log(
            &client,
            job_number,
            cli.errors_only,
            cli.grep.as_deref(),
            cli.json,
        )
        .await?;
    } else if let Some(ref workflow_id) = cli.workflow_id {
        run_workflow_jobs(&client, workflow_id, cli.json).await?;
    } else if let Some(pipeline_number) = cli.pipeline_number {
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
) -> Result<()> {
    let grep_re = grep
        .map(|pattern| Regex::new(pattern).context("Invalid regex pattern"))
        .transpose()?;

    let detail = client.fetch_job_detail(job_number).await?;
    let logs = fetch_step_logs(client, &detail, errors_only).await;

    output::print_job_log(&detail, &logs, errors_only, grep_re.as_ref(), json)?;
    Ok(())
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
            vcs_type: "github".into(),
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
            .and(path("/api/v1.1/project/github/test-org/test-repo/42"))
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

        let result = run_job_log(&client, 42, false, None, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn run_job_log_invalid_grep() {
        let client = CircleCiClient::with_base_url(test_config(), "http://unused:9999".to_string());

        let result = run_job_log(&client, 42, false, Some("[invalid"), false).await;
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid regex pattern"));
    }
}
