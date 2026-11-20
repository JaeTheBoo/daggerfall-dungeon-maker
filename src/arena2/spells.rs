//! Daggerfall's standard spells, stored in `SPELLS.STD`.
//!
//! The file is a run of 89 byte records. Each holds a spell's effects and
//! costs, its name in 25 bytes from byte 47, and from byte 73 the number the
//! game knows it by. A "Cast spell" action casts the spell with its number.

use std::collections::BTreeMap;

/// Size of one spell record.
const SPELL_RECORD_SIZE: usize = 0x59;
/// Where a record's name starts, and its length.
const NAME_OFFSET: usize = 47;
const NAME_LENGTH: usize = 25;
/// Where a record's spell number is.
const NUMBER_OFFSET: usize = 73;

/// The name of every spell in `SPELLS.STD`, by its number. The first spell
/// with a number keeps it, as in Daggerfall Unity.
pub fn read_spell_names(data: &[u8]) -> BTreeMap<u8, String> {
    let mut spells = BTreeMap::new();
    for record in data.as_chunks::<SPELL_RECORD_SIZE>().0 {
        let name = &record[NAME_OFFSET..NAME_OFFSET + NAME_LENGTH];
        let name: String = name.iter().take_while(|&&byte| byte != 0).map(|&byte| char::from(byte)).collect();
        spells.entry(record[NUMBER_OFFSET]).or_insert_with(|| name.trim().to_string());
    }
    spells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_spells_by_their_number() {
        let record = |number: u8, name: &str| {
            let mut bytes = vec![0u8; SPELL_RECORD_SIZE];
            bytes[NAME_OFFSET..NAME_OFFSET + name.len()].copy_from_slice(name.as_bytes());
            bytes[NUMBER_OFFSET] = number;
            bytes
        };
        let data = [record(3, "Frostbite"), record(1, "Fenrik's Door Jam"), record(3, "Second")].concat();
        let spells = read_spell_names(&data);
        assert_eq!(spells.len(), 2);
        assert_eq!(spells[&1], "Fenrik's Door Jam");
        assert_eq!(spells[&3], "Frostbite");
    }
}
