pub mod display;
pub mod parse;
pub mod preprocess;
pub mod summary;
pub mod types;

use display::to_display_records;
use fitparser::encode_records;
use preprocess::preprocess_fit;
use summary::derive_workout_data;

pub use parse::parse_fit;
pub use types::{
    DisplayField, DisplayRecord, FitProcessError, ParsedFit, ProcessedFit, ProcessingOptions,
    WorkoutSummary,
};

/// Decode a FIT payload, preprocess it once, and feed downstream derivation.
///
/// The function performs four stages:
/// 1. [`parse::parse_fit`] validates FIT framing and decodes `fitparser` records.
/// 2. [`preprocess::preprocess_fit`] removes or overrides values according to
///    [`ProcessingOptions`].
/// 3. [`summary::derive_workout_data`] calculates derived metrics from the
///    preprocessed records.
/// 4. [`display::to_display_records`] formats the same preprocessed records for
///    UI rendering.
pub fn process_fit_bytes(
    bytes: &[u8],
    options: &ProcessingOptions,
) -> Result<ProcessedFit, FitProcessError> {
    let parsed = parse_fit(bytes)?;
    let (processed_records, preprocessed_records) = preprocess_fit(&parsed, options)?;

    let processed_bytes = encode_records(&processed_records)
        .map_err(|err| FitProcessError::ParseError(err.to_string()))?;
    let derived = derive_workout_data(&preprocessed_records);

    let filtered_records = to_display_records(&preprocessed_records);

    Ok(ProcessedFit {
        records: filtered_records,
        processed_bytes,
        summary: derived.summary,
    })
}
