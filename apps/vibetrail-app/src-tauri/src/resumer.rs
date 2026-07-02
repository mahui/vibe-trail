use std::process::Command;

use vibetrail_core::{Error, Result, ResumeSpec};

use crate::config::TerminalKind;

/// ADR-4 GUI resume, per configured terminal (P1). Terminal.app and iTerm2
/// have first-class AppleScript command execution; Ghostty is driven through
/// its CLI args; Warp has no scriptable "run command" surface, so it degrades
/// to opening the project and putting the command on the clipboard.
///
/// Returns an optional user-facing note describing a degraded path.
pub fn resume(spec: &ResumeSpec, terminal: TerminalKind) -> Result<Option<String>> {
    let shell_command = format!(
        "cd {} && {}",
        shell_quote(&spec.project_path),
        spec.command
            .iter()
            .map(|arg| shell_quote(arg))
            .collect::<Vec<_>>()
            .join(" ")
    );
    match terminal {
        TerminalKind::Terminal => {
            run_osascript(&format!(
                "tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell",
                applescript_escape(&shell_command)
            ))?;
            Ok(None)
        }
        TerminalKind::Iterm2 => {
            run_osascript(&format!(
                "tell application \"iTerm\"\nactivate\nset newWindow to (create window with default profile)\ntell current session of newWindow to write text \"{}\"\nend tell",
                applescript_escape(&shell_command)
            ))?;
            Ok(None)
        }
        TerminalKind::Ghostty => {
            // Ghostty's scripting dictionary (1.3) is an explicit preview —
            // driving it crashed the app in the field (upstream: breaking
            // changes expected in 1.4, new-tab crash issues open). Until it
            // stabilizes: a cold start takes launch args (single instance,
            // no duplicate Dock icon), a running instance degrades to
            // clipboard like Warp.
            let running = Command::new("/usr/bin/osascript")
                .args(["-e", "application \"Ghostty\" is running"])
                .output()
                .ok()
                .map(|out| String::from_utf8_lossy(&out.stdout).trim() == "true")
                .unwrap_or(false);
            if !running {
                let status = Command::new("/usr/bin/open")
                    .args(["-a", "Ghostty", "--args"])
                    .arg(format!("--working-directory={}", spec.project_path))
                    .args(["-e", "sh", "-lc"])
                    .arg(format!("{shell_command}; exec ${{SHELL:-/bin/zsh}}"))
                    .status()
                    .map_err(|e| Error::Data(format!("Failed to launch Ghostty: {e}")))?;
                if !status.success() {
                    return Err(Error::Data(
                        "Ghostty launch failed; is it installed?".to_string(),
                    ));
                }
                return Ok(None);
            }
            run_osascript(&format!(
                "set the clipboard to \"{}\"",
                applescript_escape(&shell_command)
            ))?;
            run_osascript("tell application \"Ghostty\" to activate")?;
            Ok(Some(
                "Ghostty is running — the resume command is on your clipboard; paste it into a new tab. (Ghostty's automation API is still a preview and unstable when driven directly.)"
                    .to_string(),
            ))
        }
        TerminalKind::Warp => {
            run_osascript(&format!(
                "set the clipboard to \"{}\"",
                applescript_escape(&shell_command)
            ))?;
            let url = format!(
                "warp://action/new_window?path={}",
                url_encode(&spec.project_path)
            );
            let status = Command::new("/usr/bin/open")
                .arg(&url)
                .status()
                .map_err(|e| Error::Data(format!("Failed to launch Warp: {e}")))?;
            if !status.success() {
                return Err(Error::Data(
                    "Warp launch failed; is it installed?".to_string(),
                ));
            }
            Ok(Some(
                "Warp opened at the project; the resume command is on your clipboard — paste to run."
                    .to_string(),
            ))
        }
    }
}

fn run_osascript(script: &str) -> Result<()> {
    let output = Command::new("/usr/bin/osascript")
        .args(["-e", script])
        .output()
        .map_err(|e| Error::Data(format!("Failed to run osascript: {e}")))?;
    if !output.status.success() {
        return Err(Error::Data(format!(
            "Terminal automation failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn applescript_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn url_encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}
