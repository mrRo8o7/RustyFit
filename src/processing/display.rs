use crate::processing::types::{DisplayField, DisplayRecord, PreprocessedRecord};

/// Convert preprocessed records into UI-friendly display records.
pub fn to_display_records(records: &[PreprocessedRecord]) -> Vec<DisplayRecord> {
    records
        .iter()
        .map(|record| DisplayRecord {
            message_type: record.message_type.clone(),
            fields: record
                .fields
                .iter()
                .map(|field| DisplayField {
                    name: field.name.clone(),
                    value: field.value.clone(),
                })
                .collect(),
        })
        .collect()
}
