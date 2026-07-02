use crate::error::Result;
use crate::provider::ResumeSpec;

/// ADR-4: one implementation per shell — the CLI chdir+execs, the GUI drives
/// a terminal via osascript. Core only defines the contract and the
/// precondition check (`SessionStore::resume_spec_for`).
pub trait Resumer {
    fn resume(&self, spec: &ResumeSpec) -> Result<()>;
}
