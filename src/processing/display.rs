use crate::processing::types::{DisplayField, DisplayRecord, ProcessingOptions};
use fitparser::FitDataRecord;

/// Build UI-friendly records while honoring filtering options.
pub fn to_display_records(
    records: &[FitDataRecord],
    _options: &ProcessingOptions,
) -> Vec<DisplayRecord> {
    records
        .iter()
        .map(|record| DisplayRecord {
            message_type: format!("{:?}", record.kind()),
            fields: record
                .fields()
                .iter()
                .map(|field| DisplayField {
                    name: field.name().to_string(),
                    value: field.to_string(),
                })
                .collect(),
        })
        .collect()
}
