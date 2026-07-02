#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod config;
mod resumer;

use vibetrail_core::{
    search_store, AntigravityProvider, ClaudeCodeProvider, CodexProvider, Message, Project,
    RawSession, Scope, SearchHit, Session, SessionStore, SessionSummary,
};

/// Stores are stateless (live reads, ADR-2), so each command builds one.
fn store() -> SessionStore {
    SessionStore::new(vec![
        Box::new(ClaudeCodeProvider::new(None)),
        Box::new(CodexProvider::new(None)),
        Box::new(AntigravityProvider::new(None)),
    ])
}

#[tauri::command]
fn list_projects() -> Result<Vec<Project>, String> {
    store().projects().map_err(|e| e.to_string())
}

/// F2 in two halves so a 700-session project neither full-parses everything
/// on one click nor re-discovers the whole store per scroll page: the
/// frontend fetches handles once, then trades pages of them for summaries.
#[tauri::command]
fn list_session_handles(project: String) -> Result<Vec<RawSession>, String> {
    store()
        .session_handles(&project, None)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn summarize_sessions(handles: Vec<RawSession>) -> Vec<SessionSummary> {
    store().summarize_handles(&handles)
}

#[tauri::command]
fn get_session(session_id: String) -> Result<Session, String> {
    let store = store();
    let (provider, raw) = store
        .resolve_session(&session_id)
        .map_err(|e| e.to_string())?;
    provider.parse(&raw).map_err(|e| e.to_string())
}

#[tauri::command]
fn search(query: String, project: Option<String>) -> Result<Vec<SearchHit>, String> {
    let scope = Scope {
        project_path: project,
        provider_id: None,
    };
    search_store(&store(), &query, &scope).map_err(|e| e.to_string())
}

/// Capability + path existence only — the detail view already holds the
/// parsed session, so this must not re-discover or re-parse anything.
#[tauri::command]
fn can_resume(provider_id: String, project_path: String) -> bool {
    store()
        .provider(&provider_id)
        .is_some_and(|p| p.capabilities().resumable)
        && std::path::Path::new(&project_path).is_dir()
}

#[tauri::command]
fn resume_session(session_id: String) -> Result<Option<String>, String> {
    let spec = store()
        .resume_spec_for(&session_id)
        .map_err(|e| e.to_string())?;
    resumer::resume(&spec, config::load().terminal).map_err(|e| e.to_string())
}

/// Untruncated message for "load full output": parse() truncates tool
/// results for display; this re-reads just one message from disk.
#[tauri::command]
fn get_message_full(session_id: String, message_uuid: String) -> Result<Option<Message>, String> {
    let store = store();
    let (provider, raw) = store
        .resolve_session(&session_id)
        .map_err(|e| e.to_string())?;
    provider
        .message_full(&raw, &message_uuid)
        .map_err(|e| e.to_string())
}

/// Rendered-markdown links open in the default browser, never inside the
/// webview. Scheme whitelist keeps file:// and custom schemes out.
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Only http(s) links can be opened".to_string());
    }
    std::process::Command::new("/usr/bin/open")
        .arg(&url)
        .status()
        .map_err(|e| e.to_string())
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err("Failed to open link".to_string())
            }
        })
}

/// Clipboard via pbcopy: the webview's navigator.clipboard is not reliable
/// in a custom-protocol context.
#[tauri::command]
fn copy_to_clipboard(text: String) -> Result<(), String> {
    use std::io::Write;
    let mut child = std::process::Command::new("/usr/bin/pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .as_mut()
        .ok_or("pbcopy stdin unavailable")?
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("pbcopy failed".to_string())
    }
}

#[tauri::command]
fn get_config() -> config::AppConfig {
    config::load()
}

#[tauri::command]
fn set_config(config: config::AppConfig) -> Result<(), String> {
    config::save(&config)
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_projects,
            list_session_handles,
            summarize_sessions,
            get_session,
            get_message_full,
            search,
            can_resume,
            resume_session,
            open_external,
            copy_to_clipboard,
            get_config,
            set_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VibeTrail");
}
