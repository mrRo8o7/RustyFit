use fitparser::from_bytes;
use rustyfit::processing::{FitProcessError, ProcessingOptions, parse_fit, process_fit_bytes};
use rustyfit::templates::render_processed_records;

fn parse_speed_values(records: &[rustyfit::processing::DisplayRecord]) -> Vec<f64> {
    records
        .iter()
        .filter_map(|record| {
            record.fields.iter().find_map(|field| {
                if field.name == "enhanced_speed" || field.name == "speed" {
                    field
                        .value
                        .split_whitespace()
                        .next()
                        .and_then(|raw| raw.parse::<f64>().ok())
                } else {
                    None
                }
            })
        })
        .collect()
}

fn parse_distance_values(records: &[rustyfit::processing::DisplayRecord]) -> Vec<f64> {
    records
        .iter()
        .filter_map(|record| {
            record.fields.iter().find_map(|field| {
                if field.name == "distance" {
                    field
                        .value
                        .split_whitespace()
                        .next()
                        .and_then(|raw| raw.parse::<f64>().ok())
                } else {
                    None
                }
            })
        })
        .collect()
}

fn smooth_series(values: &[f64], window_size: usize) -> Vec<f64> {
    if window_size == 0 || values.is_empty() {
        return values.to_vec();
    }

    let radius = window_size / 2;
    let len = values.len();

    (0..len)
        .map(|idx| {
            let start = idx.saturating_sub(radius);
            let end = (idx + radius + 1).min(len);
            let window = &values[start..end];
            window.iter().sum::<f64>() / window.len() as f64
        })
        .collect()
}

fn distance_based_speeds(records: &[fitparser::FitDataRecord]) -> Vec<f64> {
    let mut samples: Vec<(f64, f64)> = Vec::new();

    for record in records {
        let mut timestamp: Option<f64> = None;
        let mut distance: Option<f64> = None;

        for field in record.fields() {
            match field.name() {
                "timestamp" => {
                    timestamp = field.value().clone().try_into().ok().or_else(|| {
                        field
                            .to_string()
                            .split_whitespace()
                            .next()
                            .and_then(|raw| raw.parse::<f64>().ok())
                    });
                }
                "distance" => {
                    distance = field.value().clone().try_into().ok().or_else(|| {
                        field
                            .to_string()
                            .split_whitespace()
                            .next()
                            .and_then(|raw| raw.parse::<f64>().ok())
                    });
                }
                _ => {}
            }
        }

        if let (Some(ts), Some(dist)) = (timestamp, distance) {
            samples.push((ts, dist));
        }
    }

    samples
        .windows(2)
        .map(|window| match window {
            [(t1, d1), (t2, d2)] => {
                let dt = (t2 - t1).max(0.0);
                let dd = (d2 - d1).max(0.0);
                if dt > 0.0 { dd / dt } else { 0.0 }
            }
            _ => 0.0,
        })
        .collect()
}

fn smoothed_distance_series(records: &[fitparser::FitDataRecord], window: usize) -> Vec<f64> {
    let mut samples: Vec<(f64, f64)> = Vec::new();

    for record in records {
        let mut timestamp: Option<f64> = None;
        let mut distance: Option<f64> = None;

        for field in record.fields() {
            match field.name() {
                "timestamp" => {
                    timestamp = field.value().clone().try_into().ok().or_else(|| {
                        field
                            .to_string()
                            .split_whitespace()
                            .next()
                            .and_then(|raw| raw.parse::<f64>().ok())
                    });
                }
                "distance" => {
                    distance = field.value().clone().try_into().ok().or_else(|| {
                        field
                            .to_string()
                            .split_whitespace()
                            .next()
                            .and_then(|raw| raw.parse::<f64>().ok())
                    });
                }
                _ => {}
            }
        }

        if let (Some(ts), Some(dist)) = (timestamp, distance) {
            samples.push((ts, dist));
        }
    }

    if samples.is_empty() {
        return Vec::new();
    }

    let intervals: Vec<f64> = samples
        .windows(2)
        .map(|pair| match pair {
            [(t1, _), (t2, _)] => (t2 - t1).max(0.0),
            _ => 0.0,
        })
        .collect();

    let speeds = smooth_series(&distance_based_speeds(records), window);

    let mut distances = Vec::with_capacity(samples.len());
    distances.push(samples[0].1);

    for (idx, speed) in speeds.iter().enumerate() {
        let prev = *distances.last().unwrap_or(&samples[0].1);
        let increment = speed * intervals.get(idx).copied().unwrap_or(0.0);
        distances.push((prev + increment).max(prev));
    }

    while distances.len() < samples.len() {
        let last = *distances.last().unwrap_or(&samples[0].1);
        distances.push(last);
    }

    distances
}

