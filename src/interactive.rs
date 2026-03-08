use anyhow::Result;
use chrono::{DateTime, Local};
use colored::Colorize;
use dialoguer::Select;

use crate::api::CircleCiClient;
use crate::models::*;

pub enum InteractiveStart {
    Pipelines,
    Workflows { pipeline_number: u64 },
    Jobs { workflow_id: String },
}

enum State {
    Pipelines,
    Workflows {
        pipeline_number: u64,
        pipeline_id: String,
    },
    Jobs {
        workflow_id: String,
        pipeline_number: u64,
        pipeline_id: String,
    },
    Steps {
        job_number: u64,
        workflow_id: String,
        pipeline_number: u64,
        pipeline_id: String,
    },
    Done,
}

pub async fn run_interactive(client: &CircleCiClient, start: InteractiveStart) -> Result<()> {
    let mut state = match start {
        InteractiveStart::Pipelines => State::Pipelines,
        InteractiveStart::Workflows { pipeline_number } => {
            let pipeline_id = client.find_pipeline_uuid(pipeline_number).await?;
            State::Workflows {
                pipeline_number,
                pipeline_id,
            }
        }
        InteractiveStart::Jobs { workflow_id } => State::Jobs {
            workflow_id,
            pipeline_number: 0,
            pipeline_id: String::new(),
        },
    };

    loop {
        match state {
            State::Pipelines => {
                state = select_pipeline(client).await?;
            }
            State::Workflows {
                pipeline_number,
                ref pipeline_id,
            } => {
                let pid = pipeline_id.clone();
                state = select_workflow(client, pipeline_number, &pid).await?;
            }
            State::Jobs {
                ref workflow_id,
                pipeline_number,
                ref pipeline_id,
            } => {
                let wid = workflow_id.clone();
                let pid = pipeline_id.clone();
                state = select_job(client, &wid, pipeline_number, &pid).await?;
            }
            State::Steps {
                job_number,
                ref workflow_id,
                pipeline_number,
                ref pipeline_id,
            } => {
                let wid = workflow_id.clone();
                let pid = pipeline_id.clone();
                state = select_step(client, job_number, &wid, pipeline_number, &pid).await?;
            }
            State::Done => break,
        }
    }

    Ok(())
}

async fn select_pipeline(client: &CircleCiClient) -> Result<State> {
    let mut items: Vec<Pipeline> = Vec::new();
    let page = client.fetch_pipelines_page(None).await?;
    items.extend(page.items);
    let mut next_page_token = page.next_page_token;

    loop {
        let mut labels: Vec<String> = items.iter().map(format_pipeline_item).collect();
        if next_page_token.is_some() {
            labels.push("▼ Load more...".to_string());
        }

        if labels.is_empty() {
            println!("No pipelines found.");
            return Ok(State::Done);
        }

        let selection = Select::new()
            .with_prompt("Select a pipeline")
            .items(&labels)
            .default(0)
            .interact_opt()?;

        let selection = match selection {
            Some(s) => s,
            None => return Ok(State::Done),
        };

        // Load more check
        if next_page_token.is_some() && selection == labels.len() - 1 {
            let page = client
                .fetch_pipelines_page(next_page_token.as_deref())
                .await?;
            items.extend(page.items);
            next_page_token = page.next_page_token;
            continue;
        }

        let pipeline = &items[selection];
        return Ok(State::Workflows {
            pipeline_number: pipeline.number,
            pipeline_id: pipeline.id.clone(),
        });
    }
}

async fn select_workflow(
    client: &CircleCiClient,
    pipeline_number: u64,
    pipeline_id: &str,
) -> Result<State> {
    let mut items: Vec<PipelineWorkflow> = Vec::new();
    let page = client
        .fetch_pipeline_workflows_page(pipeline_id, None)
        .await?;
    items.extend(page.items);
    let mut next_page_token = page.next_page_token;

    loop {
        let mut labels: Vec<String> = vec![".. (back)".to_string()];
        labels.extend(items.iter().map(format_workflow_item));
        if next_page_token.is_some() {
            labels.push("▼ Load more...".to_string());
        }

        let selection = Select::new()
            .with_prompt("Select a workflow")
            .items(&labels)
            .default(0)
            .interact_opt()?;

        let selection = match selection {
            Some(s) => s,
            None => return Ok(State::Done),
        };

        // back
        if selection == 0 {
            return Ok(State::Pipelines);
        }

        // Load more check
        if next_page_token.is_some() && selection == labels.len() - 1 {
            let page = client
                .fetch_pipeline_workflows_page(pipeline_id, next_page_token.as_deref())
                .await?;
            items.extend(page.items);
            next_page_token = page.next_page_token;
            continue;
        }

        let wf = &items[selection - 1]; // -1 for back entry
        return Ok(State::Jobs {
            workflow_id: wf.id.clone(),
            pipeline_number,
            pipeline_id: pipeline_id.to_string(),
        });
    }
}

