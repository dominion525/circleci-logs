use anyhow::Result;
use dialoguer::Select;

use crate::api::{CircleCiClient, LogSource};
use crate::models::*;
use crate::output::{colorize_status, format_timestamp};

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
    Nodes {
        job_number: u64,
        detail: JobDetail,
        workflow_id: String,
        pipeline_number: u64,
        pipeline_id: String,
    },
    Steps {
        job_number: u64,
        detail: JobDetail,
        node_index: Option<usize>,
        workflow_id: String,
        pipeline_number: u64,
        pipeline_id: String,
    },
    Done,
}

enum LogAction {
    Back,
    Exit,
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
            State::Nodes {
                job_number,
                detail,
                ref workflow_id,
                pipeline_number,
                ref pipeline_id,
            } => {
                let wid = workflow_id.clone();
                let pid = pipeline_id.clone();
                state =
                    select_node(client, job_number, detail, &wid, pipeline_number, &pid).await?;
            }
            State::Steps {
                job_number,
                detail,
                node_index,
                ref workflow_id,
                pipeline_number,
                ref pipeline_id,
            } => {
                let wid = workflow_id.clone();
                let pid = pipeline_id.clone();
                state = select_step(
                    client,
                    job_number,
                    detail,
                    node_index,
                    &wid,
                    pipeline_number,
                    &pid,
                )
                .await?;
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

    let back_label = ".. (back to pipelines)";

    loop {
        let mut labels: Vec<String> = vec![back_label.to_string()];
        labels.extend(items.iter().map(format_workflow_item));
        if next_page_token.is_some() {
            labels.push("▼ Load more...".to_string());
        }
        labels.push(back_label.to_string());

        let selection = Select::new()
            .with_prompt("Select a workflow")
            .items(&labels)
            .default(1)
            .interact_opt()?;

        let selection = match selection {
            Some(s) => s,
            None => return Ok(State::Done),
        };

        // back (top or bottom)
        if selection == 0 || selection == labels.len() - 1 {
            return Ok(State::Pipelines);
        }

        // Load more check (now second-to-last if present)
        if next_page_token.is_some() && selection == labels.len() - 2 {
            let page = client
                .fetch_pipeline_workflows_page(pipeline_id, next_page_token.as_deref())
                .await?;
            items.extend(page.items);
            next_page_token = page.next_page_token;
            continue;
        }

        let wf = &items[selection - 1]; // -1 for top back entry
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

    let back_label = ".. (back to workflows)";

    loop {
        let mut labels: Vec<String> = vec![back_label.to_string()];
        labels.extend(items.iter().map(format_job_item));
        if next_page_token.is_some() {
            labels.push("▼ Load more...".to_string());
        }
        labels.push(back_label.to_string());

        let selection = Select::new()
            .with_prompt("Select a job")
            .items(&labels)
            .default(1)
            .interact_opt()?;

        let selection = match selection {
            Some(s) => s,
            None => return Ok(State::Done),
        };

        // back (top or bottom)
        if selection == 0 || selection == labels.len() - 1 {
            if pipeline_id.is_empty() {
                return Ok(State::Done);
            }
            return Ok(State::Workflows {
                pipeline_number,
                pipeline_id: pipeline_id.to_string(),
            });
        }

        // Load more check (second-to-last if present)
        if next_page_token.is_some() && selection == labels.len() - 2 {
            let page = client
                .fetch_workflow_jobs_page(workflow_id, next_page_token.as_deref())
                .await?;
            items.extend(page.items);
            next_page_token = page.next_page_token;
            continue;
        }

        let job = &items[selection - 1];
        if let Some(job_number) = job.job_number {
            let detail = client.fetch_job_detail(job_number).await?;
            let steps = match detail.steps {
                Some(ref s) if !s.is_empty() => s,
                _ => {
                    println!("No steps found for this job.");
                    continue;
                }
            };

            // Parallel job: any step has >1 actions
            if steps.first().is_some_and(|s| s.actions.len() > 1) {
                return Ok(State::Nodes {
                    job_number,
                    detail,
                    workflow_id: workflow_id.to_string(),
                    pipeline_number,
                    pipeline_id: pipeline_id.to_string(),
                });
            } else {
                return Ok(State::Steps {
                    job_number,
                    detail,
                    node_index: None,
                    workflow_id: workflow_id.to_string(),
                    pipeline_number,
                    pipeline_id: pipeline_id.to_string(),
                });
            }
        } else {
            println!("This job has no job number (may be pending or blocked).");
        }
        continue;
    }
}

async fn select_node(
    _client: &CircleCiClient,
    job_number: u64,
    detail: JobDetail,
    workflow_id: &str,
    pipeline_number: u64,
    pipeline_id: &str,
) -> Result<State> {
    let steps = match detail.steps {
        Some(ref s) => s,
        None => {
            return Ok(State::Jobs {
                workflow_id: workflow_id.to_string(),
                pipeline_number,
                pipeline_id: pipeline_id.to_string(),
            });
        }
    };

    let parallelism = steps.first().map(|s| s.actions.len()).unwrap_or(0);
    if parallelism == 0 {
        return Ok(State::Jobs {
            workflow_id: workflow_id.to_string(),
            pipeline_number,
            pipeline_id: pipeline_id.to_string(),
        });
    }
    let back_label = ".. (back to jobs)";

    let mut labels: Vec<String> = vec![back_label.to_string()];
    for i in 0..parallelism {
        labels.push(format_node_item(steps, i));
    }
    labels.push(back_label.to_string());

    let selection = Select::new()
        .with_prompt("Select a node")
        .items(&labels)
        .default(1)
        .interact_opt()?;

    let selection = match selection {
        Some(s) => s,
        None => return Ok(State::Done),
    };

    // back (top or bottom) → Jobs (discard detail, will re-fetch on next selection)
    if selection == 0 || selection == labels.len() - 1 {
        return Ok(State::Jobs {
            workflow_id: workflow_id.to_string(),
            pipeline_number,
            pipeline_id: pipeline_id.to_string(),
        });
    }

    let node_index = selection - 1;
    Ok(State::Steps {
        job_number,
        detail,
        node_index: Some(node_index),
        workflow_id: workflow_id.to_string(),
        pipeline_number,
        pipeline_id: pipeline_id.to_string(),
    })
}

async fn select_step(
    client: &CircleCiClient,
    job_number: u64,
    detail: JobDetail,
    node_index: Option<usize>,
    workflow_id: &str,
    pipeline_number: u64,
    pipeline_id: &str,
) -> Result<State> {
    let steps = match detail.steps {
        Some(ref s) => s,
        None => {
            return Ok(State::Jobs {
                workflow_id: workflow_id.to_string(),
                pipeline_number,
                pipeline_id: pipeline_id.to_string(),
            });
        }
    };

    let back_label = match node_index {
        Some(_) => ".. (back to nodes)",
        None => ".. (back to jobs)",
    };

    loop {
        let mut labels: Vec<String> = vec![back_label.to_string()];
        match node_index {
            Some(ni) => {
                labels.extend(steps.iter().map(|s| format_step_item_for_node(s, ni)));
            }
            None => {
                labels.extend(steps.iter().map(format_step_item));
            }
        }
        labels.push(back_label.to_string());

        let selection = Select::new()
            .with_prompt("Select a step")
            .items(&labels)
            .default(1)
            .interact_opt()?;

        let selection = match selection {
            Some(s) => s,
            None => return Ok(State::Done),
        };

        // back (top or bottom)
        if selection == 0 || selection == labels.len() - 1 {
            return match node_index {
                Some(_) => Ok(State::Nodes {
                    job_number,
                    detail,
                    workflow_id: workflow_id.to_string(),
                    pipeline_number,
                    pipeline_id: pipeline_id.to_string(),
                }),
                None => Ok(State::Jobs {
                    workflow_id: workflow_id.to_string(),
                    pipeline_number,
                    pipeline_id: pipeline_id.to_string(),
                }),
            };
        }

        let step_index = selection - 1;
        let step = &steps[step_index];
        let action_index = node_index.unwrap_or(0);
        let Some(action) = step.actions.get(action_index) else {
            continue;
        };

        match show_log(client, &detail, step, action, action_index).await? {
            LogAction::Back => continue,
            LogAction::Exit => return Ok(State::Done),
        }
    }
}

async fn show_log(
    client: &CircleCiClient,
    detail: &JobDetail,
    step: &Step,
    action: &Action,
    node_index: usize,
) -> Result<LogAction> {
    let job_number = detail.build_num.unwrap_or(0);
    let log = match LogSource::from_action(action, job_number) {
        Some(source) => match client.fetch_log(&source).await {
            Ok(content) => content,
            Err(e) => format!("(failed to fetch log: {})", e),
        },
        None => String::new(),
    };

    crate::output::print_node_log(detail, step, action, node_index, &log)?;

    let selection = Select::new()
        .with_prompt("Log view")
        .items(&["Back to steps", "Exit"])
        .default(0)
        .clear(false)
        .interact_opt()?;

    match selection {
        Some(0) => Ok(LogAction::Back),
        _ => Ok(LogAction::Exit),
    }
}

// --- Aggregate helpers ---

fn aggregate_node_status(steps: &[Step], node_index: usize) -> String {
    let statuses: Vec<&str> = steps
        .iter()
        .filter_map(|s| s.actions.get(node_index))
        .map(|a| a.status.as_str())
        .collect();

    if statuses.iter().any(|s| *s == "failed" || *s == "timedout") {
        "failed".to_string()
    } else if statuses.iter().all(|s| *s == "success") {
        "success".to_string()
    } else if statuses.contains(&"running") {
        "running".to_string()
    } else {
        statuses.first().copied().unwrap_or("-").to_string()
    }
}

fn aggregate_node_duration(steps: &[Step], node_index: usize) -> Option<u64> {
    let mut sum: u64 = 0;
    let mut any = false;
    for step in steps {
        #[allow(clippy::collapsible_if)]
        if let Some(action) = step.actions.get(node_index) {
            if let Some(ms) = action.run_time_millis {
                sum += ms;
                any = true;
            }
        }
    }
    if any { Some(sum) } else { None }
}

// --- Format helpers ---

fn format_node_item(steps: &[Step], node_index: usize) -> String {
    let status = aggregate_node_status(steps, node_index);
    let duration = crate::output::format_duration(aggregate_node_duration(steps, node_index));
    format!(
        "node {:<4} [{}] {}",
        node_index,
        colorize_status_padded(&status, 7),
        duration,
    )
}

fn format_step_item_for_node(step: &Step, node_index: usize) -> String {
    let Some(action) = step.actions.get(node_index) else {
        return format!("[{:<7}] {:<40} -", "-", step.name);
    };
    let duration = crate::output::format_duration(action.run_time_millis);
    format!(
        "[{}] {:<40} {}",
        colorize_status_padded(&action.status, 7),
        step.name,
        duration,
    )
}

fn format_step_item(step: &Step) -> String {
    let Some(action) = step.actions.first() else {
        return format!("[{:<7}] {:<40} -", "-", step.name);
    };
    let duration = crate::output::format_duration(action.run_time_millis);
    format!(
        "[{}] {:<40} {}",
        colorize_status_padded(&action.status, 7),
        step.name,
        duration,
    )
}

/// Pad status to `width` visible characters, then colorize.
fn colorize_status_padded(status: &str, width: usize) -> String {
    let padded = format!("{:<width$}", status, width = width);
    colorize_status(&padded)
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

    fn make_action(name: &str, status: &str, millis: Option<u64>) -> Action {
        Action {
            name: name.to_string(),
            status: status.to_string(),
            run_time_millis: millis,
            output_url: None,
            step: None,
            index: None,
        }
    }

    fn make_step_with_actions(name: &str, actions: Vec<Action>) -> Step {
        Step {
            name: name.to_string(),
            actions,
        }
    }

    // --- format_step_item tests (non-parallel only) ---

    #[test]
    fn format_step_item_success() {
        colored::control::set_override(false);
        let step =
            make_step_with_actions("Build", vec![make_action("Build", "success", Some(5000))]);
        let result = format_step_item(&step);
        assert!(result.contains("success"));
        assert!(result.contains("Build"));
        assert!(result.contains("5s"));
    }

    #[test]
    fn format_step_item_failed() {
        colored::control::set_override(false);
        let step = make_step_with_actions("Test", vec![make_action("Test", "failed", Some(12000))]);
        let result = format_step_item(&step);
        assert!(result.contains("failed"));
        assert!(result.contains("12s"));
    }

    #[test]
    fn format_step_item_no_duration() {
        colored::control::set_override(false);
        let step = make_step_with_actions("Setup", vec![make_action("Setup", "success", None)]);
        let result = format_step_item(&step);
        assert!(result.contains("-"));
    }

    // --- format_step_item_for_node tests ---

    #[test]
    fn format_step_item_for_node_shows_specific_node() {
        colored::control::set_override(false);
        let step = make_step_with_actions(
            "RSpec",
            vec![
                make_action("node 0", "success", Some(10000)),
                make_action("node 1", "failed", Some(8000)),
            ],
        );
        let result = format_step_item_for_node(&step, 1);
        assert!(result.contains("failed"));
        assert!(result.contains("RSpec"));
        assert!(result.contains("8s"));
    }

    #[test]
    fn format_step_item_for_node_first_node() {
        colored::control::set_override(false);
        let step = make_step_with_actions(
            "Build",
            vec![
                make_action("node 0", "success", Some(5000)),
                make_action("node 1", "success", Some(6000)),
            ],
        );
        let result = format_step_item_for_node(&step, 0);
        assert!(result.contains("success"));
        assert!(result.contains("5s"));
    }

    // --- format_node_item tests (aggregate) ---

    #[test]
    fn format_node_item_all_success() {
        colored::control::set_override(false);
        let steps = vec![
            make_step_with_actions(
                "Build",
                vec![
                    make_action("node 0", "success", Some(5000)),
                    make_action("node 1", "success", Some(6000)),
                ],
            ),
            make_step_with_actions(
                "Test",
                vec![
                    make_action("node 0", "success", Some(10000)),
                    make_action("node 1", "success", Some(12000)),
                ],
            ),
        ];
        let result = format_node_item(&steps, 0);
        assert!(result.contains("node 0"));
        assert!(result.contains("success"));
        assert!(result.contains("15s")); // 5000+10000
    }

    #[test]
    fn format_node_item_any_failed() {
        colored::control::set_override(false);
        let steps = vec![
            make_step_with_actions(
                "Build",
                vec![
                    make_action("node 0", "success", Some(5000)),
                    make_action("node 1", "success", Some(6000)),
                ],
            ),
            make_step_with_actions(
                "Test",
                vec![
                    make_action("node 0", "success", Some(10000)),
                    make_action("node 1", "failed", Some(8000)),
                ],
            ),
        ];
        let result = format_node_item(&steps, 1);
        assert!(result.contains("node 1"));
        assert!(result.contains("failed"));
        assert!(result.contains("14s")); // 6000+8000
    }

    #[test]
    fn format_node_item_no_duration() {
        colored::control::set_override(false);
        let steps = vec![make_step_with_actions(
            "Build",
            vec![
                make_action("node 0", "running", None),
                make_action("node 1", "running", None),
            ],
        )];
        let result = format_node_item(&steps, 0);
        assert!(result.contains("running"));
        assert!(result.contains("-"));
    }

    // --- aggregate_node_status tests ---

    #[test]
    fn aggregate_status_all_success() {
        let steps = vec![
            make_step_with_actions("s1", vec![make_action("n0", "success", None)]),
            make_step_with_actions("s2", vec![make_action("n0", "success", None)]),
        ];
        assert_eq!(aggregate_node_status(&steps, 0), "success");
    }

    #[test]
    fn aggregate_status_any_failed() {
        let steps = vec![
            make_step_with_actions("s1", vec![make_action("n0", "success", None)]),
            make_step_with_actions("s2", vec![make_action("n0", "failed", None)]),
        ];
        assert_eq!(aggregate_node_status(&steps, 0), "failed");
    }

    #[test]
    fn aggregate_status_any_running() {
        let steps = vec![
            make_step_with_actions("s1", vec![make_action("n0", "success", None)]),
            make_step_with_actions("s2", vec![make_action("n0", "running", None)]),
        ];
        assert_eq!(aggregate_node_status(&steps, 0), "running");
    }

    #[test]
    fn aggregate_status_timedout() {
        let steps = vec![
            make_step_with_actions("s1", vec![make_action("n0", "success", None)]),
            make_step_with_actions("s2", vec![make_action("n0", "timedout", None)]),
        ];
        assert_eq!(aggregate_node_status(&steps, 0), "failed");
    }

    // --- aggregate_node_status: priority & edge cases ---

    #[test]
    fn aggregate_status_failed_takes_priority_over_running() {
        let steps = vec![
            make_step_with_actions("s1", vec![make_action("n0", "running", None)]),
            make_step_with_actions("s2", vec![make_action("n0", "failed", None)]),
        ];
        assert_eq!(aggregate_node_status(&steps, 0), "failed");
    }

    #[test]
    fn aggregate_status_empty_steps() {
        let steps: Vec<Step> = vec![];
        // Empty statuses → .all(success) is vacuously true → "success"
        // This path is unreachable in practice (select_node ensures steps exist)
        assert_eq!(aggregate_node_status(&steps, 0), "success");
    }

    #[test]
    fn aggregate_status_node_index_out_of_range() {
        let steps = vec![make_step_with_actions(
            "s1",
            vec![make_action("n0", "success", None)],
        )];
        // node_index=5 → filter_map yields nothing → vacuously "success"
        // Unreachable in practice; node_index is always within parallelism range
        assert_eq!(aggregate_node_status(&steps, 5), "success");
    }

    #[test]
    fn aggregate_status_cancelled_only() {
        let steps = vec![
            make_step_with_actions("s1", vec![make_action("n0", "canceled", None)]),
            make_step_with_actions("s2", vec![make_action("n0", "canceled", None)]),
        ];
        // Not all success, not failed, not running → falls to first status
        assert_eq!(aggregate_node_status(&steps, 0), "canceled");
    }

    #[test]
    fn aggregate_status_infrastructure_fail() {
        // infrastructure_fail is not caught by aggregate (only "failed"/"timedout")
        // but it's not "success" either, so running check applies if present
        let steps = vec![make_step_with_actions(
            "s1",
            vec![make_action("n0", "infrastructure_fail", None)],
        )];
        // Not failed/timedout, not all success, not running → first status
        assert_eq!(aggregate_node_status(&steps, 0), "infrastructure_fail");
    }

    #[test]
    fn aggregate_status_single_step() {
        let steps = vec![make_step_with_actions(
            "s1",
            vec![make_action("n0", "success", None)],
        )];
        assert_eq!(aggregate_node_status(&steps, 0), "success");
    }

    // --- aggregate_node_duration tests ---

    #[test]
    fn aggregate_duration_sums() {
        let steps = vec![
            make_step_with_actions("s1", vec![make_action("n0", "success", Some(5000))]),
            make_step_with_actions("s2", vec![make_action("n0", "success", Some(3000))]),
        ];
        assert_eq!(aggregate_node_duration(&steps, 0), Some(8000));
    }

    #[test]
    fn aggregate_duration_partial_none() {
        let steps = vec![
            make_step_with_actions("s1", vec![make_action("n0", "success", Some(5000))]),
            make_step_with_actions("s2", vec![make_action("n0", "success", None)]),
        ];
        assert_eq!(aggregate_node_duration(&steps, 0), Some(5000));
    }

    #[test]
    fn aggregate_duration_all_none() {
        let steps = vec![
            make_step_with_actions("s1", vec![make_action("n0", "success", None)]),
            make_step_with_actions("s2", vec![make_action("n0", "success", None)]),
        ];
        assert_eq!(aggregate_node_duration(&steps, 0), None);
    }

    #[test]
    fn aggregate_duration_empty_steps() {
        let steps: Vec<Step> = vec![];
        assert_eq!(aggregate_node_duration(&steps, 0), None);
    }

    #[test]
    fn aggregate_duration_node_index_out_of_range() {
        let steps = vec![make_step_with_actions(
            "s1",
            vec![make_action("n0", "success", Some(5000))],
        )];
        assert_eq!(aggregate_node_duration(&steps, 5), None);
    }

    #[test]
    fn aggregate_duration_zero_values() {
        let steps = vec![
            make_step_with_actions("s1", vec![make_action("n0", "success", Some(0))]),
            make_step_with_actions("s2", vec![make_action("n0", "success", Some(0))]),
        ];
        // Zero is still a valid duration → Some(0)
        assert_eq!(aggregate_node_duration(&steps, 0), Some(0));
    }

    #[test]
    fn aggregate_duration_single_step() {
        let steps = vec![make_step_with_actions(
            "s1",
            vec![make_action("n0", "success", Some(7500))],
        )];
        assert_eq!(aggregate_node_duration(&steps, 0), Some(7500));
    }

    // --- format_step_item_for_node: additional cases ---

    #[test]
    fn format_step_item_for_node_no_duration() {
        colored::control::set_override(false);
        let step = make_step_with_actions(
            "Setup",
            vec![
                make_action("node 0", "success", None),
                make_action("node 1", "success", None),
            ],
        );
        let result = format_step_item_for_node(&step, 0);
        assert!(result.contains("Setup"));
        assert!(result.contains("-"));
    }

    #[test]
    fn format_step_item_for_node_running_status() {
        colored::control::set_override(false);
        let step = make_step_with_actions(
            "Test",
            vec![
                make_action("node 0", "running", Some(3000)),
                make_action("node 1", "success", Some(5000)),
            ],
        );
        let result = format_step_item_for_node(&step, 0);
        assert!(result.contains("running"));
        assert!(result.contains("3s"));
        // Ensure it does NOT show node 1's duration
        assert!(!result.contains("5s"));
    }

    // --- format_step_item: additional cases ---

    #[test]
    fn format_step_item_running() {
        colored::control::set_override(false);
        let step = make_step_with_actions(
            "Deploy",
            vec![make_action("Deploy", "running", Some(30000))],
        );
        let result = format_step_item(&step);
        assert!(result.contains("running"));
        assert!(result.contains("30s"));
    }

    // --- format_node_item: additional cases ---

    #[test]
    fn format_node_item_single_step() {
        colored::control::set_override(false);
        let steps = vec![make_step_with_actions(
            "Build",
            vec![
                make_action("node 0", "success", Some(5000)),
                make_action("node 1", "failed", Some(3000)),
            ],
        )];
        let result = format_node_item(&steps, 0);
        assert!(result.contains("node 0"));
        assert!(result.contains("success"));
        assert!(result.contains("5s"));

        let result = format_node_item(&steps, 1);
        assert!(result.contains("node 1"));
        assert!(result.contains("failed"));
        assert!(result.contains("3s"));
    }

    #[test]
    fn format_node_item_large_node_index() {
        colored::control::set_override(false);
        let mut actions: Vec<Action> = (0..20)
            .map(|i| make_action(&format!("node {}", i), "success", Some(1000)))
            .collect();
        actions[15] = make_action("node 15", "failed", Some(2000));
        let steps = vec![make_step_with_actions("RSpec", actions)];
        let result = format_node_item(&steps, 15);
        assert!(result.contains("node 15"));
        assert!(result.contains("failed"));
        assert!(result.contains("2s"));
    }

    // --- Safety tests: empty actions / out-of-range index ---

    #[test]
    fn format_step_item_empty_actions_no_panic() {
        colored::control::set_override(false);
        let step = make_step_with_actions("Empty", vec![]);
        let result = format_step_item(&step);
        assert!(result.contains("Empty"));
        assert!(result.contains("-"));
    }

    #[test]
    fn format_step_item_for_node_out_of_range_no_panic() {
        colored::control::set_override(false);
        let step =
            make_step_with_actions("Build", vec![make_action("node 0", "success", Some(1000))]);
        let result = format_step_item_for_node(&step, 5);
        assert!(result.contains("Build"));
        assert!(result.contains("-"));
    }

    #[test]
    fn format_node_item_empty_actions_no_panic() {
        colored::control::set_override(false);
        let steps = vec![make_step_with_actions("Empty", vec![])];
        let result = format_node_item(&steps, 0);
        // Should produce output without panicking (aggregates already use .get())
        assert!(result.contains("node 0"));
    }

    #[test]
    fn format_node_item_many_steps_mixed_status() {
        colored::control::set_override(false);
        let steps = vec![
            make_step_with_actions(
                "Spin up",
                vec![
                    make_action("n0", "success", Some(2000)),
                    make_action("n1", "success", Some(2000)),
                ],
            ),
            make_step_with_actions(
                "Checkout",
                vec![
                    make_action("n0", "success", Some(1000)),
                    make_action("n1", "success", Some(1000)),
                ],
            ),
            make_step_with_actions(
                "Test",
                vec![
                    make_action("n0", "success", Some(30000)),
                    make_action("n1", "failed", Some(25000)),
                ],
            ),
            make_step_with_actions(
                "Upload",
                vec![
                    make_action("n0", "success", Some(500)),
                    make_action("n1", "success", Some(500)),
                ],
            ),
        ];
        // node 0: all success, total = 2000+1000+30000+500 = 33500
        let result = format_node_item(&steps, 0);
        assert!(result.contains("success"));
        assert!(result.contains("33s"));

        // node 1: has failed in Test → aggregated as "failed", total = 2000+1000+25000+500 = 28500
        let result = format_node_item(&steps, 1);
        assert!(result.contains("failed"));
        assert!(result.contains("28s"));
    }
}
