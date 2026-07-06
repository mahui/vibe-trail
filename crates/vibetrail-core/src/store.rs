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
    let resolved = expanded
        .canonicalize()
        .unwrap_or_else(|_| lexical_clean(&expanded));
    let text = resolved.to_string_lossy();
    let trimmed = text.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
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
        self.providers
            .iter()
            .find(|p| p.id() == id)
            .map(|p| p.as_ref())
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
            grouped
                .entry(session.project_path.clone())
                .or_default()
                .push(session);
        }
        use rayon::prelude::*;
        let mut projects: Vec<Project> = grouped
            .into_par_iter()
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
                    providers: group
                        .iter()
                        .map(|s| s.provider_id.clone())
                        .collect::<BTreeSet<_>>(),
                }
            })
            .collect();
        projects.sort_by_key(|project| std::cmp::Reverse(project.last_active));
        Ok(projects)
    }

    /// Providers that can act as a handoff target (accept an initial prompt
    /// at launch), in registration order.
    pub fn handoff_targets(&self) -> Vec<&'static str> {
        self.providers
            .iter()
            .filter(|p| p.launch_with_prompt("/", "").is_some())
            .map(|p| p.id())
            .collect()
    }

    /// Launch spec for continuing in `target` at the project with a handoff
    /// prompt. Same precondition as resume: the project path must exist.
    pub fn handoff_spec(
        &self,
        target: &str,
        project_path: &str,
        prompt: &str,
    ) -> Result<crate::provider::ResumeSpec> {
        if !Path::new(project_path).is_dir() {
            return Err(Error::ResumePrecondition(format!(
                "Project path does not exist: {project_path}"
            )));
        }
        self.provider(target)
            .and_then(|p| p.launch_with_prompt(project_path, prompt))
            .ok_or_else(|| Error::Unsupported(format!("{target} cannot take a handoff prompt")))
    }

    /// Every provider's project-scoped memory documents for one project,
    /// provider order preserved (each provider orders its own docs).
    pub fn project_memory(&self, project_path: &str) -> Vec<crate::model::MemoryDoc> {
        let normalized = normalize_path(project_path);
        self.providers
            .iter()
            .flat_map(|provider| provider.project_memory(&normalized))
            .collect()
    }

    /// Every provider's custom-agent definitions for one project.
    pub fn project_agents(&self, project_path: &str) -> Vec<crate::model::AgentDef> {
        let normalized = normalize_path(project_path);
        self.providers
            .iter()
            .flat_map(|provider| provider.project_agents(&normalized))
            .collect()
    }

    /// F2 discovery half: every session handle of one project, newest first.
    /// Metadata only — pair with `summarize_handles` to page in summaries
    /// without re-running discovery per page.
    pub fn session_handles(
        &self,
        project_path: &str,
        provider_id: Option<&str>,
    ) -> Result<Vec<RawSession>> {
        let normalized = normalize_path(project_path);
        let mut raws: Vec<RawSession> = self
            .discover_all(provider_id)?
            .into_iter()
            .filter(|raw| raw.project_path == normalized)
            .collect();
        raws.sort_by_key(|raw| std::cmp::Reverse(raw.mtime));
        Ok(raws)
    }

    /// F2 summary half: parallel full-parse of a page of handles, order
    /// preserved. Handles whose files vanished since discovery (agents prune
    /// old sessions) are skipped, never fatal.
    pub fn summarize_handles(&self, handles: &[RawSession]) -> Vec<SessionSummary> {
        use rayon::prelude::*;
        handles
            .par_iter()
            .filter_map(|raw| self.provider(&raw.provider_id)?.summarize(raw).ok())
            .collect()
    }

    /// F2: sessions of one project, newest first.
    pub fn sessions(
        &self,
        project_path: &str,
        provider_id: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<SessionSummary>> {
        let mut raws = self.session_handles(project_path, provider_id)?;
        if let Some(limit) = limit {
            raws.truncate(limit);
        }
        Ok(self.summarize_handles(&raws))
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
                suffix_matches
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    /// Resolve a session by composite id ("provider:native-id"), full native
    /// id, or unique native-id prefix. A composite id short-circuits to its
    /// provider, and lookup goes through `Provider::find` — both keep session
    /// opens from paying a whole-store discovery.
    pub fn resolve_session(&self, reference: &str) -> Result<(&dyn Provider, RawSession)> {
        let (provider_hint, native_reference) = match reference.split_once(':') {
            Some((prefix, rest)) if self.provider(prefix).is_some() => (Some(prefix), rest),
            _ => (None, reference),
        };
        let mut matches: Vec<RawSession> = Vec::new();
        for provider in &self.providers {
            if provider_hint.is_some_and(|id| id != provider.id()) {
                continue;
            }
            matches.extend(provider.find(native_reference)?);
        }
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
                matches
                    .iter()
                    .take(5)
                    .map(|raw| raw.composite_id())
                    .collect::<Vec<_>>()
                    .join(", ")
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
        let spec = provider.resume_spec(&raw).ok_or_else(|| {
            Error::Unsupported(format!("Session {} cannot be resumed", raw.composite_id()))
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
