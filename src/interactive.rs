use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
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
    mut detail: JobDetail,
    node_index: Option<usize>,
    workflow_id: &str,
    pipeline_number: u64,
    pipeline_id: &str,
) -> Result<State> {
    let back_label = match node_index {
        Some(_) => ".. (back to nodes)",
        None => ".. (back to jobs)",
    };

    loop {
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
        let action_index = node_index.unwrap_or(0);

        // Borrow steps temporarily to extract what we need for show_log
        let (step_clone, action_clone) = {
            let steps = detail.steps.as_ref().unwrap();
            let step = &steps[step_index];
            let Some(action) = step.actions.get(action_index) else {
                continue;
            };
            (step.clone(), action.clone())
        };

        match show_log(
            client,
            &detail,
            &step_clone,
            &action_clone,
            action_index,
            step_index,
        )
        .await?
        {
            LogAction::Back => {
                // Re-fetch job detail to get updated statuses and durations
                if let Ok(refreshed) = client.fetch_job_detail(job_number).await {
                    detail = refreshed;
                }
                continue;
            }
            LogAction::Exit => return Ok(State::Done),
        }
    }
}

// --- Streaming helpers ---

/// RAII guard for crossterm raw mode.
///
/// Entering raw mode disables line buffering and echo so we can poll for
/// individual key presses.  The guard ensures `disable_raw_mode()` is called
/// on all exit paths — including panics and early `?` returns — so the
/// terminal is never left in an unusable state.
struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// Whether the given action status represents a terminal (finished) state.
///
/// CircleCI uses these terminal states for completed actions.  Both "canceled"
/// and "cancelled" appear in practice — the v1.1 API uses "canceled" while the
/// v2 API uses "cancelled", so we accept both.
fn is_step_finished(status: &str) -> bool {
    matches!(
        status,
        "success"
            | "failed"
            | "canceled"
            | "cancelled"
            | "timedout"
            | "infrastructure_fail"
            | "not_run"
    )
}

/// Look up the current status string for a specific action within a job.
///
/// `step_index` is the positional index into `detail.steps[]` (the step array),
/// while `node_index` selects the action within that step's `actions[]` array.
/// In parallelism > 1 jobs each parallel node is a separate action under the
/// same step, so `node_index` corresponds to the parallel container number.
fn find_action_status(detail: &JobDetail, step_index: usize, node_index: usize) -> Option<String> {
    detail
        .steps
        .as_ref()
        .and_then(|steps| steps.get(step_index))
        .and_then(|step| step.actions.get(node_index))
        .map(|action| action.status.clone())
}

