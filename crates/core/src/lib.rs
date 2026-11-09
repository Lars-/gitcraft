//! The repository model behind GitCraft.
//!
//! Reading the history goes through gitoxide in-process, because the graph needs millions of
//! lookups. Everything else (status, diffs and every change to the repository) runs the user's own
//! `git`, so their config, hooks, credentials and signing apply unchanged. What git asks while it
//! runs, such as a password, it asks the user through GitCraft ([`askpass`]).
#![warn(missing_docs)]

pub mod askpass;
pub mod conflict;
pub mod diff;
mod error;
pub mod git;
pub mod graph;
pub mod repo;
pub mod status;
pub mod undo;
pub mod watch;

pub use error::{Error, Result};
pub use gix::{ObjectId, date};
