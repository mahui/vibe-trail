#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod config;
mod resumer;

use vibetrail_core::{
    search_store, AgentDef, MemoryDoc, Message, Project, RawSession, Scope, SearchHit, Session,
    SessionStore, SessionSummary,
};

/// Stores are stateless (live reads, ADR-2), so each command builds one —
/// from config.json, so provider enable/root settings apply immediately and
/// identically to the CLI (TECH_SPEC §12).
fn store() -> SessionStore {
    vibetrail_core::config::default_store()
}

/// Off-main-thread bridge. Tauri runs synchronous commands on the main
/// thread, so a store-touching command (live file reads, seconds at scale)
/// would freeze rendering, hover and clicks for its whole duration — the
/// single biggest responsiveness killer. Every command that does IO or
/// spawns processes goes through here instead.
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn list_projects() -> Result<Vec<Project>, String> {
    blocking(|| store().projects().map_err(|e| e.to_string())).await
}

/// One whole-store discovery for the frontend's cache warm-up: the project
/// overview already paid this read and threw the handles away — this hands
/// them over so the first click into any project renders instantly.
#[tauri::command]
async fn list_all_handles() -> Result<Vec<RawSession>, String> {
    blocking(|| store().discover_all(None).map_err(|e| e.to_string())).await
}

