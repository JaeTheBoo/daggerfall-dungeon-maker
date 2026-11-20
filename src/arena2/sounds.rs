//! Daggerfall's sound effects, stored in `DAGGER.SND`.
//!
//! `DAGGER.SND` is a BSA archive whose records are numbered. Each record is
//! raw 8-bit unsigned mono sound at 11025 samples a second. An action in a
//! cell names its sound by record number (Daggerfall Unity looks the number
//! up in the archive's directory), while the names Daggerfall Unity gives
//! sounds follow their place in the archive.

use super::bsa::BsaArchive;
use super::error::Arena2Result;
use super::sound_names::SOUND_NAMES;

/// Samples a second of every sound effect.
pub const SAMPLE_RATE: u32 = 11025;

/// The sound effects of `DAGGER.SND`.
pub struct SoundEffects {
    archive: BsaArchive,
}

impl SoundEffects {
    /// Reads `DAGGER.SND` from its bytes.
    pub fn from_bytes(data: Vec<u8>) -> Arena2Result<Self> {
        Ok(Self { archive: BsaArchive::from_bytes(data)? })
    }

    /// The number of every sound that has sound data, in ascending order.
    pub fn ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self
            .archive
            .entries()
            .iter()
            .filter_map(|entry| entry.id)
            .filter(|&id| self.samples(id).is_some())
            .collect();
        ids.sort_unstable();
        ids
    }

    /// The samples of sound `id`, or `None` if there is no such sound or it is empty.
    pub fn samples(&self, id: u32) -> Option<&[u8]> {
        self.archive.record_by_id(id).filter(|samples| !samples.is_empty())
    }

    /// The name of sound `id`, for example "Dungeon door open", or its
    /// number if Daggerfall Unity has no name for it. `None` if there is no
    /// such sound.
    pub fn name(&self, id: u32) -> Option<String> {
        self.samples(id)?;
        let index = self.archive.index_of_id(id)?;
        let name = SOUND_NAMES.iter().find(|(place, _)| usize::from(*place) == index).map(|(_, name)| *name);
        Some(name.map_or_else(|| format!("Sound {id}"), str::to_string))
    }

    /// Sound `id` as a WAV file, or `None` if there is no such sound.
    pub fn wav(&self, id: u32) -> Option<Vec<u8>> {
        self.samples(id).map(wav_file)
    }
}

/// An 8-bit mono WAV file of `samples` at [`SAMPLE_RATE`].
fn wav_file(samples: &[u8]) -> Vec<u8> {
    let data_length = samples.len() as u32;
    let mut wav = Vec::with_capacity(samples.len() + 44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(data_length + 36).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // Size of the format block.
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM.
    wav.extend_from_slice(&1u16.to_le_bytes()); // One channel.
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes()); // Bytes a second.
    wav.extend_from_slice(&1u16.to_le_bytes()); // Bytes a sample.
    wav.extend_from_slice(&8u16.to_le_bytes()); // Bits a sample.
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_length.to_le_bytes());
    wav.extend_from_slice(samples);
    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A number-record archive with sounds 3 (two samples), 4 (empty) and 375 (one sample).
    fn archive() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&3i16.to_le_bytes());
        data.extend_from_slice(&0x0200u16.to_le_bytes());
        data.extend_from_slice(&[10, 20, 30]);
        for (id, size) in [(3u32, 2i32), (4, 0), (375, 1)] {
            data.extend_from_slice(&id.to_le_bytes());
            data.extend_from_slice(&size.to_le_bytes());
        }
        data
    }

    #[test]
    fn finds_sounds_by_number_and_names_them_by_place() {
        let sounds = SoundEffects::from_bytes(archive()).unwrap();
        assert_eq!(sounds.ids(), [3, 375]);
        assert_eq!(sounds.samples(3), Some(&[10u8, 20][..]));
        assert_eq!(sounds.samples(4), None, "an empty record has no sound");
        // Sound 3 is first in the archive, which Daggerfall Unity calls "SpookyHigh".
        assert_eq!(sounds.name(3).as_deref(), Some("Spooky high"));
        assert_eq!(sounds.name(375).as_deref(), Some("Spooky low"));
        assert_eq!(sounds.name(9), None);

        let wav = sounds.wav(375).unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 45);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), SAMPLE_RATE);
        assert_eq!(wav[44], 30);
    }
}
