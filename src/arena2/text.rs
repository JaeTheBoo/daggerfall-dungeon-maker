//! Daggerfall's text records, stored in `TEXT.RSC`.
//!
//! The file starts with a table of records: a 16-bit length of the table,
//! then for each record its 16-bit id and 32-bit offset. A record is text
//! (bytes 0x20 to 0x7F) mixed with formatting bytes, and ends with 0xFE.
//! Actions in cells show some of these records: "Show text" shows record
//! 8600 + its number, for example.

use std::collections::BTreeMap;

use super::error::{Arena2Error, Arena2Result};
use super::reader::ByteReader;

/// Ends a record.
const END_OF_RECORD: u8 = 0xFE;
/// Formatting bytes that start a new line: new line, end of page, left and centred lines.
const LINE_BREAKS: [u8; 4] = [0x00, 0xF6, 0xFC, 0xFD];
/// Formatting bytes followed by one byte of their own (a position or a font).
const PREFIXES: [u8; 2] = [0xFB, 0xF9];

/// Every text record of `TEXT.RSC`, as plain text.
pub struct TextRecords {
    records: BTreeMap<u16, String>,
}

impl TextRecords {
    /// Reads `TEXT.RSC` from its bytes.
    pub fn from_bytes(data: &[u8]) -> Arena2Result<Self> {
        let mut header = ByteReader::new(data);
        let table_length = usize::from(header.read_u16()?);
        let count = (table_length / 6).saturating_sub(1);
        let mut records = BTreeMap::new();
        for _ in 0..count {
            let id = header.read_u16()?;
            let offset = header.read_u32()? as usize;
            let bytes = data
                .get(offset..)
                .ok_or_else(|| Arena2Error::format("text record past the end of the file"))?;
            let end = bytes.iter().position(|&byte| byte == END_OF_RECORD).unwrap_or(bytes.len());
            records.entry(id).or_insert_with(|| plain_text(&bytes[..end]));
        }
        Ok(Self { records })
    }

    /// The text of record `id`, or `None` if there is no such record.
    pub fn get(&self, id: u16) -> Option<&str> {
        self.records.get(&id).map(String::as_str)
    }

    /// The id and text of every record from `first` to `last`.
    pub fn range(&self, first: u16, last: u16) -> impl Iterator<Item = (u16, &str)> {
        self.records.range(first..=last).map(|(&id, text)| (id, text.as_str()))
    }
}

/// The text of a record's bytes: formatting bytes that break lines become
/// line breaks, the others are dropped, and each line is trimmed.
fn plain_text(bytes: &[u8]) -> String {
    let mut text = String::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            0x20..=0x7F => text.push(char::from(byte)),
            _ if LINE_BREAKS.contains(&byte) => text.push('\n'),
            _ if PREFIXES.contains(&byte) => index += 1,
            _ => {}
        }
        index += 1;
    }
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    lines.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_records_as_plain_text() {
        let first = b"Hear me:\xFD The Blind God\xFD is jealous.\xFD\xFE";
        let second = b"\xFB\x05Left\xFCline\xFE";
        let table_length = 6 * 3;
        let mut data = Vec::new();
        data.extend_from_slice(&(table_length as u16).to_le_bytes());
        let first_offset = 2 + 6 * 2 + 6;
        for (id, offset) in [(8605u16, first_offset), (7705, first_offset + first.len())] {
            data.extend_from_slice(&id.to_le_bytes());
            data.extend_from_slice(&(offset as u32).to_le_bytes());
        }
        data.extend_from_slice(&[0; 6]);
        data.extend_from_slice(first);
        data.extend_from_slice(second);

        let records = TextRecords::from_bytes(&data).unwrap();
        assert_eq!(records.get(8605), Some("Hear me:\nThe Blind God\nis jealous."));
        assert_eq!(records.get(7705), Some("Left\nline"));
        assert_eq!(records.get(8600), None);
        assert_eq!(records.range(8600, 8700).count(), 1);
    }
}