/// F2 in two halves so a 700-session project neither full-parses everything
/// on one click nor re-discovers the whole store per scroll page: the
/// frontend fetches handles once, then trades pages of them for summaries.
#[tauri::command]
async fn list_session_handles(project: String) -> Result<Vec<RawSession>, String> {
    blocking(move || {
        store()
            .session_handles(&project, None)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn summarize_sessions(handles: Vec<RawSession>) -> Vec<SessionSummary> {
    blocking(move || Ok(store().summarize_handles(&handles)))
        .await
        .unwrap_or_default()
}

/// F7: agent-persisted project memory, read-only.
#[tauri::command]
async fn get_project_memory(project: String) -> Result<Vec<MemoryDoc>, String> {
    blocking(move || Ok(store().project_memory(&project))).await
}

/// Custom-agent roster for a project, read-only.
#[tauri::command]
async fn get_project_agents(project: String) -> Result<Vec<AgentDef>, String> {
    blocking(move || Ok(store().project_agents(&project))).await
}

/// Handoff (TECH_SPEC §14): capsule + rendered prompt + available targets,
/// one call so the panel opens with everything it needs.
#[tauri::command]
async fn get_handoff(session_id: String) -> Result<serde_json::Value, String> {
    blocking(move || {
        let store = store();
        let (provider, raw) = store
            .resolve_session(&session_id)
            .map_err(|e| e.to_string())?;
        let session = provider.parse(&raw).map_err(|e| e.to_string())?;
        let capsule = vibetrail_core::HandoffCapsule::from_session(&session);
        let project_exists = std::path::Path::new(&capsule.project_path).is_dir();
        Ok(serde_json::json!({
            "prompt": capsule.prompt(),
            "capsule": capsule,
            "targets": store.handoff_targets(),
            "projectExists": project_exists,
        }))
    })
    .await
}

/// Continue the session in `target`: terminal agents get the prompt as a
/// launch argument; GUI clients (Cursor) get the project opened and the
/// prompt on the clipboard.
#[tauri::command]
async fn handoff_continue(session_id: String, target: String) -> Result<Option<String>, String> {
    blocking(move || {
        let store = store();
        let (provider, raw) = store
            .resolve_session(&session_id)
            .map_err(|e| e.to_string())?;
        let session = provider.parse(&raw).map_err(|e| e.to_string())?;
        let capsule = vibetrail_core::HandoffCapsule::from_session(&session);
        let prompt = capsule.prompt();
        let spec = store
            .handoff_spec(&target, &capsule.project_path, &prompt)
            .map_err(|e| e.to_string())?;
        if spec.launch == vibetrail_core::LaunchMode::GuiApp {
            resumer::set_clipboard(&prompt).map_err(|e| e.to_string())?;
            resumer::resume(&spec, config::load().terminal).map_err(|e| e.to_string())?;
            return Ok(Some(
                "App opened at the project — the handoff prompt is on your clipboard; paste it into a new chat."
                    .to_string(),
            ));
        }
        resumer::resume(&spec, config::load().terminal).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn get_session(session_id: String) -> Result<Session, String> {
    blocking(move || {
        let store = store();
        let (provider, raw) = store
            .resolve_session(&session_id)
            .map_err(|e| e.to_string())?;
        provider.parse(&raw).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn search(query: String, project: Option<String>) -> Result<Vec<SearchHit>, String> {
    blocking(move || {
        let scope = Scope {
            project_path: project,
            provider_id: None,
        };
        search_store(&store(), &query, &scope).map_err(|e| e.to_string())
    })
    .await
}

/// Capability + path existence only — the detail view already holds the
/// parsed session, so this must not re-discover or re-parse anything.
#[tauri::command]
async fn can_resume(provider_id: String, project_path: String) -> bool {
    blocking(move || {
        Ok(store()
            .provider(&provider_id)
            .is_some_and(|p| p.capabilities().resumable)
            && std::path::Path::new(&project_path).is_dir())
    })
    .await
    .unwrap_or(false)
}

#[tauri::command]
async fn resume_session(session_id: String) -> Result<Option<String>, String> {
    blocking(move || {
        let spec = store()
            .resume_spec_for(&session_id)
            .map_err(|e| e.to_string())?;
        resumer::resume(&spec, config::load().terminal).map_err(|e| e.to_string())
    })
    .await
}

/// Untruncated message for "load full output": parse() truncates tool
/// results for display; this re-reads just one message from disk.
#[tauri::command]
async fn get_message_full(
    session_id: String,
    message_uuid: String,
) -> Result<Option<Message>, String> {
    blocking(move || {
        let store = store();
        let (provider, raw) = store
            .resolve_session(&session_id)
            .map_err(|e| e.to_string())?;
        provider
            .message_full(&raw, &message_uuid)
            .map_err(|e| e.to_string())
    })
    .await
}

/// Rendered-markdown links open in the default browser, never inside the
/// webview. Scheme whitelist keeps file:// and custom schemes out.
#[tauri::command]
async fn open_external(url: String) -> Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Only http(s) links can be opened".to_string());
    }
    blocking(move || {
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
    })
    .await
}

/// Clipboard via pbcopy: the webview's navigator.clipboard is not reliable
/// in a custom-protocol context.
#[tauri::command]
async fn copy_to_clipboard(text: String) -> Result<(), String> {
    blocking(move || {
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
    })
    .await
}

/// Self-update (tauri-plugin-updater): manifest is `latest.json` on the
/// GitHub release, artifacts are minisign-verified against the pubkey in
/// tauri.conf.json. The frontend drives the flow — a background check on
/// boot and a manual check in Settings; installs never happen silently.
#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(Some(update.version.clone())),
        Ok(None) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("No update available")?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

#[tauri::command]
fn app_version(app: tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
fn get_config() -> config::AppConfig {
    config::load()
}

#[tauri::command]
fn set_config(config: config::AppConfig) -> Result<(), String> {
    config::save(&config)
}

/// Settings pane data: config file location plus each provider's effective
/// discovery state (enabled, root in effect, path validity). Same shape as
/// `vibetrail config --json`.
#[tauri::command]
async fn settings_info() -> Result<vibetrail_core::config::ConfigReport, String> {
    blocking(|| {
        Ok(vibetrail_core::config::report(
            &vibetrail_core::config::load_discovery(),
        ))
    })
    .await
}

/// "Reveal config in Finder": settings are a file first, a UI second. Write
/// the file if it doesn't exist yet so there is something to reveal.
#[tauri::command]
async fn reveal_config() -> Result<(), String> {
    blocking(|| {
        let path = vibetrail_core::config::config_path();
        if !path.exists() {
            config::save(&config::load())?;
        }
        let status = std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(&path)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err("Failed to reveal config file".to_string())
        }
    })
    .await
}

/// Native menu bar: the default Tauri menu (About / Edit / Window …) with a
/// standard macOS "Settings…" item (⌘,) inserted right after "About" in the
/// app menu. The item emits `open-settings`; the frontend opens the pane.
fn install_menu(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, MenuItemKind, PredefinedMenuItem};
    use tauri::Emitter;

    let menu = Menu::default(app.handle())?;
    // Localized only on an explicit choice — "auto" resolution lives in the
    // frontend (navigator.language); the native layer has no cheap locale
    // probe worth adding a plugin for.
    let label = if config::load().language == "zh" {
        "设置…"
    } else {
        "Settings…"
    };
    let settings = MenuItem::with_id(app, "settings", label, true, Some("CmdOrCtrl+,"))?;
    if let Some(MenuItemKind::Submenu(app_menu)) = menu.items()?.into_iter().next() {
        app_menu.insert_items(&[&PredefinedMenuItem::separator(app)?, &settings], 1)?;
    }
    app.set_menu(menu)?;
    app.on_menu_event(|handle, event| {
        if event.id().as_ref() == "settings" {
            let _ = handle.emit("open-settings", ());
        }
    });
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            install_menu(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            list_all_handles,
            list_session_handles,
            summarize_sessions,
            get_project_memory,
            get_project_agents,
            get_handoff,
            handoff_continue,
            get_session,
            get_message_full,
            search,
            can_resume,
            resume_session,
            open_external,
            copy_to_clipboard,
            get_config,
            set_config,
            settings_info,
            reveal_config,
            check_update,
            install_update,
            app_version,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VibeTrail");
}
