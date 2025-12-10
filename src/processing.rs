use fitparser::FitDataRecord;
use fitparser::de::{DecodeOption, from_bytes_with_options};
use std::collections::HashSet;
use std::fmt;

#[derive(Debug, Clone)]
pub struct DisplayField {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct DisplayRecord {
    pub message_type: String,
    pub fields: Vec<DisplayField>,
}

#[derive(Debug, Clone)]
pub struct ProcessedFit {
    pub records: Vec<DisplayRecord>,
    pub processed_bytes: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct ProcessingOptions {
    pub remove_speed_fields: bool,
}

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

    let processed_bytes = if options.remove_speed_fields {
        reencode_fit_with_section(&parsed, Vec::new())?
    } else {
        reencode_fit_with_section(&parsed, parsed.data_section.clone())?
    };

    Ok(ProcessedFit {
        records: filtered_records,
        processed_bytes,
    })
}

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

    let decode_options: HashSet<DecodeOption> = [
        DecodeOption::SkipHeaderCrcValidation,
        DecodeOption::SkipDataCrcValidation,
    ]
    .into_iter()
    .collect();

    let records: Vec<FitDataRecord> = from_bytes_with_options(bytes, &decode_options)
        .map_err(|err| FitProcessError::ParseError(err.to_string()))?;

    Ok(ParsedFit {
        header_without_crc,
        has_header_crc,
        data_section,
        records,
    })
}

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
