#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod resumer;

use vibetrail_core::{
    search_store, ClaudeCodeProvider, CodexProvider, Project, Resumer, Scope, SearchHit, Session,
    SessionStore, SessionSummary,
};

/// Stores are stateless (live reads, ADR-2), so each command builds one.
fn store() -> SessionStore {
    SessionStore::new(vec![
        Box::new(ClaudeCodeProvider::new(None)),
        Box::new(CodexProvider::new(None)),
    ])
}

#[tauri::command]
fn list_projects() -> Result<Vec<Project>, String> {
    store().projects().map_err(|e| e.to_string())
}

#[tauri::command]
fn list_sessions(project: String) -> Result<Vec<SessionSummary>, String> {
    store().sessions(&project, None, None).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_session(session_id: String) -> Result<Session, String> {
    let store = store();
    let (provider, raw) = store.resolve_session(&session_id).map_err(|e| e.to_string())?;
    provider.parse(&raw).map_err(|e| e.to_string())
}

#[tauri::command]
fn search(query: String, project: Option<String>) -> Result<Vec<SearchHit>, String> {
    let scope = Scope { project_path: project, provider_id: None };
    search_store(&store(), &query, &scope).map_err(|e| e.to_string())
}

#[tauri::command]
fn can_resume(session_id: String) -> bool {
    store().resume_spec_for(&session_id).is_ok()
}

#[tauri::command]
fn resume_session(session_id: String) -> Result<(), String> {
    let spec = store().resume_spec_for(&session_id).map_err(|e| e.to_string())?;
    resumer::TerminalResumer.resume(&spec).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_projects,
            list_sessions,
            get_session,
            search,
            can_resume,
            resume_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VibeTrail");
}
