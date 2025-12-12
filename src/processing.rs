//! Helpers for parsing, filtering, and re-encoding FIT files.
//!
//! The implementation mirrors the official FIT file layout:
//!
//! * A header whose first byte declares its own size, followed by a 4-byte
//!   data payload length and (optionally) a two-byte CRC for the header.
//! * A data section containing a stream of message definition records and data
//!   records. Data records are keyed by the "local message number" declared in
//!   the most recent definition record with the same local ID.
//! * A trailing two-byte CRC that covers the header (including its CRC when
//!   present) plus the entire data section.
//!
//! The functions in this module decode the bytes into the `fitparser` model so
//! the web UI can render human-readable fields, optionally remove speed-related
//! fields, and finally re-encode the data with an updated header and CRCs.

use fitparser::de::{DecodeOption, from_bytes_with_options};
use fitparser::profile::MesgNum;
use fitparser::{FitDataField, FitDataRecord};
use std::collections::HashSet;
use std::convert::TryInto;
use std::fmt;

/// Simplified representation of a FIT field for display in the UI.
#[derive(Debug, Clone)]
pub struct DisplayField {
    pub name: String,
    pub value: String,
}

/// Human-readable wrapper around a parsed FIT data record.
#[derive(Debug, Clone)]
pub struct DisplayRecord {
    pub message_type: String,
    pub fields: Vec<DisplayField>,
}

/// Processed FIT output returned to the web handler.
#[derive(Debug, Clone)]
pub struct ProcessedFit {
    /// Fields formatted for rendering.
    pub records: Vec<DisplayRecord>,
    /// Re-encoded FIT payload, optionally with filtered data fields.
    pub processed_bytes: Vec<u8>,
    /// Summary metrics extracted from the FIT payload.
    pub summary: WorkoutSummary,
}

/// User-facing toggles that adjust how FIT bytes are rewritten.
#[derive(Debug, Clone, Default)]
pub struct ProcessingOptions {
    /// Drop `speed` and `enhanced_speed` fields from record messages.
    pub remove_speed_fields: bool,
    /// Smooth derived speed values using a sliding window before presenting them.
    pub smooth_speed: bool,
}

/// Default window size (in samples) for moving-average speed smoothing.
const SPEED_SMOOTHING_WINDOW: usize = 5;

/// Decomposed pieces of the original FIT file used for later reconstruction.
#[derive(Debug, Clone)]
pub struct ParsedFit {
    pub header_without_crc: Vec<u8>,
    pub has_header_crc: bool,
    pub data_section: Vec<u8>,
    pub records: Vec<FitDataRecord>,
}

#[derive(Debug, Default)]
struct DerivedWorkoutData {
    summary: WorkoutSummary,
    /// Smoothed (or raw) speeds aligned to the data record index for future preprocessing.
    record_speeds: Vec<Option<f64>>,
    /// Smoothed (or raw) distances aligned to the data record index for future preprocessing.
    record_distances: Vec<Option<f64>>,
}

#[derive(Debug)]
pub enum FitProcessError {
    ParseError(String),
    InvalidHeader(String),
}

impl fmt::Display for FitProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FitProcessError::ParseError(msg) => write!(f, "Failed to decode FIT file: {msg}"),
            FitProcessError::InvalidHeader(msg) => write!(f, "Invalid FIT file: {msg}"),
        }
    }
}

impl std::error::Error for FitProcessError {}

/// Decode a FIT payload, optionally filter speed fields, and re-encode.
///
/// The function performs three stages:
/// 1. [`parse_fit`] splits the payload into header, data section, and parsed
///    `fitparser` records.
/// 2. The parsed records are converted into a format suitable for HTML output
///    and filtered based on [`ProcessingOptions`].
/// 3. The data section is optionally rewritten without speed fields and the
///    file is reconstructed.
pub fn process_fit_bytes(
    bytes: &[u8],
    options: &ProcessingOptions,
) -> Result<ProcessedFit, FitProcessError> {
    let parsed = parse_fit(bytes)?;
    let derived = derive_workout_data(&parsed.records, options);

    let filtered_records = parsed
        .records
        .clone()
        .into_iter()
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
        .collect();

    let processed_data_section = preprocess_data_section(
        &parsed.data_section,
        options,
        &derived.record_speeds,
        &derived.record_distances,
    )?;

    let processed_bytes = reencode_fit_with_section(&parsed, processed_data_section)?;

    Ok(ProcessedFit {
        records: filtered_records,
        processed_bytes,
        summary: derived.summary,
    })
}

