use fitparser::from_bytes;
use rustyfit::processing::{FitProcessError, ProcessingOptions, process_fit_bytes};
use rustyfit::templates::render_processed_records;

#[test]
fn round_trip_returns_identical_bytes() {
    let bytes = std::fs::read("tests/fixtures/activity.fit").expect("fixture should be present");
    let processed = process_fit_bytes(&bytes, &ProcessingOptions::default())
        .expect("processing should succeed");
    assert_eq!(processed.processed_bytes, bytes);
}

#[test]
fn speed_fields_can_be_removed_from_display_records() {
    let bytes = std::fs::read("tests/fixtures/activity.fit").expect("fixture should be present");

    let default_processed = process_fit_bytes(&bytes, &ProcessingOptions::default())
        .expect("processing should succeed");
    assert!(
        default_processed
            .records
            .iter()
            .flat_map(|record| &record.fields)
            .any(|field| field.name == "speed" || field.name == "enhanced_speed")
    );

    let processed = process_fit_bytes(
        &bytes,
        &ProcessingOptions {
            remove_speed_fields: true,
        },
    )
    .expect("processing should succeed");

    assert!(
        !processed
            .records
            .iter()
            .flat_map(|record| &record.fields)
            .any(|field| field.name == "speed" || field.name == "enhanced_speed")
    );

    // The processed bytes should also encode the filtered output so a subsequent parse
    // does not surface speed fields again.
    let downloaded_round =
        process_fit_bytes(&processed.processed_bytes, &ProcessingOptions::default())
            .expect("processed file should remain decodable");

    assert_eq!(downloaded_round.records.len(), processed.records.len());
    assert!(
        downloaded_round
            .records
            .iter()
            .any(|record| !record.fields.is_empty())
    );

    assert!(
        !downloaded_round
            .records
            .iter()
            .flat_map(|record| &record.fields)
            .any(|field| field.name == "speed" || field.name == "enhanced_speed")
    );
}

#[test]
fn summary_uses_distance_and_time_after_speed_fields_removed() {
    let bytes = std::fs::read("tests/fixtures/activity.fit").expect("fixture should be present");

    let baseline = process_fit_bytes(&bytes, &ProcessingOptions::default())
        .expect("baseline processing should succeed");

    let filtered = process_fit_bytes(
        &bytes,
        &ProcessingOptions {
            remove_speed_fields: true,
        },
    )
    .expect("filtering should succeed");

    // Re-process the filtered bytes; the speed fields are gone but distance/timestamp remain.
    let reprocessed = process_fit_bytes(&filtered.processed_bytes, &ProcessingOptions::default())
        .expect("reprocessing filtered file should succeed");

    let base_mean = baseline
        .summary
        .speed_mean
        .expect("baseline mean speed should exist");
    let repro_mean = reprocessed
        .summary
        .speed_mean
        .expect("mean speed should be computable from distance/timestamps");

    assert!(baseline.summary.duration_seconds.unwrap() > 0.0);
    assert!(reprocessed.summary.duration_seconds.unwrap() > 0.0);
    assert!(reprocessed.summary.speed_min.unwrap() > 0.0);
    assert!(reprocessed.summary.speed_max.unwrap() >= reprocessed.summary.speed_min.unwrap());

    // Speeds should stay consistent even when explicit speed fields are stripped out.
    assert!((base_mean - repro_mean).abs() < 0.05);
}

