use crate::processing::summary::{
    DistanceSample, field_value_to_f64, reconstruct_distance_series, smooth_speed_window,
};
use crate::processing::types::{FitProcessError, ProcessingOptions, SPEED_SMOOTHING_WINDOW};
use fitparser::profile::MesgNum;
use fitparser::{FitDataField, FitDataRecord, Value};

#[derive(Clone, Debug, Default)]
pub struct RecordOverrides {
    pub speed: Option<f64>,
    pub distance: Option<f64>,
}

/// Preprocess FIT data to align with downstream derive/display steps.
pub fn preprocess_fit(
    records: &[FitDataRecord],
    options: &ProcessingOptions,
) -> Result<Vec<FitDataRecord>, FitProcessError> {
    let overrides = compute_record_overrides(records, options);
    Ok(apply_overrides_and_filters(records, &overrides, options))
}

fn apply_overrides_and_filters(
    records: &[FitDataRecord],
    overrides: &[RecordOverrides],
    options: &ProcessingOptions,
) -> Vec<FitDataRecord> {
    records
        .iter()
        .enumerate()
        .map(|(idx, record)| {
            let mut updated = FitDataRecord::new(record.kind());
            let record_overrides = overrides.get(idx).cloned().unwrap_or_default();
            let is_record_message = matches!(record.kind(), MesgNum::Record);

            for field in record.fields() {
                let name = field.name();
                if options.remove_speed_fields
                    && is_record_message
                    && matches!(name, "speed" | "enhanced_speed")
                {
                    continue;
                }

                let mut overridden = false;
                let value = match name {
                    "distance" if is_record_message => {
                        overridden = true;
                        record_overrides
                            .distance
                            .map(Value::Float64)
                            .unwrap_or_else(|| field.value().clone())
                    }
                    "speed" | "enhanced_speed" if is_record_message => {
                        overridden = true;
                        record_overrides
                            .speed
                            .map(Value::Float64)
                            .unwrap_or_else(|| field.value().clone())
                    }
                    _ => field.value().clone(),
                };

                if overridden {
                    let updated_field = FitDataField::with_meta(
                        field.name().to_string(),
                        field.number(),
                        field.developer_data_index(),
                        value,
                        field.raw_value().clone(),
                        field.units().to_string(),
                        field.base_type(),
                        field.scale(),
                        field.offset(),
                        field.timestamp_kind(),
                    );
                    updated.push(updated_field);
                } else {
                    updated.push(field.clone());
                }
            }

            updated
        })
        .collect()
}

pub fn compute_record_overrides(
    records: &[FitDataRecord],
    options: &ProcessingOptions,
) -> Vec<RecordOverrides> {
    if !options.smooth_speed {
        return vec![RecordOverrides::default(); records.len()];
    }

    let mut distance_samples: Vec<DistanceSample> = Vec::new();

    for (record_index, record) in records.iter().enumerate() {
        let mut timestamp: Option<f64> = None;
        let mut distance: Option<f64> = None;

        for field in record.fields() {
            match field.name() {
                "timestamp" => timestamp = field_value_to_f64(field),
                "distance" => distance = field_value_to_f64(field),
                _ => {}
            }
        }

        if let (Some(ts), Some(dist)) = (timestamp, distance) {
            distance_samples.push(DistanceSample {
                record_index,
                timestamp: ts,
                distance: dist,
            });
        }
    }

    if distance_samples.len() < 2 {
        return vec![RecordOverrides::default(); records.len()];
    }

    let time_intervals: Vec<f64> = distance_samples
        .windows(2)
        .map(|window| match window {
            [first, second] => (second.timestamp - first.timestamp).max(0.0),
            _ => 0.0,
        })
        .collect();

    let mut speeds: Vec<f64> = Vec::new();
    for window in distance_samples.windows(2) {
        if let [first, second] = window {
            let dt = second.timestamp - first.timestamp;
            let dd = second.distance - first.distance;
            if dt > 0.0 {
                speeds.push(dd.max(0.0) / dt);
            } else {
                speeds.push(0.0);
            }
        }
    }

    let smoothed_speeds = smooth_speed_window(&speeds, SPEED_SMOOTHING_WINDOW);
    let smoothed_distances =
        reconstruct_distance_series(&distance_samples, &smoothed_speeds, &time_intervals);

    let mut record_speeds: Vec<Option<f64>> = vec![None; records.len()];
    let mut record_distances: Vec<Option<f64>> = vec![None; records.len()];

    for (window_idx, (&speed, sample)) in smoothed_speeds
        .iter()
        .zip(distance_samples.iter())
        .enumerate()
    {
        record_speeds[sample.record_index] = Some(speed);
        if let Some(distance) = smoothed_distances.get(window_idx).copied() {
            record_distances[sample.record_index] = Some(distance);
        }
    }

    if let Some(sample) = distance_samples.last() {
        if let Some(distance) = smoothed_distances.get(distance_samples.len() - 1).copied() {
            record_distances[sample.record_index] = Some(distance);
        }
    }

    records
        .iter()
        .enumerate()
        .map(|(idx, _)| RecordOverrides {
            speed: record_speeds.get(idx).cloned().unwrap_or(None),
            distance: record_distances.get(idx).cloned().unwrap_or(None),
        })
        .collect()
}
