use std::collections::HashMap;
use std::os::unix::process::CommandExt;

use vibetrail_core::{
    search_store, ContentBlock, Error, Message, Result, Scope, SearchHit, Session, SessionStore,
    SessionSummary,
};

use crate::format;

fn to_json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).map_err(|e| Error::Data(format!("JSON encoding: {e}")))
}

pub fn projects(store: &SessionStore, json: bool) -> Result<()> {
    let projects = store.projects()?;
    if json {
        println!("{}", to_json(&projects)?);
        return Ok(());
    }
    if projects.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }
    for project in projects {
        let marker = if project.exists { " " } else { "!" };
        let providers: Vec<&str> = project.providers.iter().map(String::as_str).collect();
        let mut line = format!(
            "{marker} {} {} {} {}",
            format::pad(&format::abbreviate_path(&project.real_path), 52),
            format::pad(&project.session_count.to_string(), 5),
            format::pad(&format::relative_time(&project.last_active), 12),
            providers.join(","),
        );
        if let Some(prompt) = &project.last_prompt {
            line.push_str(&format!("  {}", format::truncate(prompt, 60)));
        }
        println!("{line}");
    }
    Ok(())
}

pub fn sessions(
    store: &SessionStore,
    project: &str,
    limit: usize,
    provider: Option<&str>,
    json: bool,
) -> Result<()> {
    let project_path = store.resolve_project(project)?;
    let mut handles = store.session_handles(&project_path, provider)?;
    handles.truncate(limit);
    let sessions = store.summarize_handles(&handles);
    if json {
        println!("{}", to_json(&sessions)?);
        return Ok(());
    }
    if sessions.is_empty() {
        println!("No sessions in {}.", format::abbreviate_path(&project_path));
        return Ok(());
    }
    // Resume/fork chain markers: ↳ when the parent is in this project.
    let known: std::collections::HashSet<&str> =
        handles.iter().map(|h| h.native_id.as_str()).collect();
    let parent_of: std::collections::HashMap<&str, &str> = handles
        .iter()
        .filter_map(|h| {
            let parent = h.parent_native_id.as_deref()?;
            known
                .contains(parent)
                .then_some((h.native_id.as_str(), parent))
        })
        .collect();
    for session in sessions {
        let short_id: String = session.native_id.chars().take(8).collect();
        let chain = match parent_of.get(session.native_id.as_str()) {
            Some(parent) => format!("↳ {} ", &parent[..8.min(parent.len())]),
            None => String::new(),
        };
        let mut line = format!(
            "{short_id}  {} {} {} ",
            format::pad(&format::relative_time(&session.mtime), 12),
            format::pad(&format!("{} msg", session.message_count), 9),
            format::pad(&format::duration(session.duration), 6),
        );
        if let Some(branch) = &session.git_branch {
            line.push_str(&format!("{} ", format::pad(branch, 14)));
        }
        line.push_str(&chain);
        line.push_str(&format::truncate(&session.title, 70));
        println!("{line}");
    }
    Ok(())
}

pub fn search(
    store: &SessionStore,
    query: &str,
    project: Option<&str>,
    provider: Option<&str>,
    json: bool,
) -> Result<()> {
    let project_path = project.map(|p| store.resolve_project(p)).transpose()?;
    let scope = Scope {
        project_path,
        provider_id: provider.map(String::from),
    };
    let hits = search_store(store, query, &scope)?;
    if json {
        println!("{}", to_json(&hits)?);
        return Ok(());
    }
    if hits.is_empty() {
        println!("No matches for \"{query}\".");
        return Ok(());
    }
    // F4: results aggregated per session.
    let mut order: Vec<String> = Vec::new();
    let mut grouped: HashMap<String, Vec<&SearchHit>> = HashMap::new();
    for hit in &hits {
        grouped
            .entry(hit.session_id.clone())
            .or_insert_with(|| {
                order.push(hit.session_id.clone());
                Vec::new()
            })
            .push(hit);
    }
    for session_id in order {
        let session_hits = &grouped[&session_id];
        let first = session_hits[0];
        let short_id: String = first.native_session_id.chars().take(8).collect();
        println!(
            "{short_id}  {}  [{}]",
            format::abbreviate_path(&first.project_path),
            first.provider_id
        );
        for hit in session_hits.iter().take(5) {
            println!("    {}", format::truncate(&hit.snippet, 140));
        }
        if session_hits.len() > 5 {
            println!("    … {} more matches", session_hits.len() - 5);
        }
    }
    Ok(())
}

