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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_action_output() {
        let json = r#"{"message": "hello", "type": "out"}"#;
        let output: ActionOutput = serde_json::from_str(json).unwrap();
        assert_eq!(output.message, "hello");
        assert_eq!(output.output_type.as_deref(), Some("out"));
    }

    #[test]
    fn deserialize_workflow_job_optional_fields() {
        let json = r#"{
            "id": "abc-123",
            "name": "build",
            "status": "success",
            "job_number": null,
            "type": null,
            "started_at": null,
            "stopped_at": null
        }"#;
        let job: WorkflowJob = serde_json::from_str(json).unwrap();
        assert_eq!(job.id, "abc-123");
        assert_eq!(job.name, "build");
        assert!(job.job_number.is_none());
        assert!(job.job_type.is_none());
    }

    #[test]
    fn deserialize_pipeline_workflow() {
        let json = r#"{
            "id": "wf-456",
            "name": "deploy",
            "status": "running",
            "created_at": "2024-01-01T00:00:00Z",
            "stopped_at": null,
            "pipeline_number": 42
        }"#;
        let wf: PipelineWorkflow = serde_json::from_str(json).unwrap();
        assert_eq!(wf.id, "wf-456");
        assert_eq!(wf.name, "deploy");
        assert_eq!(wf.status, "running");
        assert_eq!(wf.pipeline_number, Some(42));
        assert!(wf.stopped_at.is_none());
    }

    #[test]
    fn deserialize_job_detail_all_null() {
        let json = r#"{
            "steps": null,
            "status": null,
            "build_num": null,
            "workflows": null
        }"#;
        let detail: JobDetail = serde_json::from_str(json).unwrap();
        assert!(detail.steps.is_none());
        assert!(detail.status.is_none());
        assert!(detail.build_num.is_none());
        assert!(detail.workflows.is_none());
    }

    #[test]
    fn deserialize_workflow_jobs_response_with_page_token() {
        let json = r#"{
            "items": [
                {
                    "id": "j1",
                    "name": "test",
                    "status": "success",
                    "job_number": 10,
                    "type": "build",
                    "started_at": "2024-01-01T00:00:00Z",
                    "stopped_at": "2024-01-01T00:01:00Z"
                }
            ],
            "next_page_token": "abc123"
        }"#;
        let resp: WorkflowJobsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.items[0].name, "test");
        assert_eq!(resp.next_page_token.as_deref(), Some("abc123"));
    }
}
