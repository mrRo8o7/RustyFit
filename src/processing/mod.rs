pub mod display;
pub mod parse;
pub mod preprocess;
pub mod summary;
pub mod types;

use display::to_display_records;
use preprocess::{preprocess_fit, reencode_fit_with_section};
use summary::derive_workout_data;

pub use parse::parse_fit;
pub use types::{
    DisplayField, DisplayRecord, FitProcessError, ParsedFit, ProcessedFit, ProcessingOptions,
    WorkoutSummary,
};

/// Decode a FIT payload, preprocess it once, and feed downstream derivation.
///
/// The function performs four stages:
/// 1. [`parse::parse_fit`] splits the payload into header, data section, and parsed
///    `fitparser` records.
/// 2. [`preprocess::preprocess_fit`] removes or overrides values at the byte and
///    record level according to [`ProcessingOptions`].
/// 3. [`summary::derive_workout_data`] calculates derived metrics from the
///    preprocessed records.
/// 4. [`display::to_display_records`] formats the same preprocessed records for
///    UI rendering.
pub fn process_fit_bytes(
    bytes: &[u8],
    options: &ProcessingOptions,
) -> Result<ProcessedFit, FitProcessError> {
    let parsed = parse_fit(bytes)?;
    let (processed_data_section, preprocessed_records) = preprocess_fit(&parsed, options)?;

    let processed_bytes = reencode_fit_with_section(&parsed, processed_data_section)?;
    let derived = derive_workout_data(&preprocessed_records);

    let filtered_records = to_display_records(&preprocessed_records);

    Ok(ProcessedFit {
        records: filtered_records,
        processed_bytes,
        summary: derived.summary,
    })
}
