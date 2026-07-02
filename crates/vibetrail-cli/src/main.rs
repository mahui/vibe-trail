mod commands;
mod format;

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use vibetrail_core::{AntigravityProvider, ClaudeCodeProvider, CodexProvider, SessionStore};

#[derive(Parser)]
#[command(
    name = "vibetrail",
    version,
    about = "Session browser & resume for coding agents."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List projects across all providers (F1).
    Projects {
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
    /// List sessions of a project, newest first (F2).
    Sessions {
        /// Project path, or a unique directory-name suffix.
        project: String,
        /// Maximum sessions to list.
        #[arg(short = 'n', default_value_t = 20)]
        limit: usize,
        /// Restrict to one provider id.
        #[arg(long)]
        provider: Option<String>,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
    /// Full-text search across providers and projects (F4).
    Search {
        /// Literal text to search for.
        query: String,
        /// Restrict to one project.
        #[arg(short = 'p', long)]
        project: Option<String>,
        /// Restrict to one provider id.
        #[arg(long)]
        provider: Option<String>,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show one session as outline (default) or full transcript (F3).
    Show {
        /// Session id (or unique prefix).
        session_id: String,
        /// Outline view (default).
        #[arg(long)]
        outline: bool,
        /// Full transcript.
        #[arg(long, conflicts_with = "outline")]
        full: bool,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
    /// Resume a session in its project directory (F5).
    Resume {
        /// Session id (or unique prefix).
        session_id: String,
    },
    /// Open the VibeTrail GUI app.
    Open {
        /// Optional project to open at.
        project: Option<String>,
    },
}

fn store() -> SessionStore {
    SessionStore::new(vec![
        Box::new(ClaudeCodeProvider::new(None)),
        Box::new(CodexProvider::new(None)),
        Box::new(AntigravityProvider::new(None)),
    ])
}

fn main() {
    // Exit-code contract (TECH_SPEC §6): clap's default usage-error code is 2,
    // ours is 1, so parse manually.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let code = match err.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => 0,
                _ => 1,
            };
            let _ = err.print();
            std::process::exit(code);
        }
    };
    let result = match cli.command {
        Command::Projects { json } => commands::projects(&store(), json),
        Command::Sessions {
            project,
            limit,
            provider,
            json,
        } => commands::sessions(&store(), &project, limit, provider.as_deref(), json),
        Command::Search {
            query,
            project,
            provider,
            json,
        } => commands::search(
            &store(),
            &query,
            project.as_deref(),
            provider.as_deref(),
            json,
        ),
        Command::Show {
            session_id,
            full,
            json,
            ..
        } => commands::show(&store(), &session_id, full, json),
        Command::Resume { session_id } => commands::resume(&store(), &session_id),
        Command::Open { project } => commands::open_gui(project.as_deref()),
    };
    if let Err(error) = result {
        eprintln!("vibetrail: {error}");
        std::process::exit(error.exit_code());
    }
}
