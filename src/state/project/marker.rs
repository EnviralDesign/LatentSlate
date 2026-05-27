use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A marker (point-in-time annotation)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Marker {
    /// Unique identifier
    pub id: Uuid,
    /// Marker track this marker belongs to. Older projects may omit this and
    /// are treated as belonging to the first marker track.
    #[serde(default)]
    pub track_id: Option<Uuid>,
    /// Time position in seconds
    pub time: f64,
    /// Optional label
    pub label: Option<String>,
    /// Optional description
    pub description: Option<String>,
    /// Optional color (hex string, e.g., "#f97316")
    pub color: Option<String>,
}

impl Marker {
    /// Create a new marker at the given time
    #[allow(dead_code)]
    pub fn new(time: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            track_id: None,
            time,
            label: None,
            description: None,
            color: None,
        }
    }

    /// Create a marker with a label
    #[allow(dead_code)]
    pub fn with_label(time: f64, label: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            track_id: None,
            time,
            label: Some(label.into()),
            description: None,
            color: None,
        }
    }
}
