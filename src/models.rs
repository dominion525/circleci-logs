#![allow(dead_code)]

use serde::{Deserialize, Serialize};

// --- v1.1 API: Job detail ---

#[derive(Debug, Deserialize)]
pub struct JobDetail {
    pub steps: Option<Vec<Step>>,
    pub status: Option<String>,
    pub build_num: Option<u64>,
    pub workflows: Option<WorkflowRef>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRef {
    pub workflow_name: Option<String>,
    pub job_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Step {
    pub name: String,
    pub actions: Vec<Action>,
}

#[derive(Debug, Deserialize)]
pub struct Action {
    pub name: String,
    pub status: String,
    pub run_time_millis: Option<u64>,
    pub output_url: Option<String>,
    pub step: Option<u32>,
    pub index: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct ActionOutput {
    pub message: String,
    #[serde(rename = "type")]
    pub output_type: Option<String>,
}

// --- v2 API: Workflow jobs ---

#[derive(Debug, Deserialize)]
pub struct WorkflowJobsResponse {
    pub items: Vec<WorkflowJob>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkflowJob {
    pub id: String,
    pub name: String,
    pub status: String,
    pub job_number: Option<u64>,
    #[serde(rename = "type")]
    pub job_type: Option<String>,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
}

// --- v2 API: Pipeline ---

#[derive(Debug, Deserialize)]
pub struct PipelinesResponse {
    pub items: Vec<Pipeline>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Pipeline {
    pub id: String,
    pub number: u64,
    pub state: Option<String>,
    pub created_at: Option<String>,
}

// --- v2 API: Pipeline workflows ---

#[derive(Debug, Deserialize)]
pub struct PipelineWorkflowsResponse {
    pub items: Vec<PipelineWorkflow>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PipelineWorkflow {
    pub id: String,
    pub name: String,
    pub status: String,
    pub created_at: Option<String>,
    pub stopped_at: Option<String>,
    pub pipeline_number: Option<u64>,
}
