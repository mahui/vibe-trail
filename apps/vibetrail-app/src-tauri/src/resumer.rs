use vibetrail_core::{Error, Result, Resumer, ResumeSpec};

/// ADR-4 GUI resume: drive Terminal.app via osascript to cd into the project
/// and run the provider's resume command. First use triggers the system
/// Automation permission prompt.
pub struct TerminalResumer;

impl Resumer for TerminalResumer {
    fn resume(&self, spec: &ResumeSpec) -> Result<()> {
        let shell_command = format!(
            "cd {} && {}",
            shell_quote(&spec.project_path),
            spec.command.iter().map(|arg| shell_quote(arg)).collect::<Vec<_>>().join(" ")
        );
        let script = format!(
            "tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell",
            applescript_escape(&shell_command)
        );
        let output = std::process::Command::new("/usr/bin/osascript")
            .args(["-e", &script])
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
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn applescript_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}