async fn select_job(
    client: &CircleCiClient,
    workflow_id: &str,
    pipeline_number: u64,
    pipeline_id: &str,
) -> Result<State> {
    let mut items: Vec<WorkflowJob> = Vec::new();
    let page = client.fetch_workflow_jobs_page(workflow_id, None).await?;
    items.extend(page.items);
    let mut next_page_token = page.next_page_token;

    loop {
        let mut labels: Vec<String> = vec![".. (back)".to_string()];
        labels.extend(items.iter().map(format_job_item));
        if next_page_token.is_some() {
            labels.push("▼ Load more...".to_string());
        }

        let selection = Select::new()
            .with_prompt("Select a job")
            .items(&labels)
            .default(0)
            .interact_opt()?;

        let selection = match selection {
            Some(s) => s,
            None => return Ok(State::Done),
        };

        // back
        if selection == 0 {
            if pipeline_id.is_empty() {
                return Ok(State::Done);
            }
            return Ok(State::Workflows {
                pipeline_number,
                pipeline_id: pipeline_id.to_string(),
            });
        }

        // Load more check
        if next_page_token.is_some() && selection == labels.len() - 1 {
            let page = client
                .fetch_workflow_jobs_page(workflow_id, next_page_token.as_deref())
                .await?;
            items.extend(page.items);
            next_page_token = page.next_page_token;
            continue;
        }

        let job = &items[selection - 1];
        if let Some(job_number) = job.job_number {
            return Ok(State::Steps {
                job_number,
                workflow_id: workflow_id.to_string(),
                pipeline_number,
                pipeline_id: pipeline_id.to_string(),
            });
        } else {
            println!("This job has no job number (may be pending or blocked).");
        }
        continue;
    }
}

async fn select_step(
    client: &CircleCiClient,
    job_number: u64,
    workflow_id: &str,
    pipeline_number: u64,
    pipeline_id: &str,
) -> Result<State> {
    let detail = client.fetch_job_detail(job_number).await?;

    let steps = match detail.steps {
        Some(ref steps) => steps,
        None => {
            println!("No steps found for this job.");
            return Ok(State::Jobs {
                workflow_id: workflow_id.to_string(),
                pipeline_number,
                pipeline_id: pipeline_id.to_string(),
            });
        }
    };

    loop {
        let mut labels: Vec<String> = vec![".. (back)".to_string()];
        labels.extend(steps.iter().map(format_step_item));

        let selection = Select::new()
            .with_prompt("Select a step")
            .items(&labels)
            .default(0)
            .interact_opt()?;

        let selection = match selection {
            Some(s) => s,
            None => return Ok(State::Done),
        };

        // back
        if selection == 0 {
            return Ok(State::Jobs {
                workflow_id: workflow_id.to_string(),
                pipeline_number,
                pipeline_id: pipeline_id.to_string(),
            });
        }

        let step = &steps[selection - 1];
        if !show_step_log(client, &detail, step).await? {
            return Ok(State::Done);
        }
        // Stay in step list after viewing log
        continue;
    }
}

async fn show_step_log(
    client: &CircleCiClient,
    detail: &JobDetail,
    step: &Step,
) -> Result<bool> {
    let logs = crate::fetch_single_step_logs(client, step).await;
    crate::output::print_step_log(detail, step, &logs)?;

    let selection = Select::new()
        .with_prompt("Log view")
        .items(&["Back to step list", "Exit"])
        .default(0)
        .clear(false)
        .interact_opt()?;

    match selection {
        Some(0) => Ok(true),
        _ => Ok(false),
    }
}

