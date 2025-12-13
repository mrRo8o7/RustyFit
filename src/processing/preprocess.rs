use crate::processing::summary::{
    field_value_to_f64, reconstruct_distance_series, smooth_speed_window, DistanceSample,
};
use crate::processing::types::{
    FitProcessError, ParsedFit, PreprocessedField, PreprocessedRecord, ProcessingOptions,
    SPEED_SMOOTHING_WINDOW,
};
use fitparser::profile::MesgNum;
use fitparser::FitDataRecord;
use std::convert::TryInto;

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

#[derive(Clone, Debug, Default)]
pub struct RecordOverrides {
    pub speed: Option<f64>,
    pub distance: Option<f64>,
}

/// Preprocess FIT data to align with downstream derive/display steps.
pub fn preprocess_fit(
    parsed: &ParsedFit,
    options: &ProcessingOptions,
) -> Result<(Vec<u8>, Vec<PreprocessedRecord>), FitProcessError> {
    let overrides = compute_record_overrides(&parsed.records, options);
    let processed_data_section = preprocess_data_section_with_overrides(
        &parsed.data_section,
        options,
        &overrides,
    )?;
    let records = build_preprocessed_records(&parsed.records, &overrides, options);

    Ok((processed_data_section, records))
}

/// Apply preprocessing transforms (filtering, smoothing) to the FIT data section.
///
/// This keeps the traversal logic centralized so future preprocessing steps can
/// be layered on without duplicating FIT framing rules.
pub fn preprocess_data_section(
    data_section: &[u8],
    records: &[FitDataRecord],
    options: &ProcessingOptions,
) -> Result<Vec<u8>, FitProcessError> {
    let overrides = compute_record_overrides(records, options);
    preprocess_data_section_with_overrides(data_section, options, &overrides)
}

pub fn preprocess_data_section_with_overrides(
    data_section: &[u8],
    options: &ProcessingOptions,
    overrides: &[RecordOverrides],
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
                filtered.extend_from_slice(&data_section[message_start..offset]);
                continue;
            }

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
                let override_speed = overrides
                    .get(data_record_index)
                    .and_then(|override_set| override_set.speed);
                let override_distance = overrides
                    .get(data_record_index)
                    .and_then(|override_set| override_set.distance);
                let field_bytes = &data_section[offset..offset + field_size];

                if should_remove_speed_field(&definition, field.number, options) {
                    // Skip speed fields entirely when filtering them out.
                } else if should_override_distance_field(
                    &definition,
                    field.number,
                    override_distance,
                ) {
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

    for (sample_idx, sample) in distance_samples.iter().enumerate().skip(1) {
        if let Some(speed) = smoothed_speeds.get(sample_idx - 1).copied() {
            record_speeds[sample.record_index] = Some(speed);
        }
    }

    for (sample_idx, sample) in distance_samples.iter().enumerate() {
        if let Some(distance) = smoothed_distances.get(sample_idx).copied() {
            record_distances[sample.record_index] = Some(distance);
        }
    }

    record_speeds
        .into_iter()
        .zip(record_distances.into_iter())
        .map(|(speed, distance)| RecordOverrides { speed, distance })
        .collect()
}

fn build_preprocessed_records(
    records: &[FitDataRecord],
    overrides: &[RecordOverrides],
    options: &ProcessingOptions,
) -> Vec<PreprocessedRecord> {
    records
        .iter()
        .enumerate()
        .map(|(idx, record)| {
            let mut fields: Vec<PreprocessedField> = Vec::new();
            let overrides = overrides.get(idx).cloned().unwrap_or_default();
            let is_record_message = matches!(record.kind(), MesgNum::Record);

            for field in record.fields() {
                let name = field.name().to_string();

                if options.remove_speed_fields
                    && is_record_message
                    && matches!(name.as_str(), "speed" | "enhanced_speed")
                {
                    continue;
                }

                let mut numeric_value = field_value_to_f64(field);
                let mut value = field.to_string();

                if is_record_message && name == "distance" {
                    if let Some(distance) = overrides.distance {
                        numeric_value = Some(distance);
                        value = format!("{distance}");
                    }
                } else if is_record_message
                    && matches!(name.as_str(), "speed" | "enhanced_speed")
                {
                    if let Some(speed) = overrides.speed {
                        numeric_value = Some(speed);
                        value = format!("{speed}");
                    }
                }

                fields.push(PreprocessedField {
                    name,
                    value,
                    numeric_value,
                });
            }

            PreprocessedRecord {
                message_type: format!("{:?}", record.kind()),
                fields,
            }
        })
        .collect()
}

pub fn reencode_fit_with_section(
    parsed: &ParsedFit,
    data_section: Vec<u8>,
) -> Result<Vec<u8>, FitProcessError> {
    if parsed.header_without_crc.is_empty() {
        return Err(FitProcessError::InvalidHeader("missing header byte".into()));
    }

    let mut header_without_crc = parsed.header_without_crc.clone();

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
