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

use fitparser::profile::MesgNum;
use fitparser::FitDataRecord;
use fitparser::de::{DecodeOption, from_bytes_with_options};
use std::collections::HashSet;
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
}

/// User-facing toggles that adjust how FIT bytes are rewritten.
#[derive(Debug, Clone, Default)]
pub struct ProcessingOptions {
    /// Drop `speed` and `enhanced_speed` fields from record messages.
    pub remove_speed_fields: bool,
}

/// Decomposed pieces of the original FIT file used for later reconstruction.
#[derive(Debug, Clone)]
pub struct ParsedFit {
    pub header_without_crc: Vec<u8>,
    pub has_header_crc: bool,
    pub data_section: Vec<u8>,
    pub records: Vec<FitDataRecord>,
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

    let processed_data_section = if options.remove_speed_fields {
        filter_data_section(&parsed.data_section, options)?
    } else {
        parsed.data_section.clone()
    };

    let processed_bytes = reencode_fit_with_section(&parsed, processed_data_section)?;

    Ok(ProcessedFit {
        records: filtered_records,
        processed_bytes,
    })
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
}

/// Remove selected fields from the data section while keeping FIT framing valid.
///
/// The FIT data stream alternates between definition messages (which describe
/// the layout of subsequent data messages for a local message number) and data
/// messages (which contain values following the last definition). When a field
/// is removed we must rebuild both the definition and the matching data
/// messages so that offsets remain correct and decoders can still read the
/// payload.
fn filter_data_section(
    data_section: &[u8],
    options: &ProcessingOptions,
) -> Result<Vec<u8>, FitProcessError> {
    let mut offset = 0usize;
    let mut definitions: std::collections::HashMap<u8, MessageDefinition> =
        std::collections::HashMap::new();
    let mut filtered: Vec<u8> = Vec::with_capacity(data_section.len());

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
                let dev_count = *data_section
                    .get(offset)
                    .ok_or_else(|| FitProcessError::InvalidHeader("missing developer count".into()))?
                    as usize;
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

            let filtered_fields = if options.remove_speed_fields
                && global_mesg_num == MesgNum::Record.as_u16()
            {
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

            let mut filtered_message =
                Vec::with_capacity(1 + definition.filtered_fields.len() * 3 + definition.developer_fields.len() * 3);
            filtered_message.push(header);

            for field in &definition.fields {
                let field_size = field.size as usize;
                if offset + field_size > data_section.len() {
                    return Err(FitProcessError::InvalidHeader(
                        "data message truncated".into(),
                    ));
                }
                let field_bytes = &data_section[offset..offset + field_size];
                if !(options.remove_speed_fields
                    && definition.global_mesg_num == MesgNum::Record.as_u16()
                    && matches!(field.number, 6 | 73))
                {
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
        }
    }

    Ok(filtered)
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
