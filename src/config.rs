use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Deserialize)]
struct ConfigFile {
    token: Option<String>,
    project: Option<String>,
    use_private_api: Option<bool>,
}

#[derive(Clone)]
pub struct Config {
    pub token: String,
    pub vcs_type: String,
    pub org: String,
    pub repo: String,
    pub use_private_api: bool,
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
        let env_private_api = env::var("CIRCLECI_LOGS_PRIVATE_API").ok();
        Self::from_file_and_env(config_file.as_deref(), env_token, env_private_api)
    }

    fn from_file_and_env(
        config_path: Option<&Path>,
        env_token: Option<String>,
        env_private_api: Option<String>,
    ) -> Result<Self> {
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

        let (vcs_type, org, repo) = match parsed.as_ref().and_then(|c| c.project.clone()) {
            Some(project_str) => parse_project(&project_str)?,
            None => detect_project_from_git_remote().context(
                "Could not determine project. Set 'project' in .circleci-logs.toml \
                 (e.g. github/org/repo) or run from inside a git repository with a remote named 'origin'",
            )?,
        };

        // env var takes precedence over config file; default is true
        let use_private_api = match env_private_api {
            Some(v) => !matches!(v.as_str(), "0" | "false" | "no"),
            None => parsed
                .as_ref()
                .and_then(|c| c.use_private_api)
                .unwrap_or(true),
        };

        Ok(Config {
            token,
            vcs_type,
            org,
            repo,
            use_private_api,
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

pub(crate) fn normalize_vcs_type(vcs: &str) -> Result<String> {
    match vcs {
        "gh" | "github" => Ok("gh".to_string()),
        "bb" | "bitbucket" => Ok("bb".to_string()),
        _ => bail!(
            "Unknown VCS type '{}'. Use 'github' (or 'gh') or 'bitbucket' (or 'bb')",
            vcs
        ),
    }
}

fn parse_project(project: &str) -> Result<(String, String, String)> {
    let parts: Vec<&str> = project.split('/').collect();
    if parts.len() != 3 {
        bail!("'project' must be in 'vcs_type/org/repo' format (e.g. github/myorg/myrepo)");
    }
    let vcs_type = normalize_vcs_type(parts[0])?;
    Ok((vcs_type, parts[1].to_string(), parts[2].to_string()))
}

fn host_to_vcs_type(host: &str) -> Result<String> {
    match host {
        "github.com" => Ok("gh".to_string()),
        "bitbucket.org" => Ok("bb".to_string()),
        _ => bail!(
            "Unsupported git host '{}'. Only github.com and bitbucket.org are supported",
            host
        ),
    }
}

fn parse_git_remote_url(url: &str) -> Result<(String, String, String)> {
    let url = url.trim();
    if url.is_empty() {
        bail!("Empty git remote URL");
    }

    let (host, path) = if url.starts_with("ssh://") {
        // ssh://git@github.com/org/repo.git
        let rest = url.strip_prefix("ssh://").unwrap();
        let rest = rest.split('@').next_back().unwrap_or(rest);
        let slash_pos = rest
            .find('/')
            .context("Invalid SSH URL: no path separator")?;
        (&rest[..slash_pos], &rest[slash_pos + 1..])
    } else if url.starts_with("https://") || url.starts_with("http://") {
        // https://github.com/org/repo.git or https://user@github.com/org/repo.git
        let rest = url.split("://").nth(1).unwrap();
        let rest = rest.split('@').next_back().unwrap_or(rest);
        let slash_pos = rest
            .find('/')
            .context("Invalid HTTPS URL: no path separator")?;
        (&rest[..slash_pos], &rest[slash_pos + 1..])
    } else if url.contains(':') && !url.contains("://") {
        // git@github.com:org/repo.git (SCP-like syntax)
        let at_host = url.split(':').next().unwrap();
        let host = at_host.split('@').next_back().unwrap_or(at_host);
        let path = url.split(':').nth(1).unwrap();
        (host, path)
    } else {
        bail!("Unrecognized git remote URL format: {}", url);
    };

    let vcs_type = host_to_vcs_type(host)?;

    // Strip trailing .git
    let path = path.strip_suffix(".git").unwrap_or(path);

    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() != 2 {
        bail!("Expected 'org/repo' path in git remote URL, got '{}'", path);
    }

    Ok((vcs_type, parts[0].to_string(), parts[1].to_string()))
}

fn detect_project_from_git_remote() -> Result<(String, String, String)> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .context("Failed to run 'git remote get-url origin'")?;

    if !output.status.success() {
        bail!(
            "git remote get-url origin failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let url = String::from_utf8(output.stdout).context("git remote URL is not valid UTF-8")?;
    parse_git_remote_url(&url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_project_valid_github() {
        let (vcs, org, repo) = parse_project("github/myorg/myrepo").unwrap();
        assert_eq!(vcs, "gh");
        assert_eq!(org, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_project_valid_gh() {
        let (vcs, org, repo) = parse_project("gh/myorg/myrepo").unwrap();
        assert_eq!(vcs, "gh");
        assert_eq!(org, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_project_valid_bitbucket() {
        let (vcs, org, repo) = parse_project("bitbucket/myorg/myrepo").unwrap();
        assert_eq!(vcs, "bb");
        assert_eq!(org, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_project_valid_bb() {
        let (vcs, org, repo) = parse_project("bb/myorg/myrepo").unwrap();
        assert_eq!(vcs, "bb");
        assert_eq!(org, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_project_unknown_vcs() {
        assert!(parse_project("gitlab/myorg/myrepo").is_err());
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
            vcs_type: "gh".to_string(),
            org: "myorg".to_string(),
            repo: "myrepo".to_string(),
            use_private_api: true,
        };
        assert_eq!(config.project_slug(), "gh/myorg/myrepo");
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
            vcs_type: "gh".to_string(),
            org: "myorg".to_string(),
            repo: "myrepo".to_string(),
            use_private_api: true,
        };
        let debug = format!("{:?}", config);
        assert!(!debug.contains("super-secret-token"));
        assert!(debug.contains("***"));
        assert!(debug.contains("myorg"));
    }

    // --- from_file_and_env tests ---

    #[test]
    fn load_full_config_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(
            &path,
            "token = \"my-token\"\nproject = \"github/myorg/myrepo\"\n",
        )
        .unwrap();

        let config = Config::from_file_and_env(Some(&path), None, None).unwrap();
        assert_eq!(config.token, "my-token");
        assert_eq!(config.vcs_type, "gh");
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
            Config::from_file_and_env(Some(&path), Some("env-token".to_string()), None).unwrap();
        assert_eq!(config.token, "env-token");
    }

    #[test]
    fn load_env_token_when_file_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(&path, "project = \"github/org/repo\"\n").unwrap();

        let config =
            Config::from_file_and_env(Some(&path), Some("env-tok".to_string()), None).unwrap();
        assert_eq!(config.token, "env-tok");
    }

    #[test]
    fn load_missing_project_field_falls_back_to_git_remote() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(&path, "token = \"tok\"\n").unwrap();

        // This test runs inside a git repo, so detect_project_from_git_remote should succeed
        let config = Config::from_file_and_env(Some(&path), None, None).unwrap();
        assert!(!config.org.is_empty());
        assert!(!config.repo.is_empty());
    }

    #[test]
    fn load_malformed_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(&path, "this is not valid toml [[[").unwrap();

        let err = Config::from_file_and_env(Some(&path), None, None).unwrap_err();
        assert!(err.to_string().contains("Failed to parse config file"));
    }

    #[test]
    fn load_no_token_anywhere() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(&path, "project = \"github/org/repo\"\n").unwrap();

        let err = Config::from_file_and_env(Some(&path), None, None).unwrap_err();
        assert!(err.to_string().contains("Token not found"));
    }

    #[test]
    fn load_no_config_file_falls_back_to_git_remote() {
        // This test runs inside a git repo, so detect_project_from_git_remote should succeed
        let config = Config::from_file_and_env(None, Some("tok".to_string()), None).unwrap();
        assert!(!config.org.is_empty());
        assert!(!config.repo.is_empty());
    }

    #[test]
    fn load_invalid_project_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(&path, "token = \"tok\"\nproject = \"invalid-format\"\n").unwrap();

        let err = Config::from_file_and_env(Some(&path), None, None).unwrap_err();
        assert!(err.to_string().contains("vcs_type/org/repo"));
    }

    // --- use_private_api tests ---

    #[test]
    fn use_private_api_default_true() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(&path, "token = \"tok\"\nproject = \"github/org/repo\"\n").unwrap();

        let config = Config::from_file_and_env(Some(&path), None, None).unwrap();
        assert!(config.use_private_api);
    }

    #[test]
    fn use_private_api_file_false() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(
            &path,
            "token = \"tok\"\nproject = \"github/org/repo\"\nuse_private_api = false\n",
        )
        .unwrap();

        let config = Config::from_file_and_env(Some(&path), None, None).unwrap();
        assert!(!config.use_private_api);
    }

    #[test]
    fn use_private_api_env_overrides_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(
            &path,
            "token = \"tok\"\nproject = \"github/org/repo\"\nuse_private_api = true\n",
        )
        .unwrap();

        let config =
            Config::from_file_and_env(Some(&path), None, Some("false".to_string())).unwrap();
        assert!(!config.use_private_api);
    }

    #[test]
    fn use_private_api_env_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".circleci-logs.toml");
        fs::write(&path, "token = \"tok\"\nproject = \"github/org/repo\"\n").unwrap();

        let config = Config::from_file_and_env(Some(&path), None, Some("0".to_string())).unwrap();
        assert!(!config.use_private_api);
    }

    // --- host_to_vcs_type tests ---

    #[test]
    fn host_to_vcs_type_github() {
        assert_eq!(host_to_vcs_type("github.com").unwrap(), "gh");
    }

    #[test]
    fn host_to_vcs_type_bitbucket() {
        assert_eq!(host_to_vcs_type("bitbucket.org").unwrap(), "bb");
    }

    #[test]
    fn host_to_vcs_type_unsupported() {
        let err = host_to_vcs_type("gitlab.com").unwrap_err();
        assert!(err.to_string().contains("Unsupported git host"));
    }

    // --- parse_git_remote_url tests ---

    #[test]
    fn parse_https_with_dot_git() {
        let (vcs, org, repo) = parse_git_remote_url("https://github.com/myorg/myrepo.git").unwrap();
        assert_eq!(vcs, "gh");
        assert_eq!(org, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_https_without_dot_git() {
        let (vcs, org, repo) = parse_git_remote_url("https://github.com/myorg/myrepo").unwrap();
        assert_eq!(vcs, "gh");
        assert_eq!(org, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_ssh_scp_format() {
        let (vcs, org, repo) = parse_git_remote_url("git@github.com:myorg/myrepo.git").unwrap();
        assert_eq!(vcs, "gh");
        assert_eq!(org, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_ssh_url_format() {
        let (vcs, org, repo) =
            parse_git_remote_url("ssh://git@github.com/myorg/myrepo.git").unwrap();
        assert_eq!(vcs, "gh");
        assert_eq!(org, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_bitbucket_https() {
        let (vcs, org, repo) =
            parse_git_remote_url("https://bitbucket.org/myorg/myrepo.git").unwrap();
        assert_eq!(vcs, "bb");
        assert_eq!(org, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_bitbucket_ssh() {
        let (vcs, org, repo) = parse_git_remote_url("git@bitbucket.org:myorg/myrepo.git").unwrap();
        assert_eq!(vcs, "bb");
        assert_eq!(org, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_trailing_newline() {
        let (vcs, org, repo) =
            parse_git_remote_url("https://github.com/myorg/myrepo.git\n").unwrap();
        assert_eq!(vcs, "gh");
        assert_eq!(org, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_unsupported_host() {
        let err = parse_git_remote_url("https://gitlab.com/myorg/myrepo.git").unwrap_err();
        assert!(err.to_string().contains("Unsupported git host"));
    }

    #[test]
    fn parse_invalid_path_segments() {
        let err = parse_git_remote_url("https://github.com/only-one-segment").unwrap_err();
        assert!(err.to_string().contains("Expected 'org/repo' path"));
    }

    #[test]
    fn parse_empty_url() {
        assert!(parse_git_remote_url("").is_err());
    }

    #[test]
    fn parse_unrecognized_format() {
        let err = parse_git_remote_url("/local/path/to/repo").unwrap_err();
        assert!(
            err.to_string()
                .contains("Unrecognized git remote URL format")
        );
    }

    #[test]
    fn parse_https_with_userinfo() {
        let (vcs, org, repo) =
            parse_git_remote_url("https://user@github.com/org/repo.git").unwrap();
        assert_eq!(vcs, "gh");
        assert_eq!(org, "org");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn parse_http_url() {
        let (vcs, org, repo) = parse_git_remote_url("http://github.com/myorg/myrepo.git").unwrap();
        assert_eq!(vcs, "gh");
        assert_eq!(org, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_too_many_path_segments() {
        let err = parse_git_remote_url("https://github.com/a/b/c/d").unwrap_err();
        assert!(err.to_string().contains("Expected 'org/repo' path"));
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
