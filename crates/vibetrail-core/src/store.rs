use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::{Project, SessionSummary};
use crate::provider::{Provider, RawSession, ResumeSpec};

/// Normalizes cwd strings coming out of provider metadata so that sessions
/// from different providers referring to the same directory group together:
/// expand `~`, resolve symlinks when the path exists, fold `.`/`..` lexically
/// otherwise.
pub fn normalize_path(path: &str) -> String {
    let expanded: PathBuf = if path == "~" {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"))
    } else if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join(rest)
    } else {
        PathBuf::from(path)
    };
    let resolved = expanded.canonicalize().unwrap_or_else(|_| lexical_clean(&expanded));
    let text = resolved.to_string_lossy();
    let trimmed = text.trim_end_matches('/');
    if trimmed.is_empty() { "/".to_string() } else { trimmed.to_string() }
}

fn lexical_clean(path: &Path) -> PathBuf {
    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                cleaned.pop();
            }
            other => cleaned.push(other),
        }
    }
    cleaned
}

/// Aggregation façade shared by CLI and GUI. Owns no state: every call
/// re-reads provider stores (ADR-2, live reads, no index).
pub struct SessionStore {
    providers: Vec<Box<dyn Provider>>,
}

impl SessionStore {
    pub fn new(providers: Vec<Box<dyn Provider>>) -> Self {
        Self { providers }
    }

    pub fn providers(&self) -> &[Box<dyn Provider>] {
        &self.providers
    }

    pub fn provider(&self, id: &str) -> Option<&dyn Provider> {
        self.providers.iter().find(|p| p.id() == id).map(|p| p.as_ref())
    }

    pub fn discover_all(&self, provider_id: Option<&str>) -> Result<Vec<RawSession>> {
        let mut all = Vec::new();
        for provider in &self.providers {
            if provider_id.is_some_and(|id| id != provider.id()) {
                continue;
            }
            all.extend(provider.discover()?);
        }
        Ok(all)
    }

    /// F1: cross-provider project overview, grouped by normalized cwd,
    /// newest first.
    pub fn projects(&self) -> Result<Vec<Project>> {
        let sessions = self.discover_all(None)?;
        let mut grouped: std::collections::HashMap<String, Vec<&RawSession>> =
            std::collections::HashMap::new();
        for session in &sessions {
            grouped.entry(session.project_path.clone()).or_default().push(session);
        }
        let mut projects: Vec<Project> = grouped
            .into_iter()
            .map(|(path, group)| {
                let newest = group.iter().max_by_key(|s| s.mtime).unwrap();
                let last_prompt = self
                    .provider(&newest.provider_id)
                    .and_then(|p| p.quick_title(newest))
                    .filter(|t| !t.is_empty());
                Project {
                    exists: Path::new(&path).is_dir(),
                    id: path.clone(),
                    real_path: path,
                    session_count: group.len(),
                    last_active: newest.mtime,
                    last_prompt,
                    providers: group.iter().map(|s| s.provider_id.clone()).collect::<BTreeSet<_>>(),
                }
            })
            .collect();
        projects.sort_by_key(|project| std::cmp::Reverse(project.last_active));
        Ok(projects)
    }

    /// F2: sessions of one project, newest first.
    pub fn sessions(
        &self,
        project_path: &str,
        provider_id: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<SessionSummary>> {
        let normalized = normalize_path(project_path);
        let mut raws: Vec<RawSession> = self
            .discover_all(provider_id)?
            .into_iter()
            .filter(|raw| raw.project_path == normalized)
            .collect();
        raws.sort_by_key(|raw| std::cmp::Reverse(raw.mtime));
        if let Some(limit) = limit {
            raws.truncate(limit);
        }
        raws.iter()
            .map(|raw| {
                self.provider(&raw.provider_id)
                    .ok_or_else(|| Error::Data(format!("Unknown provider {}", raw.provider_id)))?
                    .summarize(raw)
            })
            .collect()
    }

    /// Resolve a user-supplied project reference: absolute/relative path or a
    /// unique suffix of a known project path.
    pub fn resolve_project(&self, reference: &str) -> Result<String> {
        let normalized = normalize_path(reference);
        let known: BTreeSet<String> = self
            .discover_all(None)?
            .into_iter()
            .map(|raw| raw.project_path)
            .collect();
        if known.contains(&normalized) {
            return Ok(normalized);
        }
        let suffix_matches: Vec<&String> = known
            .iter()
            .filter(|path| {
                path.ends_with(&format!("/{reference}"))
                    || Path::new(path).file_name().is_some_and(|n| n == reference)
            })
            .collect();
        match suffix_matches.len() {
            1 => Ok(suffix_matches[0].clone()),
            0 => Err(Error::Data(format!("No project matches \"{reference}\""))),
            _ => Err(Error::Usage(format!(
                "Ambiguous project \"{reference}\": {}",
                suffix_matches.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            ))),
        }
    }

    /// Resolve a session by composite id ("provider:native-id"), full native
    /// id, or unique native-id prefix.
    pub fn resolve_session(&self, reference: &str) -> Result<(&dyn Provider, RawSession)> {
        let matches: Vec<RawSession> = self
            .discover_all(None)?
            .into_iter()
            .filter(|raw| {
                raw.composite_id() == reference
                    || raw.native_id == reference
                    || raw.native_id.starts_with(reference)
            })
            .collect();
        match matches.len() {
            1 => {
                let raw = matches.into_iter().next().unwrap();
                let provider = self
                    .provider(&raw.provider_id)
                    .ok_or_else(|| Error::Data(format!("Unknown provider {}", raw.provider_id)))?;
                Ok((provider, raw))
            }
            0 => Err(Error::Data(format!("No session matches \"{reference}\""))),
            _ => Err(Error::Usage(format!(
                "Ambiguous session id \"{reference}\": {}…",
                matches.iter().take(5).map(|raw| raw.composite_id()).collect::<Vec<_>>().join(", ")
            ))),
        }
    }

    /// F5 precondition: a session can only be resumed into an existing project
    /// directory, and only by a provider that declares the capability.
    pub fn resume_spec_for(&self, reference: &str) -> Result<ResumeSpec> {
        let (provider, raw) = self.resolve_session(reference)?;
        if !provider.capabilities().resumable {
            return Err(Error::Unsupported(format!(
                "Provider {} does not support resume",
                provider.id()
            )));
        }
        let summary = provider.summarize(&raw)?;
        let spec = provider.resume_spec(&summary).ok_or_else(|| {
            Error::Unsupported(format!("Session {} cannot be resumed", summary.id))
        })?;
        if !Path::new(&spec.project_path).is_dir() {
            return Err(Error::ResumePrecondition(format!(
                "Project path no longer exists: {}",
                spec.project_path
            )));
        }
        Ok(spec)
    }
}
