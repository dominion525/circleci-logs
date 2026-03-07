use anyhow::Result;
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

fn filter_log_lines(content: &str, grep: Option<&Regex>) -> String {
    match grep {
        Some(re) => content
            .lines()
            .filter(|line| re.is_match(line))
            .collect::<Vec<_>>()
            .join("\n"),
        None => content.to_string(),
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

fn build_job_log_json(
    detail: &JobDetail,
    logs: &[(String, String)],
    errors_only: bool,
    grep: Option<&Regex>,
) -> serde_json::Value {
    serde_json::json!({
        "build_num": detail.build_num,
        "status": detail.status,
        "steps": detail.steps.as_ref().map(|steps| {
            steps.iter().filter(|step| {
                !errors_only || step.actions.iter().any(|a| a.status != "success")
            }).map(|step| {
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
            let filtered = filter_log_lines(content, grep);
            serde_json::json!({
                "step": name,
                "output": filtered,
            })
        }).collect::<Vec<_>>(),
    })
}

pub fn print_job_log(
    detail: &JobDetail,
    logs: &[(String, String)],
    errors_only: bool,
    grep: Option<&Regex>,
    json: bool,
) -> Result<()> {
    if json {
        let output = build_job_log_json(detail, logs, errors_only, grep);
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
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
            let filtered = filter_log_lines(content, grep);
            if filtered.is_empty() {
                continue;
            }
            println!("--- {} ---", step_name.bold());
            println!("{}", filtered);
            println!();
        }
    }
    Ok(())
}

pub fn print_workflow_jobs(jobs: &[WorkflowJob], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(jobs)?);
        return Ok(());
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
    Ok(())
}

pub fn print_pipeline_workflows(workflows: &[PipelineWorkflow], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(workflows)?);
        return Ok(());
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_none() {
        assert_eq!(format_duration(None), "-");
    }

    #[test]
    fn format_duration_zero() {
        assert_eq!(format_duration(Some(0)), "0s");
    }

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(Some(5000)), "5s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(Some(60000)), "1m0s");
    }

    #[test]
    fn format_duration_minutes_and_seconds() {
        assert_eq!(format_duration(Some(125000)), "2m5s");
    }

    #[test]
    fn colorize_status_values() {
        colored::control::set_override(false);
        assert_eq!(colorize_status("success"), "success");
        assert_eq!(colorize_status("failed"), "failed");
        assert_eq!(colorize_status("running"), "running");
        assert_eq!(colorize_status("cancelled"), "cancelled");
        assert_eq!(colorize_status("queued"), "queued");
    }

    #[test]
    fn filter_log_lines_no_grep() {
        let content = "line1\nline2\nline3";
        assert_eq!(filter_log_lines(content, None), content);
    }

    #[test]
    fn filter_log_lines_with_match() {
        let re = Regex::new("error").unwrap();
        let content = "info: ok\nerror: bad\ninfo: fine";
        assert_eq!(filter_log_lines(content, Some(&re)), "error: bad");
    }

    #[test]
    fn filter_log_lines_no_match() {
        let re = Regex::new("error").unwrap();
        let content = "info: ok\ninfo: fine";
        assert_eq!(filter_log_lines(content, Some(&re)), "");
    }

    // --- build_job_log_json tests ---

    fn make_detail(
        steps: Option<Vec<Step>>,
        status: Option<&str>,
        build_num: Option<u64>,
    ) -> JobDetail {
        JobDetail {
            steps,
            status: status.map(|s| s.to_string()),
            build_num,
            workflows: None,
        }
    }

    fn make_step(name: &str, actions: Vec<Action>) -> Step {
        Step {
            name: name.to_string(),
            actions,
        }
    }

    fn make_action(name: &str, status: &str, run_time_millis: Option<u64>) -> Action {
        Action {
            name: name.to_string(),
            status: status.to_string(),
            run_time_millis,
            output_url: None,
            step: None,
            index: None,
        }
    }

    #[test]
    fn build_job_log_json_normal() {
        let detail = make_detail(
            Some(vec![make_step(
                "build",
                vec![make_action("compile", "success", Some(3000))],
            )]),
            Some("success"),
            Some(42),
        );
        let logs = vec![("build".to_string(), "output line".to_string())];
        let val = build_job_log_json(&detail, &logs, false, None);

        assert_eq!(val["build_num"], 42);
        assert_eq!(val["status"], "success");
        assert!(val["steps"].is_array());
        let step = &val["steps"][0];
        assert_eq!(step["name"], "build");
        assert_eq!(step["actions"][0]["name"], "compile");
        assert_eq!(step["actions"][0]["status"], "success");
        assert_eq!(step["actions"][0]["run_time_millis"], 3000);
        assert_eq!(val["logs"][0]["step"], "build");
        assert_eq!(val["logs"][0]["output"], "output line");
    }

    #[test]
    fn build_job_log_json_steps_none() {
        let detail = make_detail(None, Some("failed"), Some(1));
        let val = build_job_log_json(&detail, &[], false, None);
        assert!(val["steps"].is_null());
    }

    #[test]
    fn build_job_log_json_empty_logs() {
        let detail = make_detail(None, None, None);
        let val = build_job_log_json(&detail, &[], false, None);
        assert_eq!(val["logs"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn build_job_log_json_errors_only() {
        let detail = make_detail(
            Some(vec![
                make_step("build", vec![make_action("compile", "success", Some(3000))]),
                make_step("test", vec![make_action("run tests", "failed", Some(5000))]),
            ]),
            Some("failed"),
            Some(99),
        );
        let logs = vec![];
        let val = build_job_log_json(&detail, &logs, true, None);

        let steps = val["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0]["name"], "test");
    }

    #[test]
    fn build_job_log_json_grep_filter() {
        let detail = make_detail(None, None, None);
        let logs = vec![("step1".to_string(), "ok line\nerror here\nfine".to_string())];
        let re = Regex::new("error").unwrap();
        let val = build_job_log_json(&detail, &logs, false, Some(&re));
        assert_eq!(val["logs"][0]["output"], "error here");
    }

    // --- smoke tests for print_* functions ---

    #[test]
    fn print_job_log_text_smoke() {
        let detail = make_detail(
            Some(vec![make_step(
                "test",
                vec![make_action("run tests", "success", Some(1000))],
            )]),
            Some("success"),
            Some(10),
        );
        let logs = vec![("test".to_string(), "all passed".to_string())];
        let result = print_job_log(&detail, &logs, false, None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn print_job_log_json_smoke() {
        let detail = make_detail(
            Some(vec![make_step(
                "test",
                vec![make_action("run tests", "success", Some(1000))],
            )]),
            Some("success"),
            Some(10),
        );
        let logs = vec![("test".to_string(), "all passed".to_string())];
        let result = print_job_log(&detail, &logs, false, None, true);
        assert!(result.is_ok());
    }

    #[test]
    fn print_workflow_jobs_empty_smoke() {
        assert!(print_workflow_jobs(&[], false).is_ok());
    }

    #[test]
    fn print_workflow_jobs_one_item_smoke() {
        let jobs = vec![WorkflowJob {
            id: "j1".to_string(),
            name: "build".to_string(),
            status: "success".to_string(),
            job_number: Some(5),
            job_type: None,
            started_at: Some("2024-01-01T00:00:00Z".to_string()),
            stopped_at: None,
        }];
        assert!(print_workflow_jobs(&jobs, false).is_ok());
        assert!(print_workflow_jobs(&jobs, true).is_ok());
    }

    #[test]
    fn print_pipeline_workflows_empty_smoke() {
        assert!(print_pipeline_workflows(&[], false).is_ok());
    }

    #[test]
    fn print_pipeline_workflows_one_item_smoke() {
        let wfs = vec![PipelineWorkflow {
            id: "wf-1".to_string(),
            name: "deploy".to_string(),
            status: "running".to_string(),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
            stopped_at: None,
            pipeline_number: Some(99),
        }];
        assert!(print_pipeline_workflows(&wfs, false).is_ok());
        assert!(print_pipeline_workflows(&wfs, true).is_ok());
    }
}
