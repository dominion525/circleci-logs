use colored::Colorize;
use regex::Regex;

use crate::models::*;

fn colorize_status(status: &str) -> String {
    match status {
        "success" => status.green().to_string(),
        "failed" | "timedout" | "infrastructure_fail" => status.red().to_string(),
        "running" => status.yellow().to_string(),
        "canceled" | "cancelled" => status.dimmed().to_string(),
        _ => status.to_string(),
    }
}

fn format_duration(millis: Option<u64>) -> String {
    match millis {
        Some(ms) => {
            let secs = ms / 1000;
            if secs >= 60 {
                format!("{}m{}s", secs / 60, secs % 60)
            } else {
                format!("{}s", secs)
            }
        }
        None => "-".to_string(),
    }
}

pub fn print_job_log(
    detail: &JobDetail,
    logs: &[(String, String)],
    errors_only: bool,
    grep: Option<&Regex>,
    json: bool,
) {
    if json {
        let output = serde_json::json!({
            "build_num": detail.build_num,
            "status": detail.status,
            "steps": detail.steps.as_ref().map(|steps| {
                steps.iter().map(|step| {
                    serde_json::json!({
                        "name": step.name,
                        "actions": step.actions.iter().map(|a| {
                            serde_json::json!({
                                "name": a.name,
                                "status": a.status,
                                "run_time_millis": a.run_time_millis,
                            })
                        }).collect::<Vec<_>>()
                    })
                }).collect::<Vec<_>>()
            }),
            "logs": logs.iter().map(|(name, content)| {
                serde_json::json!({
                    "step": name,
                    "output": content,
                })
            }).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
        return;
    }

    if let Some(ref wf) = detail.workflows {
        if let Some(ref wf_name) = wf.workflow_name {
            print!("Workflow: {}  ", wf_name);
        }
        if let Some(ref job_name) = wf.job_name {
            print!("Job: {}  ", job_name);
        }
        println!();
    }

    if let Some(ref status) = detail.status {
        println!("Status: {}", colorize_status(status));
    }
    println!();

    if let Some(ref steps) = detail.steps {
        for step in steps {
            for action in &step.actions {
                if errors_only && action.status == "success" {
                    continue;
                }
                println!(
                    "[{}] {} ({})",
                    colorize_status(&action.status),
                    step.name,
                    format_duration(action.run_time_millis)
                );
            }
        }
    }

    if !logs.is_empty() {
        println!();
        for (step_name, content) in logs {
            if content.is_empty() {
                continue;
            }
            let filtered: String = match grep {
                Some(re) => content
                    .lines()
                    .filter(|line| re.is_match(line))
                    .collect::<Vec<_>>()
                    .join("\n"),
                None => content.clone(),
            };
            if filtered.is_empty() {
                continue;
            }
            println!("--- {} ---", step_name.bold());
            println!("{}", filtered);
            println!();
        }
    }
}

pub fn print_workflow_jobs(jobs: &[WorkflowJob], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(jobs).unwrap());
        return;
    }

    println!(
        "{:<8} {:<30} {:<12} {:<20} {:<20}",
        "JOB#", "NAME", "STATUS", "STARTED", "STOPPED"
    );
    println!("{}", "-".repeat(90));
    for job in jobs {
        let job_num = job
            .job_number
            .map(|n| n.to_string())
            .unwrap_or_else(|| "-".to_string());
        let started = job.started_at.as_deref().unwrap_or("-");
        let stopped = job.stopped_at.as_deref().unwrap_or("-");
        println!(
            "{:<8} {:<30} {:<12} {:<20} {:<20}",
            job_num,
            job.name,
            colorize_status(&job.status),
            started,
            stopped
        );
    }
}

pub fn print_pipeline_workflows(workflows: &[PipelineWorkflow], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(workflows).unwrap());
        return;
    }

    println!(
        "{:<38} {:<25} {:<12} {:<20} {:<20}",
        "WORKFLOW ID", "NAME", "STATUS", "CREATED", "STOPPED"
    );
    println!("{}", "-".repeat(115));
    for wf in workflows {
        let created = wf.created_at.as_deref().unwrap_or("-");
        let stopped = wf.stopped_at.as_deref().unwrap_or("-");
        println!(
            "{:<38} {:<25} {:<12} {:<20} {:<20}",
            wf.id,
            wf.name,
            colorize_status(&wf.status),
            created,
            stopped
        );
    }
}