#[test]
fn filtered_download_preserves_non_speed_fields() {
    let bytes = std::fs::read("tests/fixtures/activity.fit").expect("fixture should be present");

    let original = process_fit_bytes(&bytes, &ProcessingOptions::default())
        .expect("baseline processing should succeed");

    let original_record_count = original
        .records
        .iter()
        .filter(|record| record.message_type.contains("Record"))
        .count();

    let processed = process_fit_bytes(
        &bytes,
        &ProcessingOptions {
            remove_speed_fields: true,
        },
    )
    .expect("filtered processing should succeed");

    let reparsed = process_fit_bytes(&processed.processed_bytes, &ProcessingOptions::default())
        .expect("re-processed file should decode");

    let field_names: std::collections::HashSet<_> = reparsed
        .records
        .iter()
        .flat_map(|record| record.fields.iter().map(|field| field.name.clone()))
        .collect();

    let record_count = reparsed
        .records
        .iter()
        .filter(|record| record.message_type.contains("Record"))
        .count();

    assert_eq!(reparsed.records.len(), original.records.len());
    assert_eq!(record_count, original_record_count);
    assert!(field_names.contains("distance"));
}

#[test]
fn filtered_download_keeps_crc_valid() {
    let bytes = std::fs::read("tests/fixtures/activity.fit").expect("fixture should be present");

    let processed = process_fit_bytes(
        &bytes,
        &ProcessingOptions {
            remove_speed_fields: true,
        },
    )
    .expect("processing should succeed");

    // Decoding without skipping CRC validation should succeed if we updated the header and data CRC.
    from_bytes(&processed.processed_bytes).expect("processed FIT bytes should have valid CRC");
}

#[test]
fn rendered_summary_uses_pace_units() {
    let bytes = std::fs::read("tests/fixtures/activity.fit").expect("fixture should be present");

    let processed = process_fit_bytes(&bytes, &ProcessingOptions::default())
        .expect("processing should succeed");

    let rendered = render_processed_records(&processed);

    assert!(
        rendered.contains("min/km"),
        "Rendered summary should display speed as pace"
    );
}

#[test]
fn heart_rate_summary_is_rendered() {
    let bytes = std::fs::read("tests/fixtures/activity.fit").expect("fixture should be present");

    let processed = process_fit_bytes(&bytes, &ProcessingOptions::default())
        .expect("processing should succeed");

    let rendered = render_processed_records(&processed);

    assert!(rendered.contains("Heart Rate (mean)"));
    assert!(rendered.contains("Heart Rate (min)"));
    assert!(rendered.contains("Heart Rate (max)"));
}

#[test]
fn rendering_handles_missing_workout_fields() {
    let processed = rustyfit::processing::ProcessedFit {
        records: Vec::new(),
        processed_bytes: Vec::new(),
        summary: rustyfit::processing::WorkoutSummary::default(),
    };

    let rendered = render_processed_records(&processed);

    assert!(rendered.contains("Workout Overview"));
    assert!(rendered.contains("Unknown"));
    assert!(rendered.contains("—"));
}

#[test]
fn heart_rate_formatting_uses_bpm_units() {
    let processed = rustyfit::processing::ProcessedFit {
        records: Vec::new(),
        processed_bytes: Vec::new(),
        summary: rustyfit::processing::WorkoutSummary {
            heart_rate_min: Some(120.4),
            heart_rate_mean: Some(130.6),
            heart_rate_max: Some(148.9),
            ..Default::default()
        },
    };

    let rendered = render_processed_records(&processed);

    assert!(rendered.contains("120 bpm"));
    assert!(rendered.contains("131 bpm"));
    assert!(rendered.contains("149 bpm"));
}

#[test]
fn invalid_crc_surfaces_an_error() {
    let mut bytes =
        std::fs::read("tests/fixtures/activity.fit").expect("fixture should be present");

    // Flip the last byte to invalidate the trailing data CRC.
    if let Some(last) = bytes.last_mut() {
        *last ^= 0xFF;
    }

    let error = process_fit_bytes(&bytes, &ProcessingOptions::default())
        .expect_err("processing should fail when CRC is invalid");

    match error {
        FitProcessError::ParseError(message) => {
            assert!(
                message.to_lowercase().contains("crc"),
                "error message should mention CRC validation"
            );
        }
        other => panic!("unexpected error variant: {:?}", other),
    }
}