/// Derived overview metrics from the FIT records.
#[derive(Debug, Clone, Default)]
pub struct WorkoutSummary {
    pub duration_seconds: Option<f64>,
    pub workout_type: Option<String>,
    pub distance_meters: Option<f64>,
    pub speed_min: Option<f64>,
    pub speed_mean: Option<f64>,
    pub speed_max: Option<f64>,
    pub heart_rate_min: Option<f64>,
    pub heart_rate_mean: Option<f64>,
    pub heart_rate_max: Option<f64>,
}

fn field_value_to_f64(field: &FitDataField) -> Option<f64> {
    field.value().clone().try_into().ok().or_else(|| {
        field
            .to_string()
            .split_whitespace()
            .next()
            .and_then(|raw| raw.parse::<f64>().ok())
    })
}

fn derive_workout_data(
    records: &[FitDataRecord],
    options: &ProcessingOptions,
) -> DerivedWorkoutData {
    let mut timestamps: Vec<f64> = Vec::new();
    let mut workout_type: Option<String> = None;
    let mut distance_samples: Vec<DistanceSample> = Vec::new();
    let mut heart_rates: Vec<f64> = Vec::new();

    for (idx, record) in records.iter().enumerate() {
        let mut timestamp: Option<f64> = None;
        let mut distance: Option<f64> = None;

        for field in record.fields() {
            match field.name() {
                "timestamp" => {
                    if let Some(value) = field_value_to_f64(field) {
                        timestamp = Some(value);
                        timestamps.push(value);
                    }
                }
                "distance" => {
                    if let Some(value) = field_value_to_f64(field) {
                        distance = Some(value);
                    }
                }
                "heart_rate" => {
                    if let Some(value) = field_value_to_f64(field) {
                        heart_rates.push(value);
                    }
                }
                "sport" | "workout_type" if workout_type.is_none() => {
                    let display = field.to_string();
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

    let duration_seconds = if timestamps.is_empty() {
        None
    } else {
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
    };

    let time_intervals: Vec<f64> = distance_samples
        .windows(2)
        .map(|window| match window {
            [first, second] => (second.timestamp - first.timestamp).max(0.0),
            _ => 0.0,
        })
        .collect();

    let mut speeds = compute_distance_based_speeds(&distance_samples);
    if options.smooth_speed {
        // Apply a simple centered moving average to dampen sharp spikes (staccato speeds)
        // without relying on speed fields that might already be filtered out.
        speeds = smooth_speed_window(&speeds, SPEED_SMOOTHING_WINDOW);
    }

    let smoothed_distances = if options.smooth_speed {
        Some(reconstruct_distance_series(
            &distance_samples,
            &speeds,
            &time_intervals,
        ))
    } else {
        None
    };

    let distance_series: Vec<f64> = smoothed_distances
        .as_ref()
        .map(|distances| distances.clone())
        .unwrap_or_else(|| distance_samples.iter().map(|sample| sample.distance).collect());

    let distance_meters = distance_series.last().copied();

    let positive_speeds: Vec<f64> = speeds.iter().copied().filter(|value| *value > 0.0).collect();
    let speed_min = positive_speeds.iter().cloned().reduce(f64::min);
    let speed_max = positive_speeds.iter().cloned().reduce(f64::max);

    let distance_mean = if let (Some(first), Some(last)) =
        (distance_samples.first(), distance_series.last())
    {
        let dt = distance_samples
            .last()
            .map(|sample| sample.timestamp - first.timestamp)
            .unwrap_or(0.0);
        let dd = last - first.distance;
        if dt > 0.0 && dd >= 0.0 {
            Some(dd / dt)
        } else {
            None
        }
    } else {
        None
    };

    let speed_mean = if options.smooth_speed && !speeds.is_empty() {
        Some(speeds.iter().sum::<f64>() / speeds.len() as f64)
    } else {
        distance_mean
    };

    let heart_rate_min = heart_rates.iter().cloned().reduce(f64::min);
    let heart_rate_max = heart_rates.iter().cloned().reduce(f64::max);
    let heart_rate_mean = if heart_rates.is_empty() {
        None
    } else {
        Some(heart_rates.iter().sum::<f64>() / heart_rates.len() as f64)
    };

    let mut record_speeds: Vec<Option<f64>> = vec![None; records.len()];
    let mut record_distances: Vec<Option<f64>> = vec![None; records.len()];
    if options.smooth_speed {
        for (sample_idx, sample) in distance_samples.iter().enumerate().skip(1) {
            if let Some(speed) = speeds.get(sample_idx - 1).copied() {
                record_speeds[sample.record_index] = Some(speed);
            }
        }

        for (sample_idx, sample) in distance_samples.iter().enumerate() {
            if let Some(distance) = distance_series.get(sample_idx).copied() {
                record_distances[sample.record_index] = Some(distance);
            }
        }
    }

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
        record_speeds,
        record_distances,
    }
}

/// Calculate per-sample speeds using distance deltas to avoid relying on FIT speed fields.
#[derive(Debug, Clone)]
struct DistanceSample {
    record_index: usize,
    timestamp: f64,
    distance: f64,
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

/// Reconstruct a distance series that aligns with smoothed speeds and timestamps.
fn reconstruct_distance_series(
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

/// Smooth a speed series by averaging neighboring samples within a sliding window.
fn smooth_speed_window(speeds: &[f64], window_size: usize) -> Vec<f64> {
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

/// Parse a raw FIT file into its component parts while validating CRCs.
///
/// The official FIT structure is enforced here: the header length must be at
/// least 12 bytes, the declared data length must match the payload present, and
/// the file must be long enough to include the final two-byte CRC. CRC values
/// are verified so corruption can be reported back to the caller.
pub fn parse_fit(bytes: &[u8]) -> Result<ParsedFit, FitProcessError> {
    let header_size = *bytes
        .first()
        .ok_or_else(|| FitProcessError::InvalidHeader("missing header byte".into()))?
        as usize;

    if header_size < 12 {
        return Err(FitProcessError::InvalidHeader(
            "header too small to be a FIT file".into(),
        ));
    }

    if bytes.len() < header_size + 2 {
        return Err(FitProcessError::InvalidHeader(
            "file shorter than minimum header + CRC".into(),
        ));
    }

    let has_header_crc = header_size > 12;
    let header_without_crc_end = if has_header_crc {
        header_size - 2
    } else {
        header_size
    };
    let header_without_crc = bytes[..header_without_crc_end].to_vec();

    let data_size_start = 4;
    let data_size_end = data_size_start + 4;
    if data_size_end > header_without_crc.len() {
        return Err(FitProcessError::InvalidHeader(
            "header missing data size field".into(),
        ));
    }

    let data_size = u32::from_le_bytes(
        header_without_crc[data_size_start..data_size_end]
            .try_into()
            .map_err(|_| FitProcessError::InvalidHeader("unable to read data size".into()))?,
    ) as usize;

    let data_start = header_size;
    let data_end = data_start + data_size;
    if data_end + 2 > bytes.len() {
        return Err(FitProcessError::InvalidHeader(
            "file shorter than declared data size".into(),
        ));
    }

    let data_section = bytes[data_start..data_end].to_vec();

    // Validate CRCs during parsing to surface corruption errors back to the caller.
    // CRCs are recalculated when rebuilding the file in `reencode_fit_with_section`.
    let decode_options: HashSet<DecodeOption> = HashSet::new();

    let records: Vec<FitDataRecord> = from_bytes_with_options(bytes, &decode_options)
        .map_err(|err| FitProcessError::ParseError(err.to_string()))?;

    Ok(ParsedFit {
        header_without_crc,
        has_header_crc,
        data_section,
        records,
    })
}

/// Rebuild a FIT file by combining the original header with a new data section.
///
/// The function updates the header's declared data length when possible and
/// recalculates CRCs for both header (when present) and data payload.
fn reencode_fit_with_section(
    parsed: &ParsedFit,
    data_section: Vec<u8>,
) -> Result<Vec<u8>, FitProcessError> {
    if parsed.header_without_crc.is_empty() {
        return Err(FitProcessError::InvalidHeader("missing header byte".into()));
    }

    let mut header_without_crc = parsed.header_without_crc.clone();

    // Update data size in header to reflect the new data payload if possible
    if header_without_crc.len() >= 8 {
        let data_len: u32 = data_section
            .len()
            .try_into()
            .map_err(|_| FitProcessError::InvalidHeader("data section too large".into()))?;
        header_without_crc[4..8].copy_from_slice(&data_len.to_le_bytes());
    }

    let mut rebuilt = header_without_crc.clone();
    let mut crc_input = rebuilt.clone();

    if parsed.has_header_crc {
        let header_crc = calculate_crc(&crc_input);
        rebuilt.extend_from_slice(&header_crc.to_le_bytes());
        crc_input.extend_from_slice(&header_crc.to_le_bytes());
    }

    crc_input.extend_from_slice(&data_section);
    rebuilt.extend_from_slice(&data_section);

    let data_crc = calculate_crc(&crc_input);
    rebuilt.extend_from_slice(&data_crc.to_le_bytes());

    Ok(rebuilt)
}

#[derive(Clone, Debug)]
struct FieldDefinition {
    number: u8,
    size: u8,
    base_type: u8,
}

#[derive(Clone, Debug)]
struct DeveloperFieldDefinition {
    number: u8,
    size: u8,
    developer_index: u8,
}

#[derive(Clone, Debug)]
struct MessageDefinition {
    global_mesg_num: u16,
    fields: Vec<FieldDefinition>,
    filtered_fields: Vec<FieldDefinition>,
    developer_fields: Vec<DeveloperFieldDefinition>,
    architecture: u8,
}

/// Apply preprocessing transforms (filtering, smoothing) to the FIT data section.
///
/// This keeps the traversal logic centralized so future preprocessing steps can
/// be layered on without duplicating FIT framing rules.
fn preprocess_data_section(
    data_section: &[u8],
    options: &ProcessingOptions,
    record_speeds: &[Option<f64>],
    record_distances: &[Option<f64>],
) -> Result<Vec<u8>, FitProcessError> {
    let mut offset = 0usize;
    let mut definitions: std::collections::HashMap<u8, MessageDefinition> =
        std::collections::HashMap::new();
    let mut filtered: Vec<u8> = Vec::with_capacity(data_section.len());
    let mut data_record_index: usize = 0;

    while offset < data_section.len() {
        let message_start = offset;
        let header = data_section
            .get(offset)
            .copied()
            .ok_or_else(|| FitProcessError::InvalidHeader("unexpected end of data".into()))?;
        offset += 1;

        if header & 0x80 != 0 {
            return Err(FitProcessError::ParseError(
                "compressed timestamp headers are not supported".into(),
            ));
        }

        let is_definition = header & 0x40 != 0;
        let has_developer_data = header & 0x20 != 0;
        let local_message_num = header & 0x0F;

        if is_definition {
            if offset + 5 > data_section.len() {
                return Err(FitProcessError::InvalidHeader(
                    "definition message truncated".into(),
                ));
            }

            let reserved = data_section[offset];
            let architecture = data_section[offset + 1];
            let global_mesg_num_bytes = [data_section[offset + 2], data_section[offset + 3]];
            let global_mesg_num = if architecture == 0 {
                u16::from_le_bytes(global_mesg_num_bytes)
            } else {
                u16::from_be_bytes(global_mesg_num_bytes)
            };
            let num_fields = data_section[offset + 4] as usize;
            offset += 5;

            let mut fields = Vec::with_capacity(num_fields);
            for _ in 0..num_fields {
                if offset + 3 > data_section.len() {
                    return Err(FitProcessError::InvalidHeader(
                        "field definition truncated".into(),
                    ));
                }
                fields.push(FieldDefinition {
                    number: data_section[offset],
                    size: data_section[offset + 1],
                    base_type: data_section[offset + 2],
                });
                offset += 3;
            }

            let mut developer_fields = Vec::new();
            if has_developer_data {
                let dev_count = *data_section.get(offset).ok_or_else(|| {
                    FitProcessError::InvalidHeader("missing developer count".into())
                })? as usize;
                offset += 1;

                developer_fields = Vec::with_capacity(dev_count);
                for _ in 0..dev_count {
                    if offset + 3 > data_section.len() {
                        return Err(FitProcessError::InvalidHeader(
                            "developer field truncated".into(),
                        ));
                    }
                    developer_fields.push(DeveloperFieldDefinition {
                        number: data_section[offset],
                        size: data_section[offset + 1],
                        developer_index: data_section[offset + 2],
                    });
                    offset += 3;
                }
            }

            let filtered_fields =
                if options.remove_speed_fields && global_mesg_num == MesgNum::Record.as_u16() {
                    fields
                        .iter()
                        .filter(|field| !matches!(field.number, 6 | 73))
                        .cloned()
                        .collect::<Vec<_>>()
                } else {
                    fields.clone()
                };

            definitions.insert(
                local_message_num,
                MessageDefinition {
                    global_mesg_num,
                    fields,
                    filtered_fields: filtered_fields.clone(),
                    developer_fields: developer_fields.clone(),
                    architecture,
                },
            );

            if filtered_fields.len()
                == definitions
                    .get(&local_message_num)
                    .map(|def| def.fields.len())
                    .unwrap_or(0)
            {
                // No change: reuse the original bytes for this definition message.
                filtered.extend_from_slice(&data_section[message_start..offset]);
                continue;
            }

            // Rebuild definition message without the excluded fields.
            filtered.push(header);
            filtered.push(reserved);
            filtered.push(architecture);
            if architecture == 0 {
                filtered.extend_from_slice(&global_mesg_num.to_le_bytes());
            } else {
                filtered.extend_from_slice(&global_mesg_num.to_be_bytes());
            }
            filtered.push(filtered_fields.len() as u8);

            for field in &filtered_fields {
                filtered.push(field.number);
                filtered.push(field.size);
                filtered.push(field.base_type);
            }

            if has_developer_data {
                filtered.push(developer_fields.len() as u8);
                for dev in &developer_fields {
                    filtered.push(dev.number);
                    filtered.push(dev.size);
                    filtered.push(dev.developer_index);
                }
            }
        } else {
            let definition = definitions.get(&local_message_num).ok_or_else(|| {
                FitProcessError::InvalidHeader("data message missing preceding definition".into())
            })?;

            let mut filtered_message = Vec::with_capacity(
                1 + definition.filtered_fields.len() * 3 + definition.developer_fields.len() * 3,
            );
            filtered_message.push(header);

            for field in &definition.fields {
                let field_size = field.size as usize;
                if offset + field_size > data_section.len() {
                    return Err(FitProcessError::InvalidHeader(
                        "data message truncated".into(),
                    ));
                }
                let override_speed = record_speeds.get(data_record_index).copied().flatten();
                let override_distance =
                    record_distances.get(data_record_index).copied().flatten();
                let field_bytes = &data_section[offset..offset + field_size];

                if should_remove_speed_field(&definition, field.number, options) {
                    // Skip speed fields entirely when filtering them out.
                } else if should_override_distance_field(&definition, field.number, override_distance) {
                    filtered_message.extend_from_slice(&encode_distance_value(
                        override_distance.expect("override exists due to guard"),
                        field_size,
                        definition.architecture,
                    ));
                } else if should_override_speed_field(&definition, field.number, override_speed) {
                    filtered_message.extend_from_slice(&encode_speed_value(
                        override_speed.expect("override exists due to guard"),
                        field_size,
                        definition.architecture,
                    ));
                } else {
                    filtered_message.extend_from_slice(field_bytes);
                }
                offset += field_size;
            }

            for dev_field in &definition.developer_fields {
                let field_size = dev_field.size as usize;
                if offset + field_size > data_section.len() {
                    return Err(FitProcessError::InvalidHeader(
                        "developer data message truncated".into(),
                    ));
                }
                let field_bytes = &data_section[offset..offset + field_size];
                filtered_message.extend_from_slice(field_bytes);
                offset += field_size;
            }

            filtered.extend_from_slice(&filtered_message);
            data_record_index += 1;
        }
    }

    Ok(filtered)
}

fn is_record_speed_field(definition: &MessageDefinition, field_number: u8) -> bool {
    definition.global_mesg_num == MesgNum::Record.as_u16() && matches!(field_number, 6 | 73)
}

fn is_record_distance_field(definition: &MessageDefinition, field_number: u8) -> bool {
    definition.global_mesg_num == MesgNum::Record.as_u16() && field_number == 5
}

fn should_remove_speed_field(
    definition: &MessageDefinition,
    field_number: u8,
    options: &ProcessingOptions,
) -> bool {
    options.remove_speed_fields && is_record_speed_field(definition, field_number)
}

fn should_override_speed_field(
    definition: &MessageDefinition,
    field_number: u8,
    override_speed: Option<f64>,
) -> bool {
    override_speed.is_some() && is_record_speed_field(definition, field_number)
}

fn should_override_distance_field(
    definition: &MessageDefinition,
    field_number: u8,
    override_distance: Option<f64>,
) -> bool {
    override_distance.is_some() && is_record_distance_field(definition, field_number)
}

fn encode_speed_value(speed: f64, field_size: usize, architecture: u8) -> Vec<u8> {
    let scale = 1000.0;
    let scaled = (speed * scale).round().max(0.0);
    let little_endian = architecture == 0;

    match field_size {
        2 => {
            let clamped = scaled.min(u16::MAX as f64) as u16;
            if little_endian {
                clamped.to_le_bytes().to_vec()
            } else {
                clamped.to_be_bytes().to_vec()
            }
        }
        4 => {
            let clamped = scaled.min(u32::MAX as f64) as u32;
            if little_endian {
                clamped.to_le_bytes().to_vec()
            } else {
                clamped.to_be_bytes().to_vec()
            }
        }
        _ => vec![0u8; field_size],
    }
}

fn encode_distance_value(distance: f64, field_size: usize, architecture: u8) -> Vec<u8> {
    let scale = 100.0;
    let scaled = (distance * scale).round().max(0.0);
    let little_endian = architecture == 0;

    match field_size {
        2 => {
            let clamped = scaled.min(u16::MAX as f64) as u16;
            if little_endian {
                clamped.to_le_bytes().to_vec()
            } else {
                clamped.to_be_bytes().to_vec()
            }
        }
        4 => {
            let clamped = scaled.min(u32::MAX as f64) as u32;
            if little_endian {
                clamped.to_le_bytes().to_vec()
            } else {
                clamped.to_be_bytes().to_vec()
            }
        }
        _ => vec![0u8; field_size],
    }
}

/// Compute the standard FIT CRC-16 using the Garmin nibble lookup table.
fn calculate_crc(data: &[u8]) -> u16 {
    const CRC_TABLE: [u16; 16] = [
        0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00, 0x7800,
        0xB401, 0x5000, 0x9C01, 0x8801, 0x4400,
    ];

    data.iter().fold(0u16, |crc, byte| {
        let mut tmp = CRC_TABLE[(crc & 0xF) as usize];
        let mut crc = (crc >> 4) & 0x0FFF;
        crc ^= tmp ^ CRC_TABLE[(byte & 0xF) as usize];
        tmp = CRC_TABLE[(crc & 0xF) as usize];
        crc = (crc >> 4) & 0x0FFF;
        crc ^ tmp ^ CRC_TABLE[((byte >> 4) & 0xF) as usize]
    })
}

fn should_skip_field(field_name: &str, options: &ProcessingOptions) -> bool {
    if !options.remove_speed_fields {
        return false;
    }

    matches!(field_name, "speed" | "enhanced_speed")
}