fn format_step_item(step: &Step) -> String {
    let actions = &step.actions;

    // Aggregate status: if any failed, show failed
    let status = if actions.iter().any(|a| a.status == "failed" || a.status == "timedout") {
        "failed"
    } else if actions.iter().all(|a| a.status == "success") {
        "success"
    } else if actions.iter().any(|a| a.status == "running") {
        "running"
    } else {
        actions.first().map(|a| a.status.as_str()).unwrap_or("-")
    };

    // Total duration across all actions
    let total_millis: Option<u64> = {
        let sum: u64 = actions.iter().filter_map(|a| a.run_time_millis).sum();
        if sum > 0 || actions.iter().any(|a| a.run_time_millis.is_some()) {
            Some(sum)
        } else {
            None
        }
    };

    let duration = crate::output::format_duration(total_millis);
    let parallel = if actions.len() > 1 {
        format!(" (x{})", actions.len())
    } else {
        String::new()
    };

    format!(
        "[{}] {:<40} {}{}",
        colorize_status_padded(status, 7),
        step.name,
        duration,
        parallel
    )
}

/// Pad status to `width` visible characters, then colorize.
fn colorize_status_padded(status: &str, width: usize) -> String {
    let padded = format!("{:<width$}", status, width = width);
    match status {
        "success" | "created" => padded.green().to_string(),
        "failed" | "failure" | "timedout" | "infrastructure_fail" | "error" => {
            padded.red().to_string()
        }
        "running" => padded.yellow().to_string(),
        "canceled" | "cancelled" => padded.dimmed().to_string(),
        "not_run" | "skipped" => padded.dimmed().to_string(),
        _ => padded,
    }
}

