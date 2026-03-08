use serde::{Deserialize, Serialize};

// --- v1.1 API: Job detail ---

#[derive(Debug, Clone, Deserialize)]
pub struct JobDetail {
    pub steps: Option<Vec<Step>>,
    pub status: Option<String>,
    pub build_num: Option<u64>,
    pub workflows: Option<WorkflowRef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowRef {
    pub workflow_name: Option<String>,
    pub job_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub name: String,
    pub actions: Vec<Action>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct Action {
    pub name: String,
    pub status: String,
    pub run_time_millis: Option<u64>,
    pub output_url: Option<String>,
    pub step: Option<u32>,
    pub index: Option<u32>,
}

#[allow(dead_code)]
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

// --- v2 API: Job test results ---

#[derive(Debug, Deserialize)]
pub struct TestResultsResponse {
    pub items: Vec<TestResult>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TestResult {
    pub name: Option<String>,
    pub classname: Option<String>,
    pub result: Option<String>,
    pub message: Option<String>,
    pub run_time: Option<f64>,
    pub source: Option<String>,
    pub file: Option<String>,
}

// --- v2 API: Pipeline ---

#[derive(Debug, Deserialize)]
pub struct PipelinesResponse {
    pub items: Vec<Pipeline>,
    pub next_page_token: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Pipeline {
    pub id: String,
    pub number: u64,
    pub state: Option<String>,
    pub created_at: Option<String>,
    #[serde(default)]
    pub trigger: Option<PipelineTrigger>,
    #[serde(default)]
    pub vcs: Option<PipelineVcs>,
}

#[derive(Debug, Deserialize)]
pub struct PipelineTrigger {
    #[serde(rename = "type")]
    pub trigger_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PipelineVcs {
    pub branch: Option<String>,
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
    fn deserialize_test_result_full() {
        let json = r#"{
            "name": "test_login",
            "classname": "AuthSpec",
            "result": "failure",
            "message": "Expected true got false",
            "run_time": 0.437,
            "source": "rspec",
            "file": "spec/auth_spec.rb"
        }"#;
        let tr: TestResult = serde_json::from_str(json).unwrap();
        assert_eq!(tr.name.as_deref(), Some("test_login"));
        assert_eq!(tr.classname.as_deref(), Some("AuthSpec"));
        assert_eq!(tr.result.as_deref(), Some("failure"));
        assert_eq!(tr.message.as_deref(), Some("Expected true got false"));
        assert_eq!(tr.run_time, Some(0.437));
        assert_eq!(tr.source.as_deref(), Some("rspec"));
        assert_eq!(tr.file.as_deref(), Some("spec/auth_spec.rb"));
    }

    #[test]
    fn deserialize_test_result_all_null() {
        let json = r#"{
            "name": null,
            "classname": null,
            "result": null,
            "message": null,
            "run_time": null,
            "source": null,
            "file": null
        }"#;
        let tr: TestResult = serde_json::from_str(json).unwrap();
        assert!(tr.name.is_none());
        assert!(tr.run_time.is_none());
    }

    #[test]
    fn deserialize_test_results_response() {
        let json = r#"{
            "items": [
                {"name": "t1", "classname": null, "result": "success", "message": null, "run_time": 1.5, "source": null, "file": null}
            ],
            "next_page_token": "tok2"
        }"#;
        let resp: TestResultsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.items[0].name.as_deref(), Some("t1"));
        assert_eq!(resp.next_page_token.as_deref(), Some("tok2"));
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
    fn deserialize_pipeline_with_trigger_and_vcs() {
        let json = r#"{
            "id": "pipe-123",
            "number": 42,
            "state": "created",
            "created_at": "2024-01-01T00:00:00Z",
            "trigger": {"type": "webhook"},
            "vcs": {"branch": "main"}
        }"#;
        let p: Pipeline = serde_json::from_str(json).unwrap();
        assert_eq!(p.number, 42);
        assert_eq!(
            p.trigger.as_ref().unwrap().trigger_type.as_deref(),
            Some("webhook")
        );
        assert_eq!(p.vcs.as_ref().unwrap().branch.as_deref(), Some("main"));
    }

    #[test]
    fn deserialize_pipeline_without_trigger_and_vcs() {
        let json = r#"{
            "id": "pipe-456",
            "number": 99,
            "state": "created",
            "created_at": null
        }"#;
        let p: Pipeline = serde_json::from_str(json).unwrap();
        assert_eq!(p.number, 99);
        assert!(p.trigger.is_none());
        assert!(p.vcs.is_none());
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
