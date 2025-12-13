use crate::processing::types::{DisplayField, DisplayRecord};
use fitparser::FitDataRecord;

/// Convert processed records into UI-friendly display records.
pub fn to_display_records(records: &[FitDataRecord]) -> Vec<DisplayRecord> {
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