fn format_timestamp(ts: &str) -> String {
    match DateTime::parse_from_rfc3339(ts) {
        Ok(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        Err(_) => ts.to_string(),
    }
}

fn format_pipeline_item(p: &Pipeline) -> String {
    let branch = p
        .vcs
        .as_ref()
        .and_then(|v| v.branch.as_deref())
        .unwrap_or("-");
    let state = p.state.as_deref().unwrap_or("-");
    let created = p
        .created_at
        .as_deref()
        .map(format_timestamp)
        .unwrap_or_else(|| "-".to_string());
    format!(
        "#{:<8} {:<30} {} {}",
        p.number,
        branch,
        colorize_status_padded(state, 12),
        created
    )
}

fn format_workflow_item(wf: &PipelineWorkflow) -> String {
    let created = wf
        .created_at
        .as_deref()
        .map(format_timestamp)
        .unwrap_or_else(|| "-".to_string());
    format!(
        "{:<25} {} {}",
        wf.name,
        colorize_status_padded(&wf.status, 12),
        created
    )
}

fn format_job_item(job: &WorkflowJob) -> String {
    let num = job
        .job_number
        .map(|n| format!("#{}", n))
        .unwrap_or_else(|| "-".to_string());
    let started = job
        .started_at
        .as_deref()
        .map(format_timestamp)
        .unwrap_or_else(|| "-".to_string());
    format!(
        "{:<8} {:<25} {} {}",
        num,
        job.name,
        colorize_status_padded(&job.status, 12),
        started
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_pipeline_item_full() {
        let p = Pipeline {
            id: "id-1".to_string(),
            number: 42,
            state: Some("created".to_string()),
            created_at: Some("2024-06-01T10:00:00Z".to_string()),
            trigger: Some(PipelineTrigger {
                trigger_type: Some("webhook".to_string()),
            }),
            vcs: Some(PipelineVcs {
                branch: Some("main".to_string()),
            }),
        };
        let result = format_pipeline_item(&p);
        assert!(result.contains("#42"));
        assert!(result.contains("main"));
        assert!(result.contains("2024-06-01"));
    }

    #[test]
    fn format_pipeline_item_no_branch() {
        let p = Pipeline {
            id: "id-2".to_string(),
            number: 10,
            state: Some("created".to_string()),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
            trigger: None,
            vcs: None,
        };
        let result = format_pipeline_item(&p);
        assert!(result.contains("#10"));
        assert!(result.contains("-"));
    }

    #[test]
    fn format_pipeline_item_no_created_at() {
        let p = Pipeline {
            id: "id-3".to_string(),
            number: 5,
            state: None,
            created_at: None,
            trigger: None,
            vcs: None,
        };
        let result = format_pipeline_item(&p);
        assert!(result.contains("#5"));
    }

    #[test]
    fn format_workflow_item_full() {
        let wf = PipelineWorkflow {
            id: "wf-1".to_string(),
            name: "build-and-test".to_string(),
            status: "success".to_string(),
            created_at: Some("2024-06-01T12:00:00Z".to_string()),
            stopped_at: None,
            pipeline_number: Some(42),
        };
        let result = format_workflow_item(&wf);
        assert!(result.contains("build-and-test"));
        assert!(result.contains("2024-06-01"));
    }

    #[test]
    fn format_workflow_item_no_created_at() {
        let wf = PipelineWorkflow {
            id: "wf-2".to_string(),
            name: "deploy".to_string(),
            status: "running".to_string(),
            created_at: None,
            stopped_at: None,
            pipeline_number: None,
        };
        let result = format_workflow_item(&wf);
        assert!(result.contains("deploy"));
    }

    #[test]
    fn format_job_item_full() {
        let job = WorkflowJob {
            id: "j1".to_string(),
            name: "unit-test".to_string(),
            status: "success".to_string(),
            job_number: Some(456),
            job_type: Some("build".to_string()),
            started_at: Some("2024-06-01T12:30:00Z".to_string()),
            stopped_at: None,
        };
        let result = format_job_item(&job);
        assert!(result.contains("#456"));
        assert!(result.contains("unit-test"));
        assert!(result.contains("2024-06-01"));
    }

    #[test]
    fn format_job_item_no_job_number() {
        let job = WorkflowJob {
            id: "j2".to_string(),
            name: "pending-job".to_string(),
            status: "not_run".to_string(),
            job_number: None,
            job_type: None,
            started_at: None,
            stopped_at: None,
        };
        let result = format_job_item(&job);
        assert!(result.contains("-"));
        assert!(result.contains("pending-job"));
    }

    fn make_action(name: &str, status: &str, millis: Option<u64>) -> Step {
        Step {
            name: name.to_string(),
            actions: vec![Action {
                name: name.to_string(),
                status: status.to_string(),
                run_time_millis: millis,
                output_url: None,
                step: None,
                index: None,
            }],
        }
    }

    fn make_step_with_actions(name: &str, actions: Vec<Action>) -> Step {
        Step {
            name: name.to_string(),
            actions,
        }
    }

    #[test]
    fn format_step_item_success_single() {
        colored::control::set_override(false);
        let step = make_action("Build", "success", Some(5000));
        let result = format_step_item(&step);
        assert!(result.contains("success"));
        assert!(result.contains("Build"));
        assert!(result.contains("5s"));
        assert!(!result.contains("(x"));
    }

    #[test]
    fn format_step_item_failed_single() {
        colored::control::set_override(false);
        let step = make_action("Test", "failed", Some(12000));
        let result = format_step_item(&step);
        assert!(result.contains("failed"));
        assert!(result.contains("12s"));
    }

    #[test]
    fn format_step_item_parallel() {
        colored::control::set_override(false);
        let actions = (0..6)
            .map(|i| Action {
                name: format!("node {}", i),
                status: "success".to_string(),
                run_time_millis: Some(10000),
                output_url: None,
                step: None,
                index: Some(i),
            })
            .collect();
        let step = make_step_with_actions("RSpec", actions);
        let result = format_step_item(&step);
        assert!(result.contains("(x6)"));
        assert!(result.contains("1m0s")); // 60s total
    }

    #[test]
    fn format_step_item_parallel_with_failure() {
        colored::control::set_override(false);
        let actions = vec![
            Action {
                name: "node 0".to_string(),
                status: "success".to_string(),
                run_time_millis: Some(5000),
                output_url: None,
                step: None,
                index: Some(0),
            },
            Action {
                name: "node 1".to_string(),
                status: "failed".to_string(),
                run_time_millis: Some(3000),
                output_url: None,
                step: None,
                index: Some(1),
            },
        ];
        let step = make_step_with_actions("Test", actions);
        let result = format_step_item(&step);
        assert!(result.contains("failed"));
        assert!(result.contains("(x2)"));
        assert!(result.contains("8s"));
    }

    #[test]
    fn format_step_item_no_duration() {
        colored::control::set_override(false);
        let step = make_action("Setup", "success", None);
        let result = format_step_item(&step);
        assert!(result.contains("-"));
    }

    #[test]
    fn format_job_item_no_started_at() {
        let job = WorkflowJob {
            id: "j3".to_string(),
            name: "blocked".to_string(),
            status: "blocked".to_string(),
            job_number: Some(100),
            job_type: None,
            started_at: None,
            stopped_at: None,
        };
        let result = format_job_item(&job);
        assert!(result.contains("#100"));
        assert!(result.contains("blocked"));
    }
}
