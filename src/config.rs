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

#[derive(Clone)]
pub struct Config {
    pub token: String,
    pub vcs_type: String,
    pub org: String,
    pub repo: String,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("token", &"***")
            .field("vcs_type", &self.vcs_type)
            .field("org", &self.org)
            .field("repo", &self.repo)
            .finish()
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_file = find_config_file();
        let parsed = config_file
            .as_ref()
            .map(|path| {
                let content = fs::read_to_string(path)
                    .with_context(|| format!("Failed to read config file: {}", path.display()))?;
                let config: ConfigFile =
                    toml::from_str(&content).context("Failed to parse config file")?;
                Ok::<_, anyhow::Error>(config)
            })
            .transpose()?;

        let token = resolve_token(
            env::var("CIRCLE_TOKEN").ok(),
            parsed.as_ref().and_then(|c| c.token.clone()),
        )?;

        let project_str = parsed
            .as_ref()
            .and_then(|c| c.project.clone())
            .context("Missing 'project' field. Set it in .circleci-logs.toml (e.g. github/org/repo)")?;

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

fn resolve_token(env_token: Option<String>, file_token: Option<String>) -> Result<String> {
    env_token
        .or(file_token)
        .context("Token not found. Set CIRCLE_TOKEN env var or 'token' in .circleci-logs.toml")
}

fn parse_project(project: &str) -> Result<(String, String, String)> {
    let parts: Vec<&str> = project.split('/').collect();
    if parts.len() != 3 {
        bail!("'project' must be in 'vcs_type/org/repo' format (e.g. github/myorg/myrepo)");
    }
    Ok((
        parts[0].to_string(),
        parts[1].to_string(),
        parts[2].to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_project_valid() {
        let (vcs, org, repo) = parse_project("github/myorg/myrepo").unwrap();
        assert_eq!(vcs, "github");
        assert_eq!(org, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_project_too_few_parts() {
        assert!(parse_project("github/org").is_err());
    }

    #[test]
    fn parse_project_too_many_parts() {
        assert!(parse_project("a/b/c/d").is_err());
    }

    #[test]
    fn parse_project_empty() {
        assert!(parse_project("").is_err());
    }

    #[test]
    fn project_slug_format() {
        let config = Config {
            token: "tok".to_string(),
            vcs_type: "github".to_string(),
            org: "myorg".to_string(),
            repo: "myrepo".to_string(),
        };
        assert_eq!(config.project_slug(), "github/myorg/myrepo");
    }

    #[test]
    fn resolve_token_env_wins() {
        let result = resolve_token(
            Some("env-token".to_string()),
            Some("file-token".to_string()),
        )
        .unwrap();
        assert_eq!(result, "env-token");
    }

    #[test]
    fn resolve_token_file_fallback() {
        let result = resolve_token(None, Some("file-token".to_string())).unwrap();
        assert_eq!(result, "file-token");
    }

    #[test]
    fn resolve_token_both_none() {
        assert!(resolve_token(None, None).is_err());
    }

    #[test]
    fn debug_redacts_token() {
        let config = Config {
            token: "super-secret-token".to_string(),
            vcs_type: "github".to_string(),
            org: "myorg".to_string(),
            repo: "myrepo".to_string(),
        };
        let debug = format!("{:?}", config);
        assert!(!debug.contains("super-secret-token"));
        assert!(debug.contains("***"));
        assert!(debug.contains("myorg"));
    }
}
