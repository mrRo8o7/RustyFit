use rustyfit::processing::{ProcessingOptions, process_fit_bytes};

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
    let downloaded_round = process_fit_bytes(&processed.processed_bytes, &ProcessingOptions::default())
        .expect("processed file should remain decodable");

    assert!(
        !downloaded_round
            .records
            .iter()
            .flat_map(|record| &record.fields)
            .any(|field| field.name == "speed" || field.name == "enhanced_speed")
    );
}
