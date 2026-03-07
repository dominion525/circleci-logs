mod api;
mod config;
mod models;
mod output;

use anyhow::{Context, Result};
use clap::Parser;
use regex::Regex;

use api::CircleCiClient;
use config::Config;

#[derive(Parser)]
#[command(
    name = "circleci-logs",
    about = "Fetch job logs and workflow info from CircleCI"
)]
struct Cli {
    /// Fetch job log by job number
    #[arg(short = 'j', long = "jid")]
    job_number: Option<u64>,

    /// List jobs in a workflow by workflow ID
    #[arg(short = 'w', long = "wid")]
    workflow_id: Option<String>,

    /// List workflows in a pipeline by pipeline number
    #[arg(short = 'p', long = "pid")]
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

    if cli.job_number.is_none() && cli.workflow_id.is_none() && cli.pipeline_number.is_none() {
        Cli::parse_from(["circleci-logs", "--help"]);
        return Ok(());
    }

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

    let mut log_futures = Vec::new();
    if let Some(ref steps) = detail.steps {
        for step in steps {
            for action in &step.actions {
                if errors_only && action.status == "success" {
                    continue;
                }
                if let Some(ref url) = action.output_url {
                    let step_name = step.name.clone();
                    let url = url.clone();
                    log_futures.push((step_name, url));
                }
            }
        }
    }

    let mut logs = Vec::new();
    for (step_name, url) in &log_futures {
        let content = client.fetch_action_output(url).await.unwrap_or_default();
        logs.push((step_name.clone(), content));
    }

    output::print_job_log(&detail, &logs, errors_only, grep_re.as_ref(), json);
    Ok(())
}

async fn run_workflow_jobs(client: &CircleCiClient, workflow_id: &str, json: bool) -> Result<()> {
    let jobs = client.fetch_workflow_jobs(workflow_id).await?;
    output::print_workflow_jobs(&jobs, json);
    Ok(())
}

async fn run_pipeline_workflows(
    client: &CircleCiClient,
    pipeline_number: u64,
    json: bool,
) -> Result<()> {
    let workflows = client.fetch_pipeline_workflows(pipeline_number).await?;
    output::print_pipeline_workflows(&workflows, json);
    Ok(())
}
