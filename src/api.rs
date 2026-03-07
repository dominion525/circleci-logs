use anyhow::{Context, Result, bail};
use reqwest::Client;

use crate::config::Config;
use crate::models::*;

fn aggregate_action_outputs(outputs: Vec<ActionOutput>) -> String {
    outputs
        .into_iter()
        .map(|o| o.message)
        .collect::<Vec<_>>()
        .join("")
}

pub struct CircleCiClient {
    client: Client,
    config: Config,
    base_url: String,
}

impl CircleCiClient {
    pub fn new(config: Config) -> Self {
        Self {
            client: Client::new(),
            config,
            base_url: "https://circleci.com".to_string(),
        }
    }

    #[allow(dead_code)]
    pub fn with_base_url(config: Config, base_url: String) -> Self {
        Self {
            client: Client::new(),
            config,
            base_url,
        }
    }

    fn auth_header(&self) -> (&str, &str) {
        ("Circle-Token", &self.config.token)
    }

    async fn check_response(resp: reqwest::Response) -> Result<reqwest::Response> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        match status.as_u16() {
            401 => bail!("Authentication failed: invalid token. Check CIRCLE_TOKEN"),
            404 => bail!("Resource not found (404). Check the ID or project settings"),
            429 => bail!("Rate limited. Please wait and retry"),
            _ => {
                let body = resp.text().await.unwrap_or_default();
                bail!("API error ({}): {}", status, body)
            }
        }
    }

    // --- v1.1: Job detail ---

    pub async fn fetch_job_detail(&self, job_number: u64) -> Result<JobDetail> {
        let url = format!(
            "{}/api/v1.1/project/{}/{}/{}/{}",
            self.base_url, self.config.vcs_type, self.config.org, self.config.repo, job_number
        );
        let (header, value) = self.auth_header();
        let resp = self
            .client
            .get(&url)
            .header(header, value)
            .send()
            .await
            .context("Failed to fetch job detail")?;
        let resp = Self::check_response(resp).await?;
        resp.json().await.context("Failed to parse job detail")
    }

    pub async fn fetch_action_output(&self, output_url: &str) -> Result<String> {
        let (header, value) = self.auth_header();
        let resp = self
            .client
            .get(output_url)
            .header(header, value)
            .send()
            .await
            .context("Failed to fetch action output")?;
        let resp = Self::check_response(resp).await?;
        let outputs: Vec<ActionOutput> =
            resp.json().await.context("Failed to parse action output")?;
        Ok(aggregate_action_outputs(outputs))
    }

    // --- v2: Workflow jobs ---

    pub async fn fetch_workflow_jobs(&self, workflow_id: &str) -> Result<Vec<WorkflowJob>> {
        let mut all_jobs = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!("{}/api/v2/workflow/{}/job", self.base_url, workflow_id);
            if let Some(ref token) = page_token {
                url.push_str(&format!("?page-token={}", token));
            }
            let (header, value) = self.auth_header();
            let resp = self
                .client
                .get(&url)
                .header(header, value)
                .send()
                .await
                .context("Failed to fetch workflow jobs")?;
            let resp = Self::check_response(resp).await?;
            let data: WorkflowJobsResponse =
                resp.json().await.context("Failed to parse workflow jobs")?;
            all_jobs.extend(data.items);
            match data.next_page_token {
                Some(token) if !token.is_empty() => page_token = Some(token),
                _ => break,
            }
        }
        Ok(all_jobs)
    }

    // --- v2: Pipelines ---

    pub async fn fetch_pipeline_workflows(
        &self,
        pipeline_number: u64,
    ) -> Result<Vec<PipelineWorkflow>> {
        let pipeline_uuid = self.find_pipeline_uuid(pipeline_number).await?;
        let mut all_workflows = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/api/v2/pipeline/{}/workflow",
                self.base_url, pipeline_uuid
            );
            if let Some(ref token) = page_token {
                url.push_str(&format!("?page-token={}", token));
            }
            let (header, value) = self.auth_header();
            let resp = self
                .client
                .get(&url)
                .header(header, value)
                .send()
                .await
                .context("Failed to fetch pipeline workflows")?;
            let resp = Self::check_response(resp).await?;
            let data: PipelineWorkflowsResponse = resp
                .json()
                .await
                .context("Failed to parse pipeline workflows")?;
            all_workflows.extend(data.items);
            match data.next_page_token {
                Some(token) if !token.is_empty() => page_token = Some(token),
                _ => break,
            }
        }
        Ok(all_workflows)
    }

    async fn find_pipeline_uuid(&self, pipeline_number: u64) -> Result<String> {
        let slug = self.config.project_slug();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!("{}/api/v2/project/{}/pipeline", self.base_url, slug);
            if let Some(ref token) = page_token {
                url.push_str(&format!("?page-token={}", token));
            }
            let (header, value) = self.auth_header();
            let resp = self
                .client
                .get(&url)
                .header(header, value)
                .send()
                .await
                .context("Failed to fetch pipelines")?;
            let resp = Self::check_response(resp).await?;
            let data: PipelinesResponse = resp.json().await.context("Failed to parse pipelines")?;
            for pipeline in &data.items {
                if pipeline.number == pipeline_number {
                    return Ok(pipeline.id.clone());
                }
            }
            match data.next_page_token {
                Some(token) if !token.is_empty() => page_token = Some(token),
                _ => break,
            }
        }
        bail!("Pipeline number {} not found", pipeline_number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config() -> Config {
        Config {
            token: "test-token".into(),
            vcs_type: "gh".into(),
            org: "test-org".into(),
            repo: "test-repo".into(),
        }
    }

    #[test]
    fn aggregate_empty() {
        assert_eq!(aggregate_action_outputs(vec![]), "");
    }

    #[test]
    fn aggregate_single() {
        let outputs = vec![ActionOutput {
            message: "hello".to_string(),
            output_type: None,
        }];
        assert_eq!(aggregate_action_outputs(outputs), "hello");
    }

    #[test]
    fn aggregate_multiple() {
        let outputs = vec![
            ActionOutput {
                message: "hello ".to_string(),
                output_type: Some("out".to_string()),
            },
            ActionOutput {
                message: "world".to_string(),
                output_type: None,
            },
        ];
        assert_eq!(aggregate_action_outputs(outputs), "hello world");
    }

    // --- fetch_job_detail tests ---

    #[tokio::test]
    async fn fetch_job_detail_success() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        let body = serde_json::json!({
            "steps": [{"name": "build", "actions": [{"name": "compile", "status": "success", "run_time_millis": 1000, "output_url": null, "step": 0, "index": 0}]}],
            "status": "success",
            "build_num": 42,
            "workflows": {"workflow_name": "main", "job_name": "build"}
        });

        Mock::given(method("GET"))
            .and(path("/api/v1.1/project/gh/test-org/test-repo/42"))
            .and(header("Circle-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(1)
            .mount(&server)
            .await;

        let detail = client.fetch_job_detail(42).await.unwrap();
        assert_eq!(detail.build_num, Some(42));
        assert_eq!(detail.status.as_deref(), Some("success"));
        assert!(detail.steps.is_some());
    }

    #[tokio::test]
    async fn fetch_job_detail_401() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/api/v1.1/project/gh/test-org/test-repo/1"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let err = client.fetch_job_detail(1).await.unwrap_err();
        assert!(err.to_string().contains("Authentication failed"));
    }

    #[tokio::test]
    async fn fetch_job_detail_404() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/api/v1.1/project/gh/test-org/test-repo/999"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let err = client.fetch_job_detail(999).await.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn fetch_job_detail_500() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/api/v1.1/project/gh/test-org/test-repo/1"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
            .mount(&server)
            .await;

        let err = client.fetch_job_detail(1).await.unwrap_err();
        assert!(err.to_string().contains("API error"));
    }

    // --- fetch_action_output tests ---

    #[tokio::test]
    async fn fetch_action_output_success() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        let body = serde_json::json!([
            {"message": "line 1\n", "type": "out"},
            {"message": "line 2\n", "type": "out"}
        ]);

        Mock::given(method("GET"))
            .and(path("/output"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let output_url = format!("{}/output", server.uri());
        let result = client.fetch_action_output(&output_url).await.unwrap();
        assert_eq!(result, "line 1\nline 2\n");
    }

    #[tokio::test]
    async fn fetch_action_output_parse_error() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/output"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let output_url = format!("{}/output", server.uri());
        let err = client.fetch_action_output(&output_url).await.unwrap_err();
        assert!(err.to_string().contains("Failed to parse action output"));
    }

    // --- fetch_workflow_jobs tests ---

    #[tokio::test]
    async fn fetch_workflow_jobs_single_page() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        let body = serde_json::json!({
            "items": [
                {"id": "j1", "name": "build", "status": "success", "job_number": 10, "type": "build", "started_at": null, "stopped_at": null}
            ],
            "next_page_token": null
        });

        Mock::given(method("GET"))
            .and(path("/api/v2/workflow/wf-123/job"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let jobs = client.fetch_workflow_jobs("wf-123").await.unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].name, "build");
    }

    #[tokio::test]
    async fn fetch_workflow_jobs_pagination() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        // Page 2 (matched by query_param)
        Mock::given(method("GET"))
            .and(path("/api/v2/workflow/wf-123/job"))
            .and(query_param("page-token", "page2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{"id": "j2", "name": "test", "status": "failed", "job_number": 11, "type": "build", "started_at": null, "stopped_at": null}],
                "next_page_token": null
            })))
            .expect(1)
            .mount(&server)
            .await;

        // Page 1 (no query_param, broader match registered after specific one)
        Mock::given(method("GET"))
            .and(path("/api/v2/workflow/wf-123/job"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{"id": "j1", "name": "build", "status": "success", "job_number": 10, "type": "build", "started_at": null, "stopped_at": null}],
                "next_page_token": "page2"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let jobs = client.fetch_workflow_jobs("wf-123").await.unwrap();
        assert_eq!(jobs.len(), 2);
    }

    // --- fetch_pipeline_workflows + find_pipeline_uuid tests ---

    #[tokio::test]
    async fn fetch_pipeline_workflows_success() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        // Pipeline list (find_pipeline_uuid)
        Mock::given(method("GET"))
            .and(path("/api/v2/project/gh/test-org/test-repo/pipeline"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [
                    {"id": "pipe-abc", "number": 42, "state": "created", "created_at": "2024-01-01T00:00:00Z"}
                ],
                "next_page_token": null
            })))
            .mount(&server)
            .await;

        // Pipeline workflows
        Mock::given(method("GET"))
            .and(path("/api/v2/pipeline/pipe-abc/workflow"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [
                    {"id": "wf-1", "name": "deploy", "status": "success", "created_at": "2024-01-01T00:00:00Z", "stopped_at": null, "pipeline_number": 42}
                ],
                "next_page_token": null
            })))
            .mount(&server)
            .await;

        let workflows = client.fetch_pipeline_workflows(42).await.unwrap();
        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0].name, "deploy");
    }

    #[tokio::test]
    async fn find_pipeline_uuid_not_found() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/api/v2/project/gh/test-org/test-repo/pipeline"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [
                    {"id": "pipe-other", "number": 99, "state": "created", "created_at": null}
                ],
                "next_page_token": null
            })))
            .mount(&server)
            .await;

        let err = client.fetch_pipeline_workflows(42).await.unwrap_err();
        assert!(err.to_string().contains("Pipeline number 42 not found"));
    }
}