/// Write raw bytes to the terminal, converting lone LF (`\n`) to CRLF (`\r\n`).
///
/// In raw mode the terminal driver does not perform automatic newline
/// translation, so a bare `\n` moves the cursor down without returning to
/// column 0, producing staircase output.  This function adds `\r` before
/// every `\n` to restore normal line-break behavior.
///
/// This byte-level replacement is safe even when `data` contains ANSI escape
/// sequences: no standard escape sequence uses `0x0A` (LF) as a parameter
/// byte, so replacing it never corrupts an escape sequence.
fn write_with_crlf(w: &mut impl Write, data: &[u8]) -> std::io::Result<()> {
    for &byte in data {
        if byte == b'\n' {
            w.write_all(b"\r\n")?;
        } else {
            w.write_all(&[byte])?;
        }
    }
    w.flush()
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Clear the in-place status indicator from the current line.
fn clear_status_line(w: &mut impl Write) -> std::io::Result<()> {
    write!(w, "\r\x1b[K")?;
    w.flush()
}

/// Write the streaming status indicator on the current line (dim color).
fn write_status_line(
    w: &mut impl Write,
    elapsed: Duration,
    total_bytes: u64,
    waiting: bool,
) -> std::io::Result<()> {
    let secs = elapsed.as_secs();
    let size = format_bytes(total_bytes);
    let indicator = if waiting { " waiting..." } else { "" };
    write!(
        w,
        "\r\x1b[K\x1b[2m[{}s | {} received{}]\x1b[0m",
        secs, size, indicator
    )?;
    w.flush()
}

/// Stream a running step's log output in real time.
///
/// Enters crossterm raw mode to capture individual key presses, then runs a
/// polling loop that alternates between checking for user input and fetching
/// new log bytes from the CircleCI private API.
///
/// ## Polling loop timing
///
/// Each iteration:
/// 1. `event::poll(50ms)` — check for key presses.  50 ms is below the
///    human-perceptible latency threshold (~100 ms), so key presses feel
///    instant while keeping CPU usage negligible.
/// 2. `fetch_private_output_range` — incremental log fetch.
/// 3. `tokio::time::sleep(950ms)` — wait before next iteration.
///    Together with the 50 ms poll this gives ~1 second per cycle, balancing
///    real-time feel against API load.
/// 4. Every 5 polls (~5 seconds) the job detail is re-fetched to check
///    whether the step has finished.  5 seconds is short enough that the
///    user rarely waits long after completion.
///
/// ## Error handling
///
/// Transient fetch errors trigger adaptive backoff (950 ms → 3 s → 5 s).
/// After 3 consecutive errors the stream stops with a message rather than
/// spinning indefinitely.
#[allow(clippy::too_many_arguments)]
async fn stream_log(
    client: &CircleCiClient,
    detail: &JobDetail,
    step: &Step,
    _action: &Action,
    node_index: usize,
    step_index: usize,
    step_id: u32,
    task_index: u32,
) -> Result<LogAction> {
    // Print header in normal mode
    crate::output::print_node_header(detail, step, node_index, "streaming...");
    println!("  Press Esc or q to stop streaming\n");

    let _guard = RawModeGuard::enable()?;
    let mut stdout = std::io::stdout();

    let job_number = detail.build_num.unwrap_or(0);
    let mut byte_offset: u64 = 0;
    let mut polls_since_status_check: u32 = 0;
    let mut consecutive_errors: u32 = 0;
    let mut poll_interval_ms: u64 = 950;
    let stream_start = Instant::now();
    let mut total_bytes: u64 = 0;
    let mut status_line_visible = false;

    loop {
        // Non-blocking key input check (50ms — below human-perceptible latency)
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        if status_line_visible {
                            clear_status_line(&mut stdout)?;
                        }
                        write!(stdout, "\r\n")?;
                        stdout.flush()?;
                        drop(_guard);
                        return show_post_log_menu();
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if status_line_visible {
                            clear_status_line(&mut stdout)?;
                        }
                        write!(stdout, "\r\n")?;
                        stdout.flush()?;
                        return Ok(LogAction::Exit);
                    }
                    _ => {}
                }
            }
        }

        // Incremental fetch
        match client
            .fetch_private_output_range(job_number, task_index, step_id, byte_offset)
            .await
        {
            Ok(chunk) => {
                consecutive_errors = 0;
                poll_interval_ms = 950; // reset backoff
                if !chunk.data.is_empty() {
                    if status_line_visible {
                        clear_status_line(&mut stdout)?;
                    }
                    total_bytes += chunk.data.len() as u64;
                    write_with_crlf(&mut stdout, &chunk.data)?;
                    byte_offset = chunk.new_offset;
                }
                // Show/update the status indicator
                let elapsed = stream_start.elapsed();
                let waiting = chunk.data.is_empty();
                write_status_line(&mut stdout, elapsed, total_bytes, waiting)?;
                status_line_visible = true;
            }
            Err(_) => {
                consecutive_errors += 1;
                // Adaptive backoff for errors
                poll_interval_ms = if poll_interval_ms <= 950 { 3000 } else { 5000 };
                if consecutive_errors >= 3 {
                    if status_line_visible {
                        clear_status_line(&mut stdout)?;
                    }
                    write!(
                        stdout,
                        "\r\n--- Streaming stopped (connection error) ---\r\n"
                    )?;
                    stdout.flush()?;
                    break;
                }
                // Show status with waiting indication during errors
                let elapsed = stream_start.elapsed();
                write_status_line(&mut stdout, elapsed, total_bytes, true)?;
                status_line_visible = true;
            }
        }

        // Step completion check (~every 5 polls)
        polls_since_status_check += 1;
        if polls_since_status_check >= 5 {
            polls_since_status_check = 0;
            if let Ok(refreshed) = client.fetch_job_detail(job_number).await {
                if let Some(status) = find_action_status(&refreshed, step_index, node_index) {
                    if is_step_finished(&status) {
                        if status_line_visible {
                            clear_status_line(&mut stdout)?;
                        }
                        // Final fetch to catch any remaining output
                        if let Ok(final_chunk) = client
                            .fetch_private_output_range(
                                job_number,
                                task_index,
                                step_id,
                                byte_offset,
                            )
                            .await
                        {
                            if !final_chunk.data.is_empty() {
                                write_with_crlf(&mut stdout, &final_chunk.data)?;
                            }
                        }
                        write!(stdout, "\r\n--- {} [{}] ---\r\n", step.name, status)?;
                        stdout.flush()?;
                        break;
                    }
                }
            }
        }

        // Wait until next poll
        tokio::time::sleep(Duration::from_millis(poll_interval_ms)).await;
    }

    // Drop guard before showing menu
    drop(_guard);
    show_post_log_menu()
}

