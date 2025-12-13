pub mod display;
pub mod parse;
pub mod preprocess;
pub mod summary;
pub mod types;

use display::to_display_records;
use preprocess::{preprocess_data_section, reencode_fit_with_section};
use summary::derive_workout_data;

pub use parse::parse_fit;
pub use types::{
    DisplayField, DisplayRecord, FitProcessError, ParsedFit, ProcessedFit, ProcessingOptions,
    WorkoutSummary,
};

/// Decode a FIT payload, optionally filter speed fields, and re-encode.
///
/// The function performs three stages:
/// 1. [`parse::parse_fit`] splits the payload into header, data section, and parsed
///    `fitparser` records.
/// 2. The parsed records are converted into a format suitable for HTML output
///    and filtered based on [`ProcessingOptions`].
/// 3. The data section is optionally rewritten without speed fields and the
///    file is reconstructed.
pub fn process_fit_bytes(
    bytes: &[u8],
    options: &ProcessingOptions,
) -> Result<ProcessedFit, FitProcessError> {
    let parsed = parse_fit(bytes)?;
    let processed_data_section =
        preprocess_data_section(&parsed.data_section, &parsed.records, options)?;

    let processed_bytes = reencode_fit_with_section(&parsed, processed_data_section)?;
    let processed = parse_fit(&processed_bytes)?;
    let derived = derive_workout_data(&processed.records, options);

    let filtered_records = to_display_records(&processed.records, options);

    Ok(ProcessedFit {
        records: filtered_records,
        processed_bytes,
        summary: derived.summary,
    })
}
