use crate::processing::types::{DerivedWorkoutData, PreprocessedRecord, WorkoutSummary};
use fitparser::FitDataField;
use std::convert::TryInto;

#[derive(Debug, Clone)]
pub(crate) struct DistanceSample {
    pub(crate) record_index: usize,
    pub(crate) timestamp: f64,
    pub(crate) distance: f64,
}

/// Convert FIT fields into derived metrics and optional smoothed series.
pub fn derive_workout_data(records: &[PreprocessedRecord]) -> DerivedWorkoutData {
    let mut timestamps: Vec<f64> = Vec::new();
    let mut workout_type: Option<String> = None;
    let mut distance_samples: Vec<DistanceSample> = Vec::new();
    let mut heart_rates: Vec<f64> = Vec::new();

    for (idx, record) in records.iter().enumerate() {
        let mut timestamp: Option<f64> = None;
        let mut distance: Option<f64> = None;

        for field in &record.fields {
            match field.name.as_str() {
                "timestamp" => {
                    if let Some(value) = field.numeric_value {
                        timestamp = Some(value);
                        timestamps.push(value);
                    }
                }
                "distance" => {
                    if let Some(value) = field.numeric_value {
                        distance = Some(value);
                    }
                }
                "heart_rate" => {
                    if let Some(value) = field.numeric_value {
                        heart_rates.push(value);
                    }
                }
                "sport" | "workout_type" if workout_type.is_none() => {
                    let display = field.value.clone();
                    if !display.is_empty() {
                        workout_type = Some(display);
                    }
                }
                _ => {}
            }
        }

        if let (Some(ts), Some(dist)) = (timestamp, distance) {
            distance_samples.push(DistanceSample {
                record_index: idx,
                timestamp: ts,
                distance: dist,
            });
        }
    }

    let duration_seconds = derive_duration(&timestamps);
    let time_intervals: Vec<f64> = distance_samples
        .windows(2)
        .map(|window| match window {
            [first, second] => (second.timestamp - first.timestamp).max(0.0),
            _ => 0.0,
        })
        .collect();

    let speeds = compute_distance_based_speeds(&distance_samples);
    let smoothed_distances = Some(reconstruct_distance_series(
        &distance_samples,
        &speeds,
        &time_intervals,
    ));

    let distance_series: Vec<f64> = smoothed_distances
        .as_ref()
        .map(|distances| distances.clone())
        .unwrap_or_else(|| {
            distance_samples
                .iter()
                .map(|sample| sample.distance)
                .collect()
        });

    let distance_meters = distance_series.last().copied();
    let positive_speeds: Vec<f64> = speeds
        .iter()
        .copied()
        .filter(|value| *value > 0.0)
        .collect();
    let speed_min = positive_speeds.iter().cloned().reduce(f64::min);
    let speed_max = positive_speeds.iter().cloned().reduce(f64::max);
    let speed_mean = derive_speed_mean(&distance_samples, &distance_series, &speeds);

    let heart_rate_min = heart_rates.iter().cloned().reduce(f64::min);
    let heart_rate_max = heart_rates.iter().cloned().reduce(f64::max);
    let heart_rate_mean = if heart_rates.is_empty() {
        None
    } else {
        Some(heart_rates.iter().sum::<f64>() / heart_rates.len() as f64)
    };

    DerivedWorkoutData {
        summary: WorkoutSummary {
            duration_seconds,
            workout_type,
            distance_meters,
            speed_min,
            speed_mean,
            speed_max,
            heart_rate_min,
            heart_rate_mean,
            heart_rate_max,
        },
    }
}

fn derive_duration(timestamps: &[f64]) -> Option<f64> {
    if timestamps.is_empty() {
        return None;
    }
    let (min_ts, max_ts) = timestamps
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |acc, &ts| {
            (acc.0.min(ts), acc.1.max(ts))
        });
    if min_ts.is_infinite() || max_ts.is_infinite() {
        None
    } else {
        Some(max_ts - min_ts)
    }
}

fn derive_speed_mean(
    distance_samples: &[DistanceSample],
    distance_series: &[f64],
    speeds: &[f64],
) -> Option<f64> {
    if !speeds.is_empty() {
        return Some(speeds.iter().sum::<f64>() / speeds.len() as f64);
    }

    if let (Some(first), Some(last)) = (distance_samples.first(), distance_series.last()) {
        let dt = distance_samples
            .last()
            .map(|sample| sample.timestamp - first.timestamp)
            .unwrap_or(0.0);
        let dd = last - first.distance;
        if dt > 0.0 && dd >= 0.0 {
            return Some(dd / dt);
        }
    }

    None
}

pub(crate) fn field_value_to_f64(field: &FitDataField) -> Option<f64> {
    field.value().clone().try_into().ok().or_else(|| {
        field
            .to_string()
            .split_whitespace()
            .next()
            .and_then(|raw| raw.parse::<f64>().ok())
    })
}

fn compute_distance_based_speeds(distance_samples: &[DistanceSample]) -> Vec<f64> {
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
    speeds
}

pub(crate) fn reconstruct_distance_series(
    distance_samples: &[DistanceSample],
    smoothed_speeds: &[f64],
    intervals: &[f64],
) -> Vec<f64> {
    if distance_samples.is_empty() {
        return Vec::new();
    }

    let mut distances = Vec::with_capacity(distance_samples.len());
    distances.push(distance_samples[0].distance);

    let steps = smoothed_speeds.len().min(intervals.len());
    for idx in 0..steps {
        let previous = *distances.last().unwrap_or(&distance_samples[0].distance);
        let increment = smoothed_speeds[idx] * intervals[idx];
        distances.push((previous + increment).max(previous));
    }

    while distances.len() < distance_samples.len() {
        let last = *distances.last().unwrap_or(&distance_samples[0].distance);
        distances.push(last);
    }

    distances
}

pub(crate) fn smooth_speed_window(speeds: &[f64], window_size: usize) -> Vec<f64> {
    if window_size == 0 || speeds.is_empty() {
        return speeds.to_vec();
    }

    let radius = window_size / 2;
    let len = speeds.len();

    (0..len)
        .map(|idx| {
            let start = idx.saturating_sub(radius);
            let end = (idx + radius + 1).min(len);
            let window = &speeds[start..end];
            window.iter().sum::<f64>() / window.len() as f64
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn smoothing_defaults_to_empty_series() {
        let result = smooth_speed_window(&[], 5);
        assert!(result.is_empty());
    }

    #[test]
    fn reconstruct_distance_preserves_monotonicity() {
        let samples = vec![
            DistanceSample {
                record_index: 0,
                timestamp: 0.0,
                distance: 0.0,
            },
            DistanceSample {
                record_index: 1,
                timestamp: 1.0,
                distance: 1.0,
            },
        ];
        let series = reconstruct_distance_series(&samples, &[1.0], &[1.0]);
        assert_eq!(series, vec![0.0, 1.0]);
    }
}
