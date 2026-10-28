/// What can go wrong.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `git` ran and failed.
    #[error("git {args} failed: {message}")]
    Git {
        /// The arguments it ran with.
        args: String,
        /// What it printed on standard error.
        message: String,
    },
    /// `git` could not be started.
    #[error("could not run git: {0}")]
    Spawn(#[from] std::io::Error),
    /// Reading the repository failed.
    #[error(transparent)]
    Repository(Box<dyn std::error::Error + Send + Sync>),
    /// `git` printed something we do not understand.
    #[error("unexpected output from git: {0}")]
    Parse(String),
}

impl Error {
    pub(crate) fn repository(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Repository(Box::new(error))
    }
}

/// The result of everything in this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
