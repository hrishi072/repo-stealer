use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Github,
    Gitlab,
}

/// API and clone origins are derived from a user-selected instance, never
/// from provider response links. Cloud roots are fixed; self-hosted roots may
/// include a relative-URL prefix, but never credentials/query/fragment.
#[derive(Clone, Debug)]
pub struct Destination {
    pub provider: Provider,
    pub instance: String,
    pub repository: String,
    pub deployment: &'static str,
    api: Url,
    web: Url,
}
impl Destination {
    pub fn new(provider: Provider, instance: Option<&str>, repository: &str) -> Result<Self> {
        let segments: Vec<_> = repository.split('/').collect();
        ensure!(repository.len() <= 1024 && segments.len() >= 2
            && (provider != Provider::Github || segments.len() == 2)
            && segments.iter().all(|s| !s.is_empty() && *s != "." && *s != ".."
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))),
            "Use the canonical owner/repository or GitLab namespace/project path, without a clone URL");
        let deployment = if instance.is_some() {
            "self_hosted"
        } else {
            "cloud"
        };
        let default = match provider {
            Provider::Github => "https://github.com/",
            Provider::Gitlab => "https://gitlab.com/",
        };
        let input = instance.unwrap_or(default);
        let checked = repo_stealer_git::RemoteUrl::parse(input)?;
        let mut web = Url::parse(checked.as_str())?;
        ensure!(
            !web.path().contains('%')
                && !web.path().trim_end_matches('/').ends_with("/api/v3")
                && !web.path().trim_end_matches('/').ends_with("/api/v4"),
            "Enter an instance URL, not an API URL or encoded path"
        );
        if !web.path().ends_with('/') {
            web.set_path(&(web.path().to_owned() + "/"));
        }
        let api = match (provider, instance.is_none()) {
            (Provider::Github, true) => Url::parse("https://api.github.com/")?,
            (Provider::Github, false) => web.join("api/v3/")?,
            (Provider::Gitlab, _) => web.join("api/v4/")?,
        };
        Ok(Self {
            provider,
            instance: web.as_str().into(),
            repository: repository.into(),
            deployment,
            api,
            web,
        })
    }
    pub fn actor_url(&self) -> Url {
        self.api.join("user").expect("fixed API path")
    }
    pub fn repository_url(&self) -> Url {
        let mut url = self.api.clone();
        {
            let mut path = url.path_segments_mut().expect("HTTPS base");
            path.pop_if_empty();
            match self.provider {
                Provider::Github => {
                    path.push("repos");
                    for part in self.repository.split('/') {
                        path.push(part);
                    }
                }
                Provider::Gitlab => {
                    path.push("projects");
                    path.push(&self.repository);
                }
            }
        }
        url
    }
    pub fn policy_url(&self, suffix: &str, page: usize) -> Url {
        let mut url = self.repository_url();
        url.path_segments_mut().unwrap().push(suffix);
        url.query_pairs_mut()
            .append_pair("per_page", "100")
            .append_pair("page", &page.to_string());
        if self.provider == Provider::Github && suffix == "branches" {
            url.query_pairs_mut().append_pair("protected", "true");
        }
        if self.provider == Provider::Github && suffix == "rulesets" {
            url.query_pairs_mut()
                .append_pair("includes_parents", "true");
        }
        url
    }
    pub fn approved_clone(&self, path: &str, clone_url: &str) -> Result<String> {
        let same = match self.provider {
            Provider::Github => path.eq_ignore_ascii_case(&self.repository),
            Provider::Gitlab => path == self.repository,
        };
        ensure!(
            same,
            "Provider returned a different repository identity; use its canonical path"
        );
        let checked = repo_stealer_git::RemoteUrl::parse(clone_url)?;
        let mut expected = self.web.clone();
        {
            let mut segments = expected.path_segments_mut().unwrap();
            segments.pop_if_empty();
            let parts: Vec<_> = path.split('/').collect();
            for part in &parts[..parts.len() - 1] {
                segments.push(part);
            }
            segments.push(&(parts[parts.len() - 1].to_owned() + ".git"));
        }
        ensure!(
            checked.as_str() == expected.as_str(),
            "Provider clone URL is outside the selected instance/repository; it was not contacted"
        );
        Ok(checked.as_str().into())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Actor {
    pub id: u64,
    pub login: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub id: u64,
    pub path: String,
    pub clone_url: String,
    pub archived: bool,
    pub disabled: bool,
    pub default_branch: Option<String>,
    pub push_reported: Option<bool>,
    pub access_level: Option<u32>,
}

#[derive(Deserialize)]
struct GithubRepo {
    id: u64,
    full_name: String,
    clone_url: String,
    archived: bool,
    disabled: bool,
    default_branch: Option<String>,
    permissions: Option<GithubPermissions>,
}
#[derive(Deserialize)]
struct GithubPermissions {
    push: Option<bool>,
}
#[derive(Deserialize)]
struct GitlabRepo {
    id: u64,
    path_with_namespace: String,
    http_url_to_repo: String,
    archived: bool,
    default_branch: Option<String>,
    permissions: Option<GitlabPermissions>,
    #[serde(default)]
    marked_for_deletion_on: Option<String>,
}
#[derive(Deserialize)]
struct GitlabPermissions {
    project_access: Option<GitlabAccess>,
    group_access: Option<GitlabAccess>,
}
#[derive(Deserialize)]
struct GitlabAccess {
    access_level: u32,
}

pub fn repository(destination: &Destination, value: serde_json::Value) -> Result<Repository> {
    let result = match destination.provider {
        Provider::Github => {
            let r: GithubRepo = serde_json::from_value(value)
                .map_err(|_| anyhow::anyhow!("Invalid GitHub repository response"))?;
            Repository {
                id: r.id,
                path: r.full_name,
                clone_url: r.clone_url,
                archived: r.archived,
                disabled: r.disabled,
                default_branch: r.default_branch,
                push_reported: r.permissions.and_then(|p| p.push),
                access_level: None,
            }
        }
        Provider::Gitlab => {
            let r: GitlabRepo = serde_json::from_value(value)
                .map_err(|_| anyhow::anyhow!("Invalid GitLab project response"))?;
            let level = r.permissions.and_then(|p| {
                p.project_access
                    .into_iter()
                    .chain(p.group_access)
                    .map(|a| a.access_level)
                    .max()
            });
            Repository {
                id: r.id,
                path: r.path_with_namespace,
                clone_url: r.http_url_to_repo,
                archived: r.archived,
                disabled: r.marked_for_deletion_on.is_some(),
                default_branch: r.default_branch,
                push_reported: None,
                access_level: level,
            }
        }
    };
    ensure!(result.id > 0, "Invalid repository ID");
    if let Some(branch) = &result.default_branch {
        ensure!(valid_branch(branch), "Invalid provider default branch");
    }
    destination.approved_clone(&result.path, &result.clone_url)?;
    Ok(result)
}
pub fn actor(provider: Provider, value: serde_json::Value) -> Result<Actor> {
    #[derive(Deserialize)]
    struct GithubActor {
        id: u64,
        login: String,
    }
    #[derive(Deserialize)]
    struct GitlabActor {
        id: u64,
        username: String,
    }
    let actor = match provider {
        Provider::Github => {
            let r: GithubActor = serde_json::from_value(value)
                .map_err(|_| anyhow::anyhow!("Invalid GitHub actor response"))?;
            Actor {
                id: r.id,
                login: r.login,
            }
        }
        Provider::Gitlab => {
            let r: GitlabActor = serde_json::from_value(value)
                .map_err(|_| anyhow::anyhow!("Invalid GitLab actor response"))?;
            Actor {
                id: r.id,
                login: r.username,
            }
        }
    };
    ensure!(
        actor.id > 0
            && !actor.login.is_empty()
            && actor.login.len() <= 255
            && actor
                .login
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
        "Invalid provider actor identity"
    );
    Ok(actor)
}
pub fn valid_branch(name: &str) -> bool {
    name.len() <= 1024
        && repo_stealer_core::RefSnapshot::parse(&format!("{} refs/heads/{name}", "a".repeat(40)))
            .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binds_cloud_and_prefixed_self_hosted_endpoints() {
        let gh = Destination::new(Provider::Github, None, "owner/repo").unwrap();
        assert_eq!(
            gh.repository_url().as_str(),
            "https://api.github.com/repos/owner/repo"
        );
        assert!(gh
            .approved_clone("owner/repo", "https://evil.invalid/owner/repo.git")
            .is_err());
        assert!(gh
            .approved_clone("other/repo", "https://github.com/other/repo.git")
            .is_err());
        assert!(gh
            .approved_clone("Owner/Repo", "https://github.com/Owner/Repo.git")
            .is_ok());
        let gl = Destination::new(Provider::Gitlab, None, "group/sub/repo").unwrap();
        assert_eq!(
            gl.repository_url().as_str(),
            "https://gitlab.com/api/v4/projects/group%2Fsub%2Frepo"
        );
        let local = Destination::new(
            Provider::Gitlab,
            Some("https://git.example/platform"),
            "group/sub/repo",
        )
        .unwrap();
        assert_eq!(
            local.repository_url().as_str(),
            "https://git.example/platform/api/v4/projects/group%2Fsub%2Frepo"
        );
        assert!(local
            .approved_clone(
                "group/sub/repo",
                "https://git.example/platform/group/sub/repo.git"
            )
            .is_ok());
        for bad in [
            "../repo",
            "owner/../repo",
            "owner/repo?token=x",
            "owner/%2frepo",
        ] {
            assert!(Destination::new(Provider::Gitlab, None, bad).is_err());
        }
        assert!(Destination::new(Provider::Github, None, "group/sub/repo").is_err());
    }
}
