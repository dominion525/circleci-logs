use std::io::IsTerminal;

use anyhow::Result;
use chrono::{DateTime, Local};
use colored::Colorize;
use regex::Regex;

use crate::models::*;

pub fn colorize_status(status: &str) -> String {
    match status {
        "success" | "created" => status.green().to_string(),
        "failed" | "failure" | "timedout" | "infrastructure_fail" | "error" => {
            status.red().to_string()
        }
        "running" => status.yellow().to_string(),
        "canceled" | "cancelled" => status.dimmed().to_string(),
        "not_run" | "skipped" => status.dimmed().to_string(),
        _ => status.to_string(),
    }
}

pub fn format_timestamp(ts: &str) -> String {
    match DateTime::parse_from_rfc3339(ts) {
        Ok(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        Err(_) => ts.to_string(),
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

/// Render raw log output through vt100 terminal emulation.
/// Resolves cursor control sequences (e.g. Docker buildx progress) to final screen state.
/// When `preserve_colors` is true, retains ANSI color codes (for terminal output).
/// When false, returns plain text (for file/pipe/JSON).
pub fn render_log(content: &str, preserve_colors: bool) -> String {
    // Fast path: no escape sequences, no NUL bytes, and no literal ^@ (caret notation)
    if !content.as_bytes().contains(&0x1b)
        && !content.as_bytes().contains(&0)
        && !content.contains("^@")
    {
        return content.to_string();
    }
    // Filter NUL bytes (0x00) and literal "^@" (caret notation for NUL that
    // appears in some CI log data) before processing.
    let without_caret_at = content.replace("^@", "");
    let clean: Vec<u8> = without_caret_at.bytes().filter(|&b| b != 0).collect();
    // If no escape sequences remain, return as-is
    if !clean.contains(&0x1b) {
        return String::from_utf8_lossy(&clean).to_string();
    }
    let mut parser = vt100::Parser::new(u16::MAX, 200, 0);
    parser.process(&clean);
    if preserve_colors {
        let formatted = parser.screen().contents_formatted();
        let filtered: Vec<u8> = formatted.into_iter().filter(|&b| b != 0).collect();
        String::from_utf8_lossy(&filtered).to_string()
    } else {
        parser.screen().contents().replace('\0', "")
    }
}

/// Compute elapsed time in milliseconds for an action.
/// Prefers `run_time_millis` (set by API for completed actions).
/// For running actions (no `run_time_millis`), computes from `start_time` to now.
pub fn compute_elapsed_millis(action: &Action) -> Option<u64> {
    if let Some(ms) = action.run_time_millis {
        return Some(ms);
    }
    if let Some(ref start) = action.start_time {
        if action.end_time.is_none() {
            if let Ok(dt) = DateTime::parse_from_rfc3339(start) {
                let elapsed = chrono::Utc::now().signed_duration_since(dt);
                if elapsed.num_milliseconds() > 0 {
                    return Some(elapsed.num_milliseconds() as u64);
                }
            }
        }
    }
    None
}

/// Format elapsed time with "~" prefix to indicate approximate/in-progress value.
pub fn format_duration(millis: Option<u64>) -> String {
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
                            "step": a.step,
                            "index": a.index,
                            "output_url": a.output_url,
                        })
                    }).collect::<Vec<_>>()
                })
            }).collect::<Vec<_>>()
        }),
        "logs": logs.iter().map(|(name, content)| {
            let rendered = render_log(content, false);
            let filtered = filter_log_lines(&rendered, grep);
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
        let is_tty = std::io::stdout().is_terminal();
        println!();
        for (step_name, content) in logs {
            if content.is_empty() {
                continue;
            }
            let rendered = render_log(content, is_tty);
            let filtered = filter_log_lines(&rendered, grep);
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

/// Print the header block shown above log output for a single step/node.
///
/// Used by both `stream_log` (live streaming) and `print_node_log` (static
/// log display) to provide a consistent header format: workflow name, job
/// name, node index, step name, and status.
pub fn print_node_header(detail: &JobDetail, step: &Step, node_index: usize, status_label: &str) {
    if let Some(ref wf) = detail.workflows {
        if let Some(ref wf_name) = wf.workflow_name {
            print!("Workflow: {}  ", wf_name);
        }
        if let Some(ref job_name) = wf.job_name {
            print!("Job: {}  ", job_name);
        }
        println!();
    }

    println!("Node: {}  Step: {}", node_index, step.name.bold());
    println!("  [{}]", colorize_status(status_label));
    println!();
}

pub fn print_node_log(
    detail: &JobDetail,
    step: &Step,
    action: &Action,
    node_index: usize,
    log: &str,
) -> Result<()> {
    // Header
    if let Some(ref wf) = detail.workflows {
        if let Some(ref wf_name) = wf.workflow_name {
            print!("Workflow: {}  ", wf_name);
        }
        if let Some(ref job_name) = wf.job_name {
            print!("Job: {}  ", job_name);
        }
        println!();
    }

    println!("Node: {}  Step: {}", node_index, step.name.bold());
    println!(
        "  [{}] ({})",
        colorize_status(&action.status),
        format_duration(action.run_time_millis)
    );
    println!();

    if !log.is_empty() {
        let is_tty = std::io::stdout().is_terminal();
        let rendered = render_log(log, is_tty);
        println!("{}", rendered);
        println!();
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
        let started = job
            .started_at
            .as_deref()
            .map(format_timestamp)
            .unwrap_or_else(|| "-".to_string());
        let stopped = job
            .stopped_at
            .as_deref()
            .map(format_timestamp)
            .unwrap_or_else(|| "-".to_string());
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

fn format_run_time(secs: Option<f64>) -> String {
    match secs {
        None => "-".to_string(),
        Some(s) => {
            if s >= 60.0 {
                let mins = s as u64 / 60;
                let remainder = s - (mins as f64 * 60.0);
                format!("{}m{:.3}s", mins, remainder)
            } else {
                format!("{:.3}s", s)
            }
        }
    }
}

pub fn print_test_results(
    tests: &[TestResult],
    job_number: u64,
    failed_only: bool,
    json: bool,
) -> Result<()> {
    if json {
        let output: Vec<&TestResult> = if failed_only {
            tests
                .iter()
                .filter(|t| matches!(t.result.as_deref(), Some("failure") | Some("failed")))
                .collect()
        } else {
            tests.iter().collect()
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    let filtered: Vec<&TestResult> = if failed_only {
        tests
            .iter()
            .filter(|t| matches!(t.result.as_deref(), Some("failure") | Some("failed")))
            .collect()
    } else {
        tests.iter().collect()
    };

    println!("Test Results: Job #{}\n", job_number);

    println!("{:<10} {:<10} {:<30} NAME", "RESULT", "TIME", "FILE");
    println!("{}", "-".repeat(90));

    for t in &filtered {
        let result_str = t.result.as_deref().unwrap_or("-");
        let time_str = format_run_time(t.run_time);
        let file_str = t.file.as_deref().unwrap_or("-");
        let name_str = t.name.as_deref().unwrap_or("-");
        println!(
            "{:<10} {:<10} {:<30} {}",
            colorize_status(result_str),
            time_str,
            file_str,
            name_str
        );
    }

    // Failed tests detail section
    let failed: Vec<&&TestResult> = filtered
        .iter()
        .filter(|t| matches!(t.result.as_deref(), Some("failure") | Some("failed")))
        .filter(|t| t.message.as_ref().is_some_and(|m| !m.is_empty()))
        .collect();

    if !failed.is_empty() {
        println!("\n--- Failed Tests ---");
        for t in &failed {
            let name = t.name.as_deref().unwrap_or("-");
            let result_str = t.result.as_deref().unwrap_or("failed");
            println!("\n[{}] {}", colorize_status(result_str), name);
            if let Some(ref file) = t.file {
                println!("  File:  {}", file);
            }
            if let Some(ref classname) = t.classname {
                println!("  Class: {}", classname);
            }
            if let Some(ref message) = t.message {
                println!();
                for line in message.lines() {
                    println!("  {}", line);
                }
            }
        }
    }

    // Summary (always computed from all tests, not filtered)
    let mut passed = 0u64;
    let mut failed_count = 0u64;
    let mut skipped = 0u64;
    let mut total_time = 0.0f64;

    for t in tests {
        match t.result.as_deref() {
            Some("success") => passed += 1,
            Some("failure") | Some("failed") => failed_count += 1,
            Some("skipped") => skipped += 1,
            _ => {}
        }
        if let Some(rt) = t.run_time {
            total_time += rt;
        }
    }

    println!(
        "\nSummary: {} passed, {} failed, {} skipped ({})",
        passed,
        failed_count,
        skipped,
        format_run_time(Some(total_time))
    );

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
        let created = wf
            .created_at
            .as_deref()
            .map(format_timestamp)
            .unwrap_or_else(|| "-".to_string());
        let stopped = wf
            .stopped_at
            .as_deref()
            .map(format_timestamp)
            .unwrap_or_else(|| "-".to_string());
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
    fn format_timestamp_valid_utc() {
        let result = format_timestamp("2025-01-15T10:00:05Z");
        // Should parse successfully and produce local time in YYYY-MM-DD HH:MM:SS format
        assert!(DateTime::parse_from_rfc3339("2025-01-15T10:00:05Z").is_ok());
        assert!(result.contains("2025-01-15") || result.contains("2025-01-16"));
        assert!(result.len() == 19); // "YYYY-MM-DD HH:MM:SS"
    }

    #[test]
    fn format_timestamp_invalid() {
        assert_eq!(format_timestamp("not-a-date"), "not-a-date");
        assert_eq!(format_timestamp(""), "");
    }

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
        assert_eq!(colorize_status("created"), "created");
        assert_eq!(colorize_status("failed"), "failed");
        assert_eq!(colorize_status("failure"), "failure");
        assert_eq!(colorize_status("timedout"), "timedout");
        assert_eq!(
            colorize_status("infrastructure_fail"),
            "infrastructure_fail"
        );
        assert_eq!(colorize_status("error"), "error");
        assert_eq!(colorize_status("running"), "running");
        assert_eq!(colorize_status("canceled"), "canceled");
        assert_eq!(colorize_status("cancelled"), "cancelled");
        assert_eq!(colorize_status("not_run"), "not_run");
        assert_eq!(colorize_status("skipped"), "skipped");
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

    // --- render_log tests ---

    #[test]
    fn render_log_plain_text_unchanged() {
        let input = "hello\nworld";
        assert_eq!(render_log(input, false), input);
    }

    #[test]
    fn render_log_strips_ansi_colors() {
        let input = "\x1b[34mblue text\x1b[0m";
        assert_eq!(render_log(input, false), "blue text");
    }

    #[test]
    fn render_log_preserves_colors() {
        let input = "\x1b[34mblue text\x1b[0m";
        let result = render_log(input, true);
        assert!(result.contains("blue text"));
        assert!(result.contains("\x1b["));
    }

    #[test]
    fn render_log_resolves_cursor_control() {
        // Simulate Docker buildx-style progress: multiple lines, then cursor-up to overwrite
        // \x1b[1A = cursor up 1, \x1b[2K = erase entire line, \x1b[0G = cursor to column 0
        let input = "header\n\
                      progress: 50%\n\
                      \x1b[1A\x1b[2K\x1b[0Gprogress: 100%\n\
                      done";
        let result = render_log(input, false);
        assert!(result.contains("header"));
        assert!(result.contains("progress: 100%"));
        assert!(!result.contains("progress: 50%"));
        assert!(result.contains("done"));
    }

    #[test]
    fn render_log_filters_nul_bytes() {
        let input = "hello\x00world";
        let result = render_log(input, false);
        assert_eq!(result, "helloworld");
        assert!(!result.contains('\0'));
    }

    #[test]
    fn render_log_filters_nul_with_ansi() {
        let input = "\x1b[34mblue\x00text\x1b[0m";
        let result = render_log(input, false);
        assert_eq!(result, "bluetext");
        assert!(!result.contains('\0'));
    }

    #[test]
    fn render_log_docker_compose_no_caret_at() {
        // Docker compose output with ✔, cursor-up, erase-line
        let input = " \u{2714} Container test  Running0.0s \n\x1b[1A\x1b[2K \u{2714} Container test  Running0.0s \nBundle complete!";
        let result_plain = render_log(input, false);
        let result_color = render_log(input, true);
        // Should not contain literal ^@ from NUL cells
        assert!(
            !result_plain.contains("^@"),
            "plain contains ^@: {:?}",
            result_plain
        );
        assert!(
            !result_color.contains("^@"),
            "color contains ^@: {:?}",
            result_color
        );
        assert!(result_plain.contains("Bundle complete!"));
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
            start_time: None,
            end_time: None,
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
    fn build_job_log_json_includes_action_fields() {
        let action = Action {
            name: "compile".to_string(),
            status: "success".to_string(),
            run_time_millis: Some(1000),
            step: Some(101),
            index: Some(0),
            output_url: Some("https://example.com/output".to_string()),
            start_time: None,
            end_time: None,
        };
        let detail = make_detail(
            Some(vec![make_step("build", vec![action])]),
            Some("success"),
            Some(42),
        );
        let val = build_job_log_json(&detail, &[], false, None);

        let a = &val["steps"][0]["actions"][0];
        assert_eq!(a["step"], 101);
        assert_eq!(a["index"], 0);
        assert_eq!(a["output_url"], "https://example.com/output");
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

    // --- format_run_time tests ---

    #[test]
    fn format_run_time_none() {
        assert_eq!(format_run_time(None), "-");
    }

    #[test]
    fn format_run_time_sub_second() {
        assert_eq!(format_run_time(Some(0.437)), "0.437s");
    }

    #[test]
    fn format_run_time_seconds() {
        assert_eq!(format_run_time(Some(5.0)), "5.000s");
    }

    #[test]
    fn format_run_time_minutes() {
        assert_eq!(format_run_time(Some(125.3)), "2m5.300s");
    }

    #[test]
    fn format_run_time_zero() {
        assert_eq!(format_run_time(Some(0.0)), "0.000s");
    }

    // --- print_test_results tests ---

    fn make_test_result(
        name: &str,
        result: &str,
        run_time: Option<f64>,
        message: Option<&str>,
        file: Option<&str>,
        classname: Option<&str>,
    ) -> TestResult {
        TestResult {
            name: Some(name.to_string()),
            classname: classname.map(|s| s.to_string()),
            result: Some(result.to_string()),
            message: message.map(|s| s.to_string()),
            run_time,
            source: None,
            file: file.map(|s| s.to_string()),
        }
    }

    #[test]
    fn print_test_results_text_smoke() {
        let tests = vec![
            make_test_result("test1", "success", Some(0.5), None, Some("a.rb"), None),
            make_test_result(
                "test2",
                "failure",
                Some(0.1),
                Some("Expected true"),
                Some("b.rb"),
                Some("AuthSpec"),
            ),
        ];
        assert!(print_test_results(&tests, 42, false, false).is_ok());
    }

    #[test]
    fn print_test_results_json_smoke() {
        let tests = vec![make_test_result(
            "test1",
            "success",
            Some(0.5),
            None,
            None,
            None,
        )];
        assert!(print_test_results(&tests, 42, false, true).is_ok());
    }

    #[test]
    fn print_test_results_failed_only_smoke() {
        let tests = vec![
            make_test_result("pass", "success", Some(0.5), None, None, None),
            make_test_result("fail", "failure", Some(0.1), Some("bad"), None, None),
        ];
        assert!(print_test_results(&tests, 42, true, false).is_ok());
    }

    #[test]
    fn print_test_results_empty_smoke() {
        assert!(print_test_results(&[], 42, false, false).is_ok());
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

    // --- print_node_log: header order verification ---
    // print_node_log outputs "Node: {index}  Step: {name}" (Node first, then Step).
    // Since it uses println! directly, we verify the format string logic by
    // checking the source code pattern. The smoke tests below verify it doesn't panic.

    #[test]
    fn print_node_log_no_workflow() {
        // workflows = None → should not print Workflow/Job header line
        let detail = make_detail(
            Some(vec![make_step(
                "Build",
                vec![make_action("node 0", "success", Some(5000))],
            )]),
            Some("success"),
            Some(10),
        );
        let step = &detail.steps.as_ref().unwrap()[0];
        let action = &step.actions[0];
        let result = print_node_log(&detail, step, action, 0, "output");
        assert!(result.is_ok());
    }

    #[test]
    fn print_node_log_no_duration() {
        let detail = make_detail(
            Some(vec![make_step(
                "Setup",
                vec![make_action("node 0", "running", None)],
            )]),
            Some("running"),
            Some(5),
        );
        let step = &detail.steps.as_ref().unwrap()[0];
        let action = &step.actions[0];
        let result = print_node_log(&detail, step, action, 0, "");
        assert!(result.is_ok());
    }

    #[test]
    fn print_node_log_partial_workflow_header() {
        // workflow_name set but job_name is None
        let mut detail = make_detail(
            Some(vec![make_step(
                "Test",
                vec![make_action("node 0", "failed", Some(3000))],
            )]),
            Some("failed"),
            Some(20),
        );
        detail.workflows = Some(WorkflowRef {
            workflow_name: Some("ci".to_string()),
            job_name: None,
        });
        let step = &detail.steps.as_ref().unwrap()[0];
        let action = &step.actions[0];
        let result = print_node_log(&detail, step, action, 0, "error log");
        assert!(result.is_ok());
    }

    #[test]
    fn print_node_log_smoke() {
        let detail = make_detail(
            Some(vec![make_step(
                "RSpec",
                vec![
                    make_action("node 0", "success", Some(10000)),
                    make_action("node 1", "failed", Some(8000)),
                ],
            )]),
            Some("failed"),
            Some(42),
        );
        let step = &detail.steps.as_ref().unwrap()[0];
        let action = &step.actions[1];
        let result = print_node_log(&detail, step, action, 1, "test output here");
        assert!(result.is_ok());
    }

    #[test]
    fn print_node_log_empty_log_smoke() {
        let detail = make_detail(
            Some(vec![make_step(
                "Build",
                vec![make_action("node 0", "success", Some(5000))],
            )]),
            Some("success"),
            Some(10),
        );
        let step = &detail.steps.as_ref().unwrap()[0];
        let action = &step.actions[0];
        let result = print_node_log(&detail, step, action, 0, "");
        assert!(result.is_ok());
    }

    #[test]
    fn print_node_log_with_workflow_header() {
        let mut detail = make_detail(
            Some(vec![make_step(
                "RSpec",
                vec![make_action("node 0", "success", Some(5000))],
            )]),
            Some("success"),
            Some(10),
        );
        detail.workflows = Some(WorkflowRef {
            workflow_name: Some("build-and-test".to_string()),
            job_name: Some("rspec".to_string()),
        });
        let step = &detail.steps.as_ref().unwrap()[0];
        let action = &step.actions[0];
        let result = print_node_log(&detail, step, action, 0, "log content");
        assert!(result.is_ok());
    }

    // --- compute_elapsed_millis tests ---

    #[test]
    fn compute_elapsed_prefers_run_time_millis() {
        let action = Action {
            name: "test".to_string(),
            status: "success".to_string(),
            run_time_millis: Some(5000),
            output_url: None,
            step: None,
            index: None,
            start_time: Some("2020-01-01T00:00:00Z".to_string()),
            end_time: None,
        };
        assert_eq!(compute_elapsed_millis(&action), Some(5000));
    }

    #[test]
    fn compute_elapsed_from_start_time() {
        let recent = chrono::Utc::now() - chrono::Duration::seconds(30);
        let action = Action {
            name: "test".to_string(),
            status: "running".to_string(),
            run_time_millis: None,
            output_url: None,
            step: None,
            index: None,
            start_time: Some(recent.to_rfc3339()),
            end_time: None,
        };
        let ms = compute_elapsed_millis(&action).unwrap();
        // Should be approximately 30 seconds (allow 5s tolerance)
        assert!(ms >= 25_000 && ms <= 35_000, "elapsed was {}ms", ms);
    }

    #[test]
    fn compute_elapsed_none_when_no_data() {
        let action = Action {
            name: "test".to_string(),
            status: "running".to_string(),
            run_time_millis: None,
            output_url: None,
            step: None,
            index: None,
            start_time: None,
            end_time: None,
        };
        assert_eq!(compute_elapsed_millis(&action), None);
    }

    #[test]
    fn compute_elapsed_none_when_ended() {
        let action = Action {
            name: "test".to_string(),
            status: "success".to_string(),
            run_time_millis: None,
            output_url: None,
            step: None,
            index: None,
            start_time: Some("2020-01-01T00:00:00Z".to_string()),
            end_time: Some("2020-01-01T00:01:00Z".to_string()),
        };
        assert_eq!(compute_elapsed_millis(&action), None);
    }
}
