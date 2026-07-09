//! Reusable taskmd library layer: config, task model, safe I/O, and the
//! dependency graph. Everything else (verbs, query, ranking, web) builds on
//! this; the recurring generator predates it and keeps its own thin scanner in
//! [`crate::task`].
//!
//! The compatibility contract is the taskmd *file format* (spec 1.2, taskmd
//! 0.2.5): karamd's CLI surface may differ from taskmd's, but every file this
//! layer writes must parse identically in taskmd/Obsidian, and reading back a
//! file taskmd wrote must drop nothing (unknown frontmatter fields are
//! preserved as-is, CRLF is tolerated).

pub mod config;
pub mod graph;
pub mod model;
pub mod store;
pub mod template;

pub use config::{
    AgentSpec, Config, IdConfig, IdStrategy, Phase, PromptVia, RunConfig, Scope, Workflow,
};
pub use graph::{Graph, GraphIssue};
pub use model::{Effort, ParseOutcome, Priority, Status, Task, TaskType, VerifyCheck};
pub use store::{Entropy, Scan, SystemEntropy, Vault};
