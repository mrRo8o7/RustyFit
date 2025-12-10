use rustyfit::processing::process_fit_bytes;

#[test]
fn round_trip_returns_identical_bytes() {
    let bytes = std::fs::read("tests/fixtures/activity.fit").expect("fixture should be present");
    let processed = process_fit_bytes(&bytes).expect("processing should succeed");
    assert_eq!(processed.processed_bytes, bytes);
}