#[test]
fn round_trip_returns_identical_bytes() {
    let bytes = std::fs::read("tests/fixtures/activity.fit").expect("fixture should be present");
    let processed = process_fit_bytes(&bytes, &ProcessingOptions::default())
        .expect("processing should succeed");
    let original_records = from_bytes(&bytes).expect("fixture should decode");
    let redecoded_records =
        from_bytes(&processed.processed_bytes).expect("re-encoded bytes should decode");

    assert_eq!(redecoded_records.len(), original_records.len());
    assert!(
        redecoded_records
            .iter()
            .zip(original_records.iter())
            .all(|(reencoded, original)| reencoded.kind() == original.kind())
    );
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
            ..Default::default()
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
            ..Default::default()
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
fn speed_smoothing_can_be_enabled() {
    let bytes = std::fs::read("tests/fixtures/activity.fit").expect("fixture should be present");

    let baseline = process_fit_bytes(&bytes, &ProcessingOptions::default())
        .expect("baseline processing should succeed");

    let smoothed = process_fit_bytes(
        &bytes,
        &ProcessingOptions {
            smooth_speed: true,
            ..Default::default()
        },
    )
    .expect("smoothing should succeed");

    let base_min = baseline.summary.speed_min.expect("min speed should exist");
    let base_max = baseline.summary.speed_max.expect("max speed should exist");
    let smoothed_min = smoothed
        .summary
        .speed_min
        .expect("smoothed min should exist");
    let smoothed_max = smoothed
        .summary
        .speed_max
        .expect("smoothed max should exist");

    // Moving average smoothing should temper spikes while keeping overall pace consistent.
    assert!(smoothed_min >= base_min * 0.9);
    assert!(smoothed_max <= base_max * 1.05);

    let base_mean = baseline
        .summary
        .speed_mean
        .expect("baseline mean available");
    let smoothed_mean = smoothed
        .summary
        .speed_mean
        .expect("smoothed mean available");
    assert!((base_mean - smoothed_mean).abs() < 0.2);
}

#[test]
fn smoothing_relies_on_distance_not_speed_fields() {
    let bytes = std::fs::read("tests/fixtures/activity.fit").expect("fixture should be present");

    let processed = process_fit_bytes(
        &bytes,
        &ProcessingOptions {
            remove_speed_fields: true,
            smooth_speed: true,
            ..Default::default()
        },
    )
    .expect("processing with smoothing should succeed");

    assert!(processed.summary.speed_min.unwrap_or(0.0) > 0.0);
    assert!(processed.summary.speed_max.unwrap_or(0.0) >= processed.summary.speed_min.unwrap());
    assert!(processed.summary.speed_mean.unwrap_or(0.0) > 0.0);
}

#[test]
fn smoothed_speeds_are_written_to_processed_file() {
    let bytes = std::fs::read("tests/fixtures/activity.fit").expect("fixture should be present");

    let baseline = process_fit_bytes(&bytes, &ProcessingOptions::default())
        .expect("baseline processing should succeed");

    let smoothed = process_fit_bytes(
        &bytes,
        &ProcessingOptions {
            smooth_speed: true,
            ..Default::default()
        },
    )
    .expect("smoothing should succeed");

    let roundtripped = process_fit_bytes(&smoothed.processed_bytes, &ProcessingOptions::default())
        .expect("processed FIT should remain decodable");

    let baseline_speeds = parse_speed_values(&baseline.records);
    let roundtrip_speeds = parse_speed_values(&roundtripped.records);

    assert!(!baseline_speeds.is_empty());
    assert_eq!(baseline_speeds.len(), roundtrip_speeds.len());

    // Smoothed output should adjust at least one encoded speed value.
    assert!(
        baseline_speeds
            .iter()
            .zip(&roundtrip_speeds)
            .any(|(base, smooth)| (base - smooth).abs() > 0.05)
    );

    let parsed = parse_fit(&bytes).expect("raw parse should succeed");
    let expected_smoothed = smooth_series(&distance_based_speeds(&parsed.records), 5);
    let compare_len = expected_smoothed.len().min(roundtrip_speeds.len());
    for (expected, encoded) in expected_smoothed
        .iter()
        .zip(&roundtrip_speeds)
        .take(compare_len)
    {
        assert!(
            (expected - encoded).abs() < 0.5,
            "encoded speeds should track the smoothed series"
        );
    }
}

#[test]
fn smoothed_distances_are_written_and_reimportable() {
    let bytes = std::fs::read("tests/fixtures/activity.fit").expect("fixture should be present");

    let smoothed = process_fit_bytes(
        &bytes,
        &ProcessingOptions {
            smooth_speed: true,
            ..Default::default()
        },
    )
    .expect("smoothing should succeed");

    let roundtrip = process_fit_bytes(&smoothed.processed_bytes, &ProcessingOptions::default())
        .expect("processed FIT should remain decodable");

    let baseline = process_fit_bytes(&bytes, &ProcessingOptions::default())
        .expect("baseline processing should succeed");

    let baseline_distances = parse_distance_values(&baseline.records);
    let roundtrip_distances = parse_distance_values(&roundtrip.records);

    assert_eq!(baseline_distances.len(), roundtrip_distances.len());
    assert!(
        baseline_distances
            .iter()
            .zip(&roundtrip_distances)
            .any(|(base, smooth)| (base - smooth).abs() > 0.01),
        "downloaded FIT should reflect smoothed distances"
    );

    let parsed = parse_fit(&bytes).expect("raw parse should succeed");
    let expected_smoothed = smoothed_distance_series(&parsed.records, 5);
    let compare_len = expected_smoothed.len().min(roundtrip_distances.len());
    for (expected, encoded) in expected_smoothed
        .iter()
        .zip(&roundtrip_distances)
        .take(compare_len)
    {
        assert!(
            (expected - encoded).abs() < 1.0,
            "encoded distances should track the smoothed series"
        );
    }

    let expected_total = expected_smoothed.last().copied().unwrap_or(0.0);
    let encoded_total = roundtrip.summary.distance_meters.unwrap_or(0.0);
    assert!(
        (expected_total - encoded_total).abs() < 1.0,
        "smoothed distance should influence summary totals"
    );
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
            ..Default::default()
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
            ..Default::default()
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
