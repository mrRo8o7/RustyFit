use crate::processing::types::{DisplayField, DisplayRecord, ProcessingOptions};
use fitparser::FitDataRecord;

/// Build UI-friendly records while honoring filtering options.
pub fn to_display_records(
    records: &[FitDataRecord],
    options: &ProcessingOptions,
) -> Vec<DisplayRecord> {
    records
        .iter()
        .map(|record| DisplayRecord {
            message_type: format!("{:?}", record.kind()),
            fields: record
                .fields()
                .iter()
                .filter(|field| !should_skip_field(field.name(), options))
                .map(|field| DisplayField {
                    name: field.name().to_string(),
                    value: field.to_string(),
                })
                .collect(),
        })
        .collect()
}

fn should_skip_field(field_name: &str, options: &ProcessingOptions) -> bool {
    if !options.remove_speed_fields {
        return false;
    }
    matches!(field_name, "speed" | "enhanced_speed")
}
