use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct ConfigFile {
    token: Option<String>,
    project: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub token: String,
    pub vcs_type: String,
    pub org: String,
    pub repo: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_file = find_config_file();
        let parsed = config_file
            .as_ref()
            .map(|path| {
                let content = fs::read_to_string(path)
                    .with_context(|| format!("設定ファイルの読み込みに失敗: {}", path.display()))?;
                let config: ConfigFile =
                    toml::from_str(&content).context("設定ファイルのパースに失敗")?;
                Ok::<_, anyhow::Error>(config)
            })
            .transpose()?;

        let token = env::var("CIRCLE_TOKEN")
            .ok()
            .or_else(|| parsed.as_ref().and_then(|c| c.token.clone()))
            .context("トークンが見つかりません。環境変数 CIRCLE_TOKEN または .circleci-logs.toml の token を設定してください")?;

        let project_str = parsed
            .as_ref()
            .and_then(|c| c.project.clone())
            .context("project が設定されていません。.circleci-logs.toml の project フィールドを設定してください (例: github/org/repo)")?;

        let (vcs_type, org, repo) = parse_project(&project_str)?;

        Ok(Config {
            token,
            vcs_type,
            org,
            repo,
        })
    }

    pub fn project_slug(&self) -> String {
        format!("{}/{}/{}", self.vcs_type, self.org, self.repo)
    }
}

fn find_config_file() -> Option<PathBuf> {
    let mut dir = env::current_dir().ok()?;
    loop {
        let candidate = dir.join(".circleci-logs.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn parse_project(project: &str) -> Result<(String, String, String)> {
    let parts: Vec<&str> = project.split('/').collect();
    if parts.len() != 3 {
        bail!("project は 'vcs_type/org/repo' の形式で指定してください (例: github/myorg/myrepo)");
    }
    Ok((
        parts[0].to_string(),
        parts[1].to_string(),
        parts[2].to_string(),
    ))
}
