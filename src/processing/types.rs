use std::fmt;

/// Simplified representation of a FIT field for display in the UI.
#[derive(Debug, Clone)]
pub struct DisplayField {
    pub name: String,
    pub value: String,
}

/// Human-readable wrapper around a parsed FIT data record.
#[derive(Debug, Clone)]
pub struct DisplayRecord {
    pub message_type: String,
    pub fields: Vec<DisplayField>,
}

/// Processed FIT output returned to the web handler.
#[derive(Debug, Clone)]
pub struct ProcessedFit {
    /// Fields formatted for rendering.
    pub records: Vec<DisplayRecord>,
    /// Re-encoded FIT payload, optionally with filtered data fields.
    pub processed_bytes: Vec<u8>,
    /// Summary metrics extracted from the FIT payload.
    pub summary: WorkoutSummary,
}

/// User-facing toggles that adjust how FIT bytes are rewritten.
#[derive(Debug, Clone, Default)]
pub struct ProcessingOptions {
    /// Drop `speed` and `enhanced_speed` fields from record messages.
    pub remove_speed_fields: bool,
    /// Smooth derived speed values using a sliding window before presenting them.
    pub smooth_speed: bool,
}

/// Derived overview metrics from the FIT records.
#[derive(Debug, Clone, Default)]
pub struct WorkoutSummary {
    pub duration_seconds: Option<f64>,
    pub workout_type: Option<String>,
    pub distance_meters: Option<f64>,
    pub speed_min: Option<f64>,
    pub speed_mean: Option<f64>,
    pub speed_max: Option<f64>,
    pub heart_rate_min: Option<f64>,
    pub heart_rate_mean: Option<f64>,
    pub heart_rate_max: Option<f64>,
}

/// Default window size (in samples) for moving-average speed smoothing.
pub const SPEED_SMOOTHING_WINDOW: usize = 5;

#[derive(Debug, Default)]
pub struct DerivedWorkoutData {
    pub summary: WorkoutSummary,
}

#[derive(Debug)]
pub enum FitProcessError {
    ParseError(String),
}

impl fmt::Display for FitProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FitProcessError::ParseError(msg) => write!(f, "Failed to decode FIT file: {msg}"),
        }
    }
}

impl std::error::Error for FitProcessError {}