pub fn show(store: &SessionStore, session_id: &str, full: bool, json: bool) -> Result<()> {
    let (provider, raw) = store.resolve_session(session_id)?;
    if full {
        let session = provider.parse(&raw)?;
        if json {
            println!("{}", to_json(&session)?);
            return Ok(());
        }
        print_header(&session.summary);
        print_extensions(&session);
        for message in &session.messages {
            print_full(message);
        }
    } else {
        let stubs = provider.outline(&raw)?;
        if json {
            println!("{}", to_json(&stubs)?);
            return Ok(());
        }
        print_header(&provider.summarize(&raw)?);
        for stub in stubs {
            println!(
                "{} {} {}",
                format::pad(&stub.index.to_string(), 4),
                format::role_icon(stub.role),
                format::truncate(&stub.preview, 110)
            );
        }
    }
    Ok(())
}

fn print_header(summary: &SessionSummary) {
    println!("{}", summary.title);
    let mut meta = format!(
        "{} · {} · {} messages · {}",
        summary.id,
        format::abbreviate_path(&summary.project_path),
        summary.message_count,
        format::relative_time(&summary.mtime),
    );
    if let Some(branch) = &summary.git_branch {
        meta.push_str(&format!(" · {branch}"));
    }
    println!("{meta}");
    println!("{}", "─".repeat(80));
}

/// P1 extras carried in provider extensions: token totals, subagents,
/// artifacts.
fn print_extensions(session: &Session) {
    if let Some(usage) = session.extensions.get("usage") {
        let get = |key: &str| usage.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
        println!(
            "tokens: in {} · out {} · cache-read {} · cache-write {}",
            get("inputTokens"),
            get("outputTokens"),
            get("cacheReadTokens"),
            get("cacheCreationTokens"),
        );
    }
    if let Some(subagents) = session
        .extensions
        .get("subagents")
        .and_then(|v| v.as_array())
    {
        for agent in subagents {
            let label = agent
                .get("description")
                .or(agent.get("agentId"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let count = agent
                .get("messageCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            println!("subagent: {label} ({count} messages)");
        }
    }
    if let Some(artifacts) = session
        .extensions
        .get("artifacts")
        .and_then(|v| v.as_array())
    {
        for artifact in artifacts {
            let name = artifact.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            println!("artifact: {name}");
        }
    }
}

fn print_full(message: &Message) {
    println!(
        "{} [{}]",
        format::role_icon(message.role),
        serde_json::to_value(message.role)
            .unwrap()
            .as_str()
            .unwrap_or("?")
    );
    for block in &message.blocks {
        match block {
            ContentBlock::Text { text } => println!("{text}"),
            ContentBlock::ToolUse { name, .. } => println!("  ⚙ tool_use: {name}"),
            ContentBlock::ToolResult { summary, truncated } => {
                let first_line = summary.lines().next().unwrap_or("");
                let ellipsis = if *truncated { " …" } else { "" };
                println!(
                    "  → tool_result: {}{ellipsis}",
                    format::truncate(first_line, 100)
                );
            }
            ContentBlock::Thinking { .. } => println!("  ✳ thinking (collapsed)"),
        }
    }
    println!();
}

/// ADR-4 CLI resume: chdir into the project, then exec the provider's resume
/// command, replacing this process. Never returns on success.
pub fn resume(store: &SessionStore, session_id: &str) -> Result<()> {
    let spec = store.resume_spec_for(session_id)?;
    let (program, args) = spec
        .command
        .split_first()
        .ok_or_else(|| Error::Data("Empty resume command".to_string()))?;
    let error = std::process::Command::new(program)
        .args(args)
        .current_dir(&spec.project_path)
        .exec();
    // Only reached when exec failed.
    Err(Error::Data(format!(
        "Failed to exec {}: {error}",
        spec.command.join(" ")
    )))
}

pub fn open_gui(project: Option<&str>) -> Result<()> {
    let mut command = std::process::Command::new("/usr/bin/open");
    command.args(["-a", "VibeTrail"]);
    if let Some(project) = project {
        command.arg(project);
    }
    let status = command
        .status()
        .map_err(|e| Error::Data(format!("Failed to run open: {e}")))?;
    if !status.success() {
        return Err(Error::Data(
            "VibeTrail.app not found; build it with `cargo tauri build`".to_string(),
        ));
    }
    Ok(())
}
