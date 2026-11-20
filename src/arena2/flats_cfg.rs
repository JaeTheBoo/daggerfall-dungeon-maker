//! `FLATS.CFG`: what each image of a person (or creature) shows, and the
//! gender the game gives a person outdoors drawn with it.
//!
//! The file is text: one record per image, six lines each, records parted by
//! a blank line. The lines are the image's archive and record ("175 0"), a
//! caption ("beautiful maiden"), the gender (1 a man, 2 a woman; a leading
//! "?" marks an image ChildGard hides), two numbers the game's code makes no
//! use of, and the face shown when talking (a picture of `TFAC00I0.RCI`).
//! Daggerfall Unity reads it in `FlatsFile`; the first record of an image wins.

use std::collections::HashMap;

use super::TextureKey;

/// What `FLATS.CFG` tells of one image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatInfo {
    /// What the image shows, for example "beautiful maiden".
    pub caption: String,
    /// The gender field as written: "1" a man, "2" a woman, perhaps after a "?".
    pub gender: String,
    /// The face shown when talking: its picture in `TFAC00I0.RCI`.
    pub face_index: i32,
}

impl FlatInfo {
    /// Whether the image is of a woman, as Daggerfall Unity reads the gender
    /// field (`StaticNPC.SetLayoutData`: a "2" anywhere in it).
    pub fn is_female(&self) -> bool {
        self.gender.contains('2')
    }
}

/// Every record of `FLATS.CFG`, by image.
#[derive(Debug, Clone, Default)]
pub struct FlatsConfig {
    records: HashMap<TextureKey, FlatInfo>,
}

impl FlatsConfig {
    /// Reads the text of `FLATS.CFG`; records that do not read are skipped.
    pub fn parse(text: &str) -> Self {
        let mut records = HashMap::new();
        let mut block: Vec<&str> = Vec::new();
        let lines = text.lines().map(str::trim).chain(std::iter::once(""));
        for line in lines {
            if !line.is_empty() {
                block.push(line);
                continue;
            }
            if let Some((texture, info)) = read_record(&block) {
                records.entry(texture).or_insert(info);
            }
            block.clear();
        }
        Self { records }
    }

    /// What the file tells of `texture`, if it has a record for it.
    pub fn get(&self, texture: TextureKey) -> Option<&FlatInfo> {
        self.records.get(&texture)
    }

    /// How many images it describes.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether it describes none (the file is missing or empty).
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// One record: its six lines.
fn read_record(lines: &[&str]) -> Option<(TextureKey, FlatInfo)> {
    let [image, caption, gender, _, _, face, ..] = lines else { return None };
    let mut numbers = image.split(' ');
    let archive = numbers.next()?.parse().ok()?;
    let record = numbers.next()?.parse().ok()?;
    if numbers.next().is_some() {
        return None;
    }
    let info =
        FlatInfo { caption: caption.to_string(), gender: gender.to_string(), face_index: face.parse().ok()? };
    Some((TextureKey::new(archive, record), info))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_records_parted_by_blank_lines() {
        let text = "175 0\r\nbeautiful maiden\r\n?2\r\n-2\r\n2\r\n410\r\n\r\n175 1\r\ndark warrior\r\n1\r\n-2\r\n2\r\n411\r\n\r\n\
                    175 0\r\nsecond record of the same image\r\n1\r\n0\r\n0\r\n1\r\n\r\nbroken\r\n";
        let flats = FlatsConfig::parse(text);
        assert_eq!(flats.len(), 2);
        let maiden = flats.get(TextureKey::new(175, 0)).unwrap();
        assert_eq!((maiden.caption.as_str(), maiden.face_index), ("beautiful maiden", 410));
        assert!(maiden.is_female(), "a ChildGard-hidden woman is still a woman");
        assert!(!flats.get(TextureKey::new(175, 1)).unwrap().is_female());
        assert!(flats.get(TextureKey::new(175, 2)).is_none());
    }
}
