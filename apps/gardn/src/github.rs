pub(crate) mod diff;
pub(crate) mod domain;
pub(crate) mod runtime;
pub(crate) mod screen;
pub(crate) mod service;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GithubRepository {
    owner: String,
    repo: String,
}

impl GithubRepository {
    pub fn parse(input: &str) -> Result<Self, String> {
        let value = input.trim();
        let mut parts = value.split('/');
        let owner = parts.next().unwrap_or_default();
        let repo = parts.next().unwrap_or_default();
        let repo = if repo
            .get(repo.len().saturating_sub(4)..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".git"))
        {
            &repo[..repo.len() - 4]
        } else {
            repo
        };
        if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
            return Err(format!(
                "GitHub repository must use the owner/repository form: {value}"
            ));
        }
        validate_part("owner", owner, 39, false)?;
        validate_part("repository", repo, 100, true)?;
        Ok(Self {
            owner: owner.to_ascii_lowercase(),
            repo: repo.to_ascii_lowercase(),
        })
    }

    pub fn as_str(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }
}

impl std::fmt::Display for GithubRepository {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.owner)?;
        formatter.write_str("/")?;
        formatter.write_str(&self.repo)
    }
}

fn validate_part(
    label: &str,
    value: &str,
    max_len: usize,
    allow_repository_punctuation: bool,
) -> Result<(), String> {
    if value.len() > max_len
        || value == "."
        || value == ".."
        || (!allow_repository_punctuation
            && (value.starts_with('-') || value.ends_with('-') || value.contains("--")))
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || byte == b'-'
                || (allow_repository_punctuation && (byte == b'_' || byte == b'.'))
        })
    {
        return Err(format!(
            "GitHub {label} contains invalid characters or is too long: {value}"
        ));
    }
    Ok(())
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode", content = "repositories")]
pub enum GithubRepositoryScope {
    #[default]
    Automatic,
    Selected(Vec<GithubRepository>),
    GroupOrganization,
}

impl GithubRepositoryScope {
    pub fn selected_from_input(input: &str) -> Result<Self, String> {
        let mut repositories = Vec::new();
        for value in input.split(|character: char| character == ',' || character.is_whitespace()) {
            if value.is_empty() {
                continue;
            }
            let repository = GithubRepository::parse(value)?;
            if !repositories.contains(&repository) {
                repositories.push(repository);
            }
        }
        if repositories.is_empty() {
            return Err("Select at least one GitHub repository (owner/repository).".to_string());
        }
        repositories.sort();
        Ok(Self::Selected(repositories))
    }

    pub fn selected_repositories(&self) -> &[GithubRepository] {
        match self {
            Self::Selected(repositories) => repositories,
            Self::Automatic | Self::GroupOrganization => &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubDiscoveryOutcome {
    Repositories(Vec<GithubRepository>),
    Empty,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGithubScope {
    pub repositories: Vec<GithubRepository>,
    pub organization: Option<crate::app::state::GithubOrganization>,
}

pub fn resolve_github_scope(
    scope: &GithubRepositoryScope,
    discovery: &GithubDiscoveryOutcome,
    group_organization: Option<&crate::app::state::GithubOrganization>,
) -> Result<ResolvedGithubScope, String> {
    match scope {
        GithubRepositoryScope::Selected(repositories) => {
            if repositories.is_empty() {
                return Err("Select at least one GitHub repository (owner/repository).".to_string());
            }
            let mut repositories = repositories.clone();
            repositories.sort();
            repositories.dedup();
            Ok(ResolvedGithubScope {
                repositories,
                organization: None,
            })
        }
        GithubRepositoryScope::GroupOrganization => group_organization
            .cloned()
            .map(|organization| ResolvedGithubScope {
                repositories: Vec::new(),
                organization: Some(organization),
            })
            .ok_or_else(|| {
                "Configure a GitHub organization for this Space's group before launching GitHub."
                    .to_string()
            }),
        GithubRepositoryScope::Automatic => match discovery {
            GithubDiscoveryOutcome::Failed(error) => Err(error.clone()),
            GithubDiscoveryOutcome::Empty => Ok(ResolvedGithubScope {
                repositories: Vec::new(),
                organization: group_organization.cloned(),
            }),
            GithubDiscoveryOutcome::Repositories(discovered) => {
                let mut repositories = discovered.clone();
                repositories.sort();
                repositories.dedup();
                if repositories.is_empty() {
                    return Ok(ResolvedGithubScope {
                        repositories: Vec::new(),
                        organization: group_organization.cloned(),
                    });
                }
                Ok(ResolvedGithubScope {
                    repositories,
                    organization: None,
                })
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_input_is_canonical_and_deduplicated() {
        let scope = GithubRepositoryScope::selected_from_input("Acme/One, acme/one.GIT\nAcme/Two")
            .expect("valid selected scope");
        assert_eq!(scope.selected_repositories().len(), 2);
        assert_eq!(scope.selected_repositories()[0].to_string(), "acme/one");
    }

    #[test]
    fn automatic_discovery_failure_does_not_fall_back() {
        let org = crate::app::state::GithubOrganization::parse("acme")
            .unwrap()
            .unwrap();
        let error = resolve_github_scope(
            &GithubRepositoryScope::Automatic,
            &GithubDiscoveryOutcome::Failed("remote host unavailable".to_string()),
            Some(&org),
        )
        .unwrap_err();
        assert_eq!(error, "remote host unavailable");
    }
    #[test]
    fn selected_scope_excludes_other_discovered_repositories() {
        let scope = GithubRepositoryScope::selected_from_input("Acme/One.GIT").unwrap();
        let discovery = GithubDiscoveryOutcome::Repositories(vec![
            GithubRepository::parse("acme/one").unwrap(),
            GithubRepository::parse("acme/two").unwrap(),
        ]);
        let resolved = resolve_github_scope(&scope, &discovery, None).unwrap();
        assert_eq!(
            resolved.repositories,
            vec![GithubRepository::parse("acme/one").unwrap()]
        );
    }
}
