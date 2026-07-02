/// Core error taxonomy. `exit_code` matches the CLI contract in TECH_SPEC §6:
/// 1 usage, 2 data, 3 resume precondition, 4 unsupported by provider.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Data(String),
    #[error("{0}")]
    ResumePrecondition(String),
    #[error("{0}")]
    Unsupported(String),
}

impl Error {
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Usage(_) => 1,
            Error::Data(_) => 2,
            Error::ResumePrecondition(_) => 3,
            Error::Unsupported(_) => 4,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
