//! The repository model behind GitCraft.
//!
//! Reading the history goes through gitoxide in-process, because the graph needs millions of
//! lookups. Everything else (status, diffs and every change to the repository) runs the user's own
//! `git`, so their config, hooks, credentials and signing apply unchanged.
#![warn(missing_docs)]

pub mod diff;
mod error;
pub mod git;
pub mod graph;
pub mod repo;
pub mod status;
pub mod watch;

pub use error::{Error, Result};
pub use gix::{ObjectId, date};
