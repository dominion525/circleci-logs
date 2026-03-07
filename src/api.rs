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
}

impl CircleCiClient {
    pub fn new(config: Config) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    fn auth_header(&self) -> (&str, String) {
        ("Circle-Token", self.config.token.clone())
    }

    async fn check_response(resp: reqwest::Response) -> Result<reqwest::Response> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        match status.as_u16() {
            401 => bail!("認証エラー: トークンが無効です。CIRCLE_TOKEN を確認してください"),
            404 => bail!("リソースが見つかりません (404)。ID やプロジェクト設定を確認してください"),
            429 => bail!("レート制限に達しました。しばらく待ってから再試行してください"),
            _ => {
                let body = resp.text().await.unwrap_or_default();
                bail!("API エラー ({}): {}", status, body)
            }
        }
    }

    // --- v1.1: Job detail ---

    pub async fn fetch_job_detail(&self, job_number: u64) -> Result<JobDetail> {
        let url = format!(
            "https://circleci.com/api/v1.1/project/{}/{}/{}/{}",
            self.config.vcs_type, self.config.org, self.config.repo, job_number
        );
        let (header, value) = self.auth_header();
        let resp = self
            .client
            .get(&url)
            .header(header, value)
            .send()
            .await
            .context("ジョブ情報の取得に失敗")?;
        let resp = Self::check_response(resp).await?;
        resp.json().await.context("ジョブ情報のパースに失敗")
    }

    pub async fn fetch_action_output(&self, output_url: &str) -> Result<String> {
        let (header, value) = self.auth_header();
        let resp = self
            .client
            .get(output_url)
            .header(header, value)
            .send()
            .await
            .context("ログの取得に失敗")?;
        let resp = Self::check_response(resp).await?;
        let outputs: Vec<ActionOutput> = resp.json().await.unwrap_or_default();
        Ok(aggregate_action_outputs(outputs))
    }

    // --- v2: Workflow jobs ---

    pub async fn fetch_workflow_jobs(&self, workflow_id: &str) -> Result<Vec<WorkflowJob>> {
        let mut all_jobs = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!("https://circleci.com/api/v2/workflow/{}/job", workflow_id);
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
                .context("ワークフロージョブ一覧の取得に失敗")?;
            let resp = Self::check_response(resp).await?;
            let data: WorkflowJobsResponse = resp
                .json()
                .await
                .context("ワークフロージョブ一覧のパースに失敗")?;
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
                "https://circleci.com/api/v2/pipeline/{}/workflow",
                pipeline_uuid
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
                .context("パイプラインワークフロー一覧の取得に失敗")?;
            let resp = Self::check_response(resp).await?;
            let data: PipelineWorkflowsResponse = resp
                .json()
                .await
                .context("パイプラインワークフロー一覧のパースに失敗")?;
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
            let mut url = format!("https://circleci.com/api/v2/project/{}/pipeline", slug);
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
                .context("パイプライン一覧の取得に失敗")?;
            let resp = Self::check_response(resp).await?;
            let data: PipelinesResponse = resp
                .json()
                .await
                .context("パイプライン一覧のパースに失敗")?;
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
        bail!("パイプライン番号 {} が見つかりません", pipeline_number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