fn show_post_log_menu() -> Result<LogAction> {
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

async fn show_log(
    client: &CircleCiClient,
    detail: &JobDetail,
    step: &Step,
    action: &Action,
    node_index: usize,
    step_index: usize,
) -> Result<LogAction> {
    // Streaming mode for running steps with private API available
    if action.status == "running" {
        if let (Some(step_id), Some(task_index)) = (action.step, action.index) {
            return stream_log(
                client, detail, step, action, node_index, step_index, step_id, task_index,
            )
            .await;
        }
    }

    // Existing logic for completed steps or when streaming is unavailable
    let job_number = detail.build_num.unwrap_or(0);
    let log = match LogSource::from_action(action, job_number) {
        Some(source) => match client.fetch_log(&source).await {
            Ok(content) => content,
            Err(e) => format!("(failed to fetch log: {})", e),
        },
        None => String::new(),
    };

    crate::output::print_node_log(detail, step, action, node_index, &log)?;

    show_post_log_menu()
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
        if let Some(action) = step.actions.get(node_index) {
            if let Some(ms) = crate::output::compute_elapsed_millis(action) {
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
    let duration = crate::output::format_duration(crate::output::compute_elapsed_millis(action));
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
    let duration = crate::output::format_duration(crate::output::compute_elapsed_millis(action));
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
            start_time: None,
            end_time: None,
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

    // --- is_step_finished tests ---

    #[test]
    fn is_step_finished_success() {
        assert!(is_step_finished("success"));
    }

    #[test]
    fn is_step_finished_failed() {
        assert!(is_step_finished("failed"));
    }

    #[test]
    fn is_step_finished_canceled() {
        assert!(is_step_finished("canceled"));
        assert!(is_step_finished("cancelled"));
    }

    #[test]
    fn is_step_finished_timedout() {
        assert!(is_step_finished("timedout"));
    }

    #[test]
    fn is_step_finished_infrastructure_fail() {
        assert!(is_step_finished("infrastructure_fail"));
    }

    #[test]
    fn is_step_finished_not_run() {
        assert!(is_step_finished("not_run"));
    }

    #[test]
    fn is_step_finished_running() {
        assert!(!is_step_finished("running"));
    }

    #[test]
    fn is_step_finished_queued() {
        assert!(!is_step_finished("queued"));
    }

    // --- find_action_status tests ---

    #[test]
    fn find_action_status_found() {
        let detail = JobDetail {
            steps: Some(vec![
                make_step_with_actions("s0", vec![make_action("a0", "success", None)]),
                make_step_with_actions(
                    "s1",
                    vec![
                        make_action("a0", "running", None),
                        make_action("a1", "failed", None),
                    ],
                ),
            ]),
            status: None,
            build_num: None,
            workflows: None,
        };
        assert_eq!(
            find_action_status(&detail, 1, 0),
            Some("running".to_string())
        );
        assert_eq!(
            find_action_status(&detail, 1, 1),
            Some("failed".to_string())
        );
    }

    #[test]
    fn find_action_status_step_out_of_range() {
        let detail = JobDetail {
            steps: Some(vec![make_step_with_actions(
                "s0",
                vec![make_action("a0", "success", None)],
            )]),
            status: None,
            build_num: None,
            workflows: None,
        };
        assert_eq!(find_action_status(&detail, 5, 0), None);
    }

    #[test]
    fn find_action_status_steps_none() {
        let detail = JobDetail {
            steps: None,
            status: None,
            build_num: None,
            workflows: None,
        };
        assert_eq!(find_action_status(&detail, 0, 0), None);
    }

    // --- write_with_crlf tests ---

    #[test]
    fn write_with_crlf_converts_newlines() {
        let mut buf = Vec::new();
        write_with_crlf(&mut buf, b"hello\nworld\n").unwrap();
        assert_eq!(buf, b"hello\r\nworld\r\n");
    }

    #[test]
    fn write_with_crlf_no_newlines() {
        let mut buf = Vec::new();
        write_with_crlf(&mut buf, b"hello world").unwrap();
        assert_eq!(buf, b"hello world");
    }

    #[test]
    fn write_with_crlf_empty() {
        let mut buf = Vec::new();
        write_with_crlf(&mut buf, b"").unwrap();
        assert!(buf.is_empty());
    }

    #[test]
    fn write_with_crlf_binary_data_passthrough() {
        let mut buf = Vec::new();
        let data = [0x00, 0x01, 0xFF, 0x0A, 0x42]; // 0x0A = \n
        write_with_crlf(&mut buf, &data).unwrap();
        assert_eq!(buf, [0x00, 0x01, 0xFF, 0x0D, 0x0A, 0x42]);
    }

    // --- format_bytes tests ---

    #[test]
    fn format_bytes_small() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn format_bytes_kilobytes() {
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(4300), "4.2 KB");
    }

    #[test]
    fn format_bytes_megabytes() {
        assert_eq!(format_bytes(1048576), "1.0 MB");
        assert_eq!(format_bytes(2621440), "2.5 MB");
    }

    // --- clear_status_line / write_status_line tests ---

    #[test]
    fn clear_status_line_output() {
        let mut buf = Vec::new();
        clear_status_line(&mut buf).unwrap();
        assert_eq!(buf, b"\r\x1b[K");
    }

    #[test]
    fn write_status_line_normal() {
        let mut buf = Vec::new();
        write_status_line(&mut buf, Duration::from_secs(5), 4300, false).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("5s"));
        assert!(s.contains("4.2 KB"));
        assert!(!s.contains("waiting"));
    }

    #[test]
    fn write_status_line_waiting() {
        let mut buf = Vec::new();
        write_status_line(&mut buf, Duration::from_secs(10), 0, true).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("10s"));
        assert!(s.contains("waiting"));
    }
}
