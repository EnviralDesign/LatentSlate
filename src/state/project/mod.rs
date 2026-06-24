//! Project data model
//!
//! This module contains the core data structures for a video project.

mod clip;
mod marker;
mod persistence;
mod project;
mod settings;
mod track;

pub use clip::{Clip, ClipImageMode, ClipTransform};
pub use marker::Marker;
pub use project::{Project, ProjectWorkspaceLayout};
pub use settings::{ProjectProviderScope, ProjectSettings};
pub use track::{Track, TrackType};
