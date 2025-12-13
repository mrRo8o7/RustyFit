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
    DisplayField, DisplayRecord, FitProcessError, ProcessedFit, ProcessingOptions, WorkoutSummary,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::render_processed_records;

    fn fixture_bytes() -> Vec<u8> {
        std::fs::read("test/fixtures/activity.fit").expect("fixture should be present")
    }

    #[test]
    fn round_trip_preserves_record_kinds() {
        let bytes = fixture_bytes();

        let processed = process_fit_bytes(&bytes, &ProcessingOptions::default())
            .expect("processing should succeed");

        let original = parse_fit(&bytes).expect("fixture should decode");
        let redecoded = parse_fit(&processed.processed_bytes).expect("processed bytes decode");

        assert_eq!(original.len(), redecoded.len());
        assert!(
            original
                .iter()
                .zip(&redecoded)
                .all(|(first, second)| first.kind() == second.kind())
        );
    }

    #[test]
    fn processed_download_remains_decodable_without_speed_fields() {
        let bytes = fixture_bytes();

        let processed = process_fit_bytes(
            &bytes,
            &ProcessingOptions {
                remove_speed_fields: true,
                smooth_speed: false,
            },
        )
        .expect("processing should succeed");

        assert!(
            processed
                .records
                .iter()
                .flat_map(|record| &record.fields)
                .all(|field| field.name != "speed" && field.name != "enhanced_speed")
        );

        let download = parse_fit(&processed.processed_bytes).expect("download should decode");
        assert_eq!(download.len(), processed.records.len());
    }

    #[test]
    fn rendered_output_includes_summary_and_download_link() {
        let bytes = fixture_bytes();
        let processed = process_fit_bytes(&bytes, &ProcessingOptions::default())
            .expect("processing should succeed");

        let rendered = render_processed_records(&processed);

        assert!(rendered.contains("Workout Overview"));
        assert!(rendered.contains("Download processed FIT"));
    }
}

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
    let processed_records = preprocess_fit(&parsed, options)?;

    let processed_bytes = encode_records(&processed_records)
        .map_err(|err| FitProcessError::ParseError(err.to_string()))?;
    let derived = derive_workout_data(&processed_records);

    let filtered_records = to_display_records(&processed_records);

    Ok(ProcessedFit {
        records: filtered_records,
        processed_bytes,
        summary: derived.summary,
    })
}
