use std::path::PathBuf;

use grep_regex::RegexMatcherBuilder;
use grep_searcher::sinks::UTF8;
use grep_searcher::SearcherBuilder;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::error::{Error, Result};
use crate::provider::Provider;

#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub project_path: Option<String>,
    pub provider_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub provider_id: String,
    /// Composite session id ("provider:native-id").
    pub session_id: String,
    pub native_session_id: String,
    pub project_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_uuid: Option<String>,
    pub snippet: String,
}

impl SearchHit {
    pub fn new(
        provider_id: &str,
        native_session_id: String,
        project_path: String,
        message_uuid: Option<String>,
        snippet: String,
    ) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            session_id: format!("{provider_id}:{native_session_id}"),
            native_session_id,
            project_path,
            message_uuid,
            snippet,
        }
    }
}

pub trait SearchEngine {
    fn search(&self, query: &str, scope: &Scope) -> Result<Vec<SearchHit>>;
}

/// ADR-3: full-text search links the ripgrep engine crates directly — no
/// index, no external binary. Matched lines are handed back to the owning
/// provider so format knowledge stays out of the generic layer.
pub struct GrepSearchEngine<'a> {
    providers: &'a [Box<dyn Provider>],
    /// Cap per file: a session that mentions the query hundreds of times
    /// still only needs a few hits to be findable.
    max_hits_per_file: usize,
}

impl<'a> GrepSearchEngine<'a> {
    pub fn new(providers: &'a [Box<dyn Provider>]) -> Self {
        Self {
            providers,
            max_hits_per_file: 50,
        }
    }
}

impl SearchEngine for GrepSearchEngine<'_> {
    fn search(&self, query: &str, scope: &Scope) -> Result<Vec<SearchHit>> {
        if query.is_empty() {
            return Err(Error::Usage("Empty search query".to_string()));
        }
        let matcher = RegexMatcherBuilder::new()
            .case_insensitive(true)
            .fixed_strings(true)
            .build(query)
            .map_err(|e| Error::Data(format!("Bad search pattern: {e}")))?;
        let mut hits = Vec::new();
        for provider in self.providers {
            if scope
                .provider_id
                .as_deref()
                .is_some_and(|id| id != provider.id())
            {
                continue;
            }
            let files: Vec<PathBuf> = provider
                .search_roots(scope.project_path.as_deref())
                .iter()
                .flat_map(|root| WalkDir::new(root).into_iter().filter_map(|e| e.ok()))
                .filter(|entry| entry.file_type().is_file())
                .map(|entry| entry.into_path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
                .collect();
            // Multi-GB stores are normal; grep files in parallel (rg does the
            // same). Still no index (ADR-2).
            use rayon::prelude::*;
            let cap = self.max_hits_per_file;
            let file_hits: Vec<Vec<SearchHit>> = files
                .par_iter()
                .map(|path| {
                    let mut matched_lines: Vec<(u64, String)> = Vec::new();
                    // sinks::UTF8 requires line numbers to be enabled.
                    let mut searcher = SearcherBuilder::new().line_number(true).build();
                    let sink = UTF8(|line_number, line| {
                        matched_lines.push((line_number, line.to_string()));
                        Ok(matched_lines.len() < cap)
                    });
                    if searcher.search_path(&matcher, path, sink).is_err() {
                        return Vec::new(); // unreadable file: skip, never abort search
                    }
                    matched_lines
                        .iter()
                        .filter_map(|(line_number, line)| {
                            provider.resolve_hit(path, *line_number, line, query)
                        })
                        .collect()
                })
                .collect();
            hits.extend(file_hits.into_iter().flatten());
            // ADR-3 degrade path: compressed transcripts the engine cannot grep.
            hits.extend(provider.search_compressed(query, scope.project_path.as_deref()));
        }
        // Providers whose storage cannot narrow by project (date-organized
        // Codex) return all hits; enforce the scope generically here.
        if let Some(project) = scope.project_path.as_deref() {
            let normalized = crate::store::normalize_path(project);
            hits.retain(|hit| hit.project_path == normalized);
        }
        Ok(hits)
    }
}

/// Convenience for shells: search across a store's providers.
pub fn search_store(
    store: &crate::store::SessionStore,
    query: &str,
    scope: &Scope,
) -> Result<Vec<SearchHit>> {
    GrepSearchEngine::new(store.providers()).search(query, scope)
}

pub type SearchRoots = Vec<PathBuf>;
