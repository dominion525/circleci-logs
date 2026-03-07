use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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
        let env_token = env::var("CIRCLE_TOKEN").ok();
        Self::from_file_and_token(config_file.as_deref(), env_token)
    }

    fn from_file_and_token(config_path: Option<&Path>, env_token: Option<String>) -> Result<Self> {
        let parsed = config_path
            .map(|p| {
                let content = fs::read_to_string(p)
                    .with_context(|| format!("Failed to read config file: {}", p.display()))?;
                let config: ConfigFile =
                    toml::from_str(&content).context("Failed to parse config file")?;
                Ok::<_, anyhow::Error>(config)
            })
            .transpose()?;

        let token = resolve_token(env_token, parsed.as_ref().and_then(|c| c.token.clone()))?;

        let project_str = parsed.as_ref().and_then(|c| c.project.clone()).context(
            "Missing 'project' field. Set it in .circleci-logs.toml (e.g. github/org/repo)",
        )?;

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
    let dir = env::current_dir().ok()?;
    find_config_file_from(&dir)
}

fn find_config_file_from(start_dir: &Path) -> Option<PathBuf> {
    let mut dir = start_dir.to_path_buf();
    loop {
        let candidate = dir.join(".circleci-logs.toml");
        if candidate.exists() {
            warn_if_permissive(&candidate);
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(unix)]
fn warn_if_permissive(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mode = meta.permissions().mode();
        if mode & 0o077 != 0 {
            eprintln!(
                "Warning: {} is accessible by other users (mode {:o}). Consider: chmod 600 {}",
                path.display(),
                mode & 0o777,
                path.display(),
            );
        }
    }
}

#[cfg(not(unix))]
fn warn_if_permissive(_path: &std::path::Path) {}

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

    // --- from_file_and_token tests ---

    #[test]
    fn load_full_config_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(
            &path,
            "token = \"my-token\"\nproject = \"github/myorg/myrepo\"\n",
        )
        .unwrap();

        let config = Config::from_file_and_token(Some(&path), None).unwrap();
        assert_eq!(config.token, "my-token");
        assert_eq!(config.vcs_type, "github");
        assert_eq!(config.org, "myorg");
        assert_eq!(config.repo, "myrepo");
    }

    #[test]
    fn load_env_token_overrides_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(
            &path,
            "token = \"file-token\"\nproject = \"github/org/repo\"\n",
        )
        .unwrap();

        let config =
            Config::from_file_and_token(Some(&path), Some("env-token".to_string())).unwrap();
        assert_eq!(config.token, "env-token");
    }

    #[test]
    fn load_env_token_when_file_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(&path, "project = \"github/org/repo\"\n").unwrap();

        let config = Config::from_file_and_token(Some(&path), Some("env-tok".to_string())).unwrap();
        assert_eq!(config.token, "env-tok");
    }

    #[test]
    fn load_missing_project_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(&path, "token = \"tok\"\n").unwrap();

        let err = Config::from_file_and_token(Some(&path), None).unwrap_err();
        assert!(err.to_string().contains("Missing 'project' field"));
    }

    #[test]
    fn load_malformed_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(&path, "this is not valid toml [[[").unwrap();

        let err = Config::from_file_and_token(Some(&path), None).unwrap_err();
        assert!(err.to_string().contains("Failed to parse config file"));
    }

    #[test]
    fn load_no_token_anywhere() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(&path, "project = \"github/org/repo\"\n").unwrap();

        let err = Config::from_file_and_token(Some(&path), None).unwrap_err();
        assert!(err.to_string().contains("Token not found"));
    }

    #[test]
    fn load_no_config_file() {
        let err = Config::from_file_and_token(None, Some("tok".to_string())).unwrap_err();
        assert!(err.to_string().contains("Missing 'project' field"));
    }

    #[test]
    fn load_invalid_project_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(&path, "token = \"tok\"\nproject = \"invalid-format\"\n").unwrap();

        let err = Config::from_file_and_token(Some(&path), None).unwrap_err();
        assert!(err.to_string().contains("vcs_type/org/repo"));
    }

    // --- find_config_file_from tests ---

    #[test]
    fn find_config_in_current_dir() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".circleci-logs.toml"), "").unwrap();

        let found = find_config_file_from(dir.path()).unwrap();
        let expected = fs::canonicalize(dir.path().join(".circleci-logs.toml")).unwrap();
        let actual = fs::canonicalize(found).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn find_config_walks_up() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".circleci-logs.toml"), "").unwrap();
        let child = dir.path().join("sub").join("deep");
        fs::create_dir_all(&child).unwrap();

        let found = find_config_file_from(&child).unwrap();
        let expected = fs::canonicalize(dir.path().join(".circleci-logs.toml")).unwrap();
        let actual = fs::canonicalize(found).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn find_config_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("empty");
        fs::create_dir_all(&child).unwrap();

        assert!(find_config_file_from(&child).is_none());
    }
}
