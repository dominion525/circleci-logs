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

/// A chunk of log data returned by an incremental (Range-based) fetch.
///
/// Used by [`CircleCiClient::fetch_private_output_range`] to stream running-job
/// output. The caller tracks `new_offset` and passes it back on the next request
/// so only bytes not yet seen are returned.
pub struct StreamChunk {
    /// Raw log bytes received in this fetch (may be empty when no new output).
    pub data: Vec<u8>,
    /// The byte offset to use for the *next* fetch.
    /// - On HTTP 200 with offset 0: total length of the response body.
    /// - On HTTP 200 with offset > 0: total length if body exceeds offset
    ///   (already-seen bytes are stripped from `data`); otherwise unchanged.
    /// - On HTTP 206 (partial content): `previous_offset + body_length`.
    /// - On HTTP 204/416 (no new data): unchanged from the request offset.
    pub new_offset: u64,
}

/// Build a [`StreamChunk`] from a full-body (non-partial) response, stripping
/// bytes the caller has already seen when `byte_offset > 0`.
fn chunk_from_full_body(data: Vec<u8>, byte_offset: u64) -> StreamChunk {
    let total_len = data.len() as u64;
    if byte_offset > 0 && total_len > byte_offset {
        // Server ignored Range header; strip already-seen prefix
        StreamChunk {
            data: data[byte_offset as usize..].to_vec(),
            new_offset: total_len,
        }
    } else if byte_offset > 0 {
        // Stale/shorter response — no new data; prevent offset regression
        StreamChunk {
            data: Vec::new(),
            new_offset: byte_offset,
        }
    } else {
        // Initial fetch (byte_offset == 0)
        StreamChunk {
            data,
            new_offset: total_len,
        }
    }
}

pub enum LogSource {
    /// output_url preferred; private API used when output_url is absent (running jobs)
    Full {
        job_number: u64,
        step_id: u32,
        task_index: u32,
        output_url: Option<String>,
    },
    /// output_url only (step/index unavailable)
    OutputUrlOnly { output_url: String },
}

