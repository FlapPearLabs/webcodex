//! Shared filesystem inspection and Git checkpoint implementations.

pub mod file_read_normalize;
pub mod file_read_range;
// Public for one reason: `webcodex-runner`'s project catalog is a model-reachable
// git reader that lives outside this crate, and routing it through the broker
// requires a narrow public facade rather than a second, unaudited `Command`.
// Only the read/bounded entry points and their outcome types are public; the
// plan derivation and the spec builder stay private to this crate so the
// authority cannot be reassembled by a caller.
pub mod git_broker;
pub mod path_policy;
pub mod project_context;
pub mod project_overview;
#[cfg(feature = "workspace-checkpoints")]
pub mod workspace_checkpoint;