impl LogSource {
    pub fn from_action(action: &Action, job_number: u64) -> Option<Self> {
        match (action.step, action.index) {
            (Some(step_id), Some(task_index)) => Some(LogSource::Full {
                job_number,
                step_id,
                task_index,
                output_url: action.output_url.clone(),
            }),
            _ => action
                .output_url
                .as_ref()
                .map(|url| LogSource::OutputUrlOnly {
                    output_url: url.clone(),
                }),
        }
    }
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
        let mut retries = 0u32;
        let resp = loop {
            let resp = self
                .client
                .get(output_url)
                .header(header, value)
                .send()
                .await
                .context("Failed to fetch action output")?;
            if resp.status().as_u16() == 429 && retries < 3 {
                let wait_secs = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1);
                tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;
                retries += 1;
                continue;
            }
            break resp;
        };
        let resp = Self::check_response(resp).await?;
        let outputs: Vec<ActionOutput> =
            resp.json().await.context("Failed to parse action output")?;
        Ok(aggregate_action_outputs(outputs))
    }

    // --- Private API: raw step output ---

    async fn fetch_private_output(
        &self,
        job_number: u64,
        task_index: u32,
        step_id: u32,
    ) -> Result<String> {
        let url = format!(
            "{}/api/private/output/raw/{}/{}/output/{}/{}",
            self.base_url,
            self.config.project_slug(),
            job_number,
            task_index,
            step_id,
        );
        let (header, value) = self.auth_header();
        let resp = self
            .client
            .get(&url)
            .header(header, value)
            .send()
            .await
            .context("Failed to fetch private output")?;

        match resp.status().as_u16() {
            204 => Ok(String::new()),
            _ => {
                let resp = Self::check_response(resp).await?;
                resp.text()
                    .await
                    .context("Failed to read private output body")
            }
        }
    }

    pub async fn fetch_log(&self, source: &LogSource) -> Result<String> {
        match source {
            LogSource::Full {
                job_number,
                step_id,
                task_index,
                output_url,
            } => {
                // Prefer output_url (pre-processed by CircleCI) when available.
                // Fall back to private API for running jobs where output_url is null.
                if let Some(url) = output_url {
                    return self.fetch_action_output(url).await;
                }
                if self.config.use_private_api {
                    if let Ok(text) = self
                        .fetch_private_output(*job_number, *task_index, *step_id)
                        .await
                    {
                        return Ok(text);
                    }
                }
                Ok(String::new())
            }
            LogSource::OutputUrlOnly { output_url } => self.fetch_action_output(output_url).await,
        }
    }

    // --- Private API: incremental range fetch for streaming ---

    /// Fetch a range of raw log bytes from the CircleCI private output API.
    ///
    /// Sends `Range: bytes={byte_offset}-` when `byte_offset > 0` to request only
    /// the bytes beyond what was already received.  The response status determines
    /// how [`StreamChunk::new_offset`] is calculated:
    ///
    /// - **200** – Server ignored the Range header (or first fetch with offset 0).
    ///   `new_offset` = body length.
    /// - **206** – Partial content returned.  `new_offset` = `byte_offset` + body length.
    /// - **204 / 416** – No new data available.  `new_offset` unchanged.
    ///
    /// On HTTP 429 (rate limited) the request is retried up to 3 times, honoring
    /// the `Retry-After` header (defaults to 1 second if missing).
    pub async fn fetch_private_output_range(
        &self,
        job_number: u64,
        task_index: u32,
        step_id: u32,
        byte_offset: u64,
    ) -> Result<StreamChunk> {
        let url = format!(
            "{}/api/private/output/raw/{}/{}/output/{}/{}",
            self.base_url,
            self.config.project_slug(),
            job_number,
            task_index,
            step_id,
        );
        let (header, value) = self.auth_header();
        let mut retries = 0u32;
        let resp = loop {
            let mut req = self.client.get(&url).header(header, value);
            if byte_offset > 0 {
                req = req.header("Range", format!("bytes={}-", byte_offset));
            }
            let resp = req
                .send()
                .await
                .context("Failed to fetch private output range")?;
            let status = resp.status().as_u16();
            if status == 429 && retries < 3 {
                let wait_secs = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1);
                tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;
                retries += 1;
                continue;
            }
            break resp;
        };

        let status = resp.status().as_u16();
        match status {
            204 | 416 => Ok(StreamChunk {
                data: Vec::new(),
                new_offset: byte_offset,
            }),
            200 => {
                let data = resp
                    .bytes()
                    .await
                    .context("Failed to read range body")?
                    .to_vec();
                Ok(chunk_from_full_body(data, byte_offset))
            }
            206 => {
                let data = resp
                    .bytes()
                    .await
                    .context("Failed to read range body")?
                    .to_vec();
                let new_offset = byte_offset + data.len() as u64;
                Ok(StreamChunk { data, new_offset })
            }
            _ => {
                let resp = Self::check_response(resp).await?;
                let data = resp
                    .bytes()
                    .await
                    .context("Failed to read range body")?
                    .to_vec();
                Ok(chunk_from_full_body(data, byte_offset))
            }
        }
    }

    // --- v2: Job test results ---

    pub async fn fetch_job_tests(&self, job_number: u64) -> Result<Vec<TestResult>> {
        let slug = self.config.project_slug();
        let mut all_tests = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/api/v2/project/{}/{}/tests",
                self.base_url, slug, job_number
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
                .context("Failed to fetch job tests")?;
            let resp = Self::check_response(resp).await?;
            let data: TestResultsResponse =
                resp.json().await.context("Failed to parse job tests")?;
            all_tests.extend(data.items);
            match data.next_page_token {
                Some(token) if !token.is_empty() => page_token = Some(token),
                _ => break,
            }
        }
        Ok(all_tests)
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

    pub async fn find_pipeline_uuid(&self, pipeline_number: u64) -> Result<String> {
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

    // --- Interactive mode: single-page fetch methods ---

    pub async fn fetch_pipelines_page(
        &self,
        page_token: Option<&str>,
    ) -> Result<PipelinesResponse> {
        let slug = self.config.project_slug();
        let mut url = format!("{}/api/v2/project/{}/pipeline", self.base_url, slug);
        if let Some(token) = page_token {
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
        resp.json().await.context("Failed to parse pipelines")
    }

    pub async fn fetch_workflow_jobs_page(
        &self,
        workflow_id: &str,
        page_token: Option<&str>,
    ) -> Result<WorkflowJobsResponse> {
        let mut url = format!("{}/api/v2/workflow/{}/job", self.base_url, workflow_id);
        if let Some(token) = page_token {
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
        resp.json().await.context("Failed to parse workflow jobs")
    }

    pub async fn fetch_pipeline_workflows_page(
        &self,
        pipeline_id: &str,
        page_token: Option<&str>,
    ) -> Result<PipelineWorkflowsResponse> {
        let mut url = format!("{}/api/v2/pipeline/{}/workflow", self.base_url, pipeline_id);
        if let Some(token) = page_token {
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
        resp.json()
            .await
            .context("Failed to parse pipeline workflows")
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
            use_private_api: true,
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
    async fn fetch_action_output_retries_on_429() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        // Mount 200 response first (lower priority)
        Mock::given(method("GET"))
            .and(path("/output/retry"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([{"message": "ok\n", "type": "out"}])),
            )
            .mount(&server)
            .await;

        // Mount 429 response second (higher priority, consumed once)
        Mock::given(method("GET"))
            .and(path("/output/retry"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .up_to_n_times(1)
            .mount(&server)
            .await;

        let output_url = format!("{}/output/retry", server.uri());
        let result = client.fetch_action_output(&output_url).await.unwrap();
        assert_eq!(result, "ok\n");
    }

    #[tokio::test]
    async fn fetch_action_output_429_exhausts_retries() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        // Always return 429
        Mock::given(method("GET"))
            .and(path("/output/always429"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .mount(&server)
            .await;

        let output_url = format!("{}/output/always429", server.uri());
        let err = client.fetch_action_output(&output_url).await.unwrap_err();
        assert!(err.to_string().contains("Rate limited"));
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

    // --- fetch_job_tests tests ---

    #[tokio::test]
    async fn fetch_job_tests_single_page() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        let body = serde_json::json!({
            "items": [
                {"name": "test1", "classname": "Suite", "result": "success", "message": null, "run_time": 0.5, "source": "rspec", "file": "spec/a.rb"}
            ],
            "next_page_token": null
        });

        Mock::given(method("GET"))
            .and(path("/api/v2/project/gh/test-org/test-repo/42/tests"))
            .and(header("Circle-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(1)
            .mount(&server)
            .await;

        let tests = client.fetch_job_tests(42).await.unwrap();
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name.as_deref(), Some("test1"));
        assert_eq!(tests[0].run_time, Some(0.5));
    }

    #[tokio::test]
    async fn fetch_job_tests_pagination() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/api/v2/project/gh/test-org/test-repo/10/tests"))
            .and(query_param("page-token", "page2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{"name": "t2", "classname": null, "result": "failure", "message": "fail", "run_time": null, "source": null, "file": null}],
                "next_page_token": null
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/v2/project/gh/test-org/test-repo/10/tests"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{"name": "t1", "classname": null, "result": "success", "message": null, "run_time": 1.0, "source": null, "file": null}],
                "next_page_token": "page2"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let tests = client.fetch_job_tests(10).await.unwrap();
        assert_eq!(tests.len(), 2);
    }

    #[tokio::test]
    async fn fetch_job_tests_empty() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/api/v2/project/gh/test-org/test-repo/99/tests"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [],
                "next_page_token": null
            })))
            .mount(&server)
            .await;

        let tests = client.fetch_job_tests(99).await.unwrap();
        assert!(tests.is_empty());
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

    // --- fetch_pipelines_page tests ---

    #[tokio::test]
    async fn fetch_pipelines_page_single() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/api/v2/project/gh/test-org/test-repo/pipeline"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [
                    {"id": "p1", "number": 1, "state": "created", "created_at": "2024-01-01T00:00:00Z"}
                ],
                "next_page_token": "tok2"
            })))
            .mount(&server)
            .await;

        let resp = client.fetch_pipelines_page(None).await.unwrap();
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.items[0].number, 1);
        assert_eq!(resp.next_page_token.as_deref(), Some("tok2"));
    }

    #[tokio::test]
    async fn fetch_pipelines_page_empty() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/api/v2/project/gh/test-org/test-repo/pipeline"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [],
                "next_page_token": null
            })))
            .mount(&server)
            .await;

        let resp = client.fetch_pipelines_page(None).await.unwrap();
        assert!(resp.items.is_empty());
        assert!(resp.next_page_token.is_none());
    }

    // --- fetch_workflow_jobs_page tests ---

    #[tokio::test]
    async fn fetch_workflow_jobs_page_single() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/api/v2/workflow/wf-abc/job"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [
                    {"id": "j1", "name": "build", "status": "success", "job_number": 10, "type": "build", "started_at": null, "stopped_at": null}
                ],
                "next_page_token": null
            })))
            .mount(&server)
            .await;

        let resp = client
            .fetch_workflow_jobs_page("wf-abc", None)
            .await
            .unwrap();
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.items[0].name, "build");
        assert!(resp.next_page_token.is_none());
    }

    // --- fetch_pipeline_workflows_page tests ---

    #[tokio::test]
    async fn fetch_pipeline_workflows_page_single() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/api/v2/pipeline/pipe-xyz/workflow"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [
                    {"id": "wf-1", "name": "deploy", "status": "success", "created_at": "2024-01-01T00:00:00Z", "stopped_at": null, "pipeline_number": 42}
                ],
                "next_page_token": null
            })))
            .mount(&server)
            .await;

        let resp = client
            .fetch_pipeline_workflows_page("pipe-xyz", None)
            .await
            .unwrap();
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.items[0].name, "deploy");
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

    // --- Private output API tests ---

    #[tokio::test]
    async fn fetch_private_output_success() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_string("hello world\n"))
            .mount(&server)
            .await;

        let result = client.fetch_private_output(42, 0, 106).await.unwrap();
        assert_eq!(result, "hello world\n");
    }

    #[tokio::test]
    async fn fetch_private_output_empty() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let result = client.fetch_private_output(42, 0, 106).await.unwrap();
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn fetch_private_output_error() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let err = client.fetch_private_output(42, 0, 106).await.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn fetch_log_private_success() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_string("log output\n"))
            .mount(&server)
            .await;

        let source = LogSource::Full {
            job_number: 42,
            step_id: 106,
            task_index: 0,
            output_url: None,
        };
        let result = client.fetch_log(&source).await.unwrap();
        assert_eq!(result, "log output\n");
    }

    #[tokio::test]
    async fn fetch_log_prefers_output_url() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        // output_url should be used directly; private API should not be called
        let output_url = format!("{}/output-url", server.uri());
        Mock::given(method("GET"))
            .and(path("/output-url"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!([{"message": "from output_url\n", "type": "out"}]),
            ))
            .expect(1)
            .mount(&server)
            .await;

        let source = LogSource::Full {
            job_number: 42,
            step_id: 106,
            task_index: 0,
            output_url: Some(output_url),
        };
        let result = client.fetch_log(&source).await.unwrap();
        assert_eq!(result, "from output_url\n");
    }

    #[tokio::test]
    async fn fetch_log_private_api_fails_no_output_url() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        // Private API returns 500, no output_url
        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let source = LogSource::Full {
            job_number: 42,
            step_id: 106,
            task_index: 0,
            output_url: None,
        };
        let result = client.fetch_log(&source).await.unwrap();
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn fetch_log_output_url_only() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path("/output"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([{"message": "via url\n", "type": "out"}])),
            )
            .mount(&server)
            .await;

        let source = LogSource::OutputUrlOnly {
            output_url: format!("{}/output", server.uri()),
        };
        let result = client.fetch_log(&source).await.unwrap();
        assert_eq!(result, "via url\n");
    }

    // --- fetch_private_output_range tests ---

    #[tokio::test]
    async fn fetch_range_200_full_body() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_string("hello world\n"))
            .mount(&server)
            .await;

        let chunk = client
            .fetch_private_output_range(42, 0, 106, 0)
            .await
            .unwrap();
        assert_eq!(chunk.data, b"hello world\n");
        assert_eq!(chunk.new_offset, 12); // "hello world\n".len()
    }

    #[tokio::test]
    async fn fetch_range_206_partial_content() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .and(header("Range", "bytes=100-"))
            .respond_with(ResponseTemplate::new(206).set_body_string("new data"))
            .mount(&server)
            .await;

        let chunk = client
            .fetch_private_output_range(42, 0, 106, 100)
            .await
            .unwrap();
        assert_eq!(chunk.data, b"new data");
        assert_eq!(chunk.new_offset, 108); // 100 + 8
    }

    #[tokio::test]
    async fn fetch_range_204_no_content() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let chunk = client
            .fetch_private_output_range(42, 0, 106, 0)
            .await
            .unwrap();
        assert!(chunk.data.is_empty());
        assert_eq!(chunk.new_offset, 0);
    }

    #[tokio::test]
    async fn fetch_range_416_range_not_satisfiable() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .and(header("Range", "bytes=999-"))
            .respond_with(ResponseTemplate::new(416))
            .mount(&server)
            .await;

        let chunk = client
            .fetch_private_output_range(42, 0, 106, 999)
            .await
            .unwrap();
        assert!(chunk.data.is_empty());
        assert_eq!(chunk.new_offset, 999);
    }

    #[tokio::test]
    async fn fetch_range_no_range_header_at_offset_zero() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        // This mock requires NO Range header — it only matches requests without one
        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_string("initial"))
            .expect(1)
            .mount(&server)
            .await;

        let chunk = client
            .fetch_private_output_range(42, 0, 106, 0)
            .await
            .unwrap();
        assert_eq!(chunk.data, b"initial");
        assert_eq!(chunk.new_offset, 7);
    }

    #[tokio::test]
    async fn fetch_range_429_retries_then_succeeds() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        // 200 response (lower priority)
        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_string("after retry"))
            .mount(&server)
            .await;

        // 429 response (higher priority, consumed once)
        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .up_to_n_times(1)
            .mount(&server)
            .await;

        let chunk = client
            .fetch_private_output_range(42, 0, 106, 0)
            .await
            .unwrap();
        assert_eq!(chunk.data, b"after retry");
    }

    // --- chunk_from_full_body unit tests ---

    #[test]
    fn chunk_from_full_body_strips_prefix() {
        let data = b"AAAAABBBBB".to_vec(); // 10 bytes
        let chunk = super::chunk_from_full_body(data, 5);
        assert_eq!(chunk.data, b"BBBBB");
        assert_eq!(chunk.new_offset, 10);
    }

    #[test]
    fn chunk_from_full_body_no_regression() {
        let data = vec![b'X'; 3];
        let chunk = super::chunk_from_full_body(data, 5);
        assert!(chunk.data.is_empty());
        assert_eq!(chunk.new_offset, 5);
    }

    #[test]
    fn chunk_from_full_body_zero_offset() {
        let data = b"hello".to_vec();
        let chunk = super::chunk_from_full_body(data.clone(), 0);
        assert_eq!(chunk.data, data);
        assert_eq!(chunk.new_offset, 5);
    }

    #[test]
    fn chunk_from_full_body_exact_match() {
        let data = vec![b'Y'; 100];
        let chunk = super::chunk_from_full_body(data, 100);
        assert!(chunk.data.is_empty());
        assert_eq!(chunk.new_offset, 100);
    }

    // --- fetch_private_output_range: HTTP 200 with nonzero offset ---

    #[tokio::test]
    async fn fetch_range_200_with_nonzero_offset_strips_prefix() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        // Server ignores Range header and returns full body with HTTP 200
        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .and(header("Range", "bytes=5-"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'A'; 10]))
            .mount(&server)
            .await;

        let chunk = client
            .fetch_private_output_range(42, 0, 106, 5)
            .await
            .unwrap();
        // Should only contain the 5 bytes beyond offset 5
        assert_eq!(chunk.data.len(), 5);
        assert_eq!(chunk.data, vec![b'A'; 5]);
        assert_eq!(chunk.new_offset, 10);
    }

    #[tokio::test]
    async fn fetch_range_200_stale_response_no_regression() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        // Server returns shorter response (3 bytes) when we've seen 5
        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .and(header("Range", "bytes=5-"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'B'; 3]))
            .mount(&server)
            .await;

        let chunk = client
            .fetch_private_output_range(42, 0, 106, 5)
            .await
            .unwrap();
        assert!(chunk.data.is_empty());
        assert_eq!(chunk.new_offset, 5); // must not regress
    }

    #[tokio::test]
    async fn fetch_range_200_exact_length_equals_offset() {
        let server = MockServer::start().await;
        let client = CircleCiClient::with_base_url(test_config(), server.uri());

        // Server returns exactly 5 bytes (same as offset) — no new data
        Mock::given(method("GET"))
            .and(path(
                "/api/private/output/raw/gh/test-org/test-repo/42/output/0/106",
            ))
            .and(header("Range", "bytes=5-"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'C'; 5]))
            .mount(&server)
            .await;

        let chunk = client
            .fetch_private_output_range(42, 0, 106, 5)
            .await
            .unwrap();
        assert!(chunk.data.is_empty());
        assert_eq!(chunk.new_offset, 5);
    }
}
