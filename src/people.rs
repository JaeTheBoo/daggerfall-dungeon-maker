//! The people of Daggerfall's buildings and settlements, as Daggerfall Unity
//! makes them (`StaticNPC`): their gender, whether they are children, and
//! their names.
//!
//! * Inside, bit 32 of a person's flags makes a woman; outdoors, the image's
//!   record in `FLATS.CFG` decides ([`outdoor_gender`]).
//! * A person of an individual faction is that named person, and takes the
//!   faction's name. Anyone else gets a name made up from the region's name
//!   bank (`NameHelper.FullName`), with Daggerfall Unity's word lists, seeded
//!   by where their record is in the cell's data, the building's key and the
//!   location's place in its region ([`name_seed`]). Daggerfall Unity's own
//!   code calls that seed its best guess at the original game's, so the
//!   names are Daggerfall Unity's.
//! * Their race is their faction's, or else the region's ([`PersonRace`]);
//!   their faction's social and guild groups decide how they talk and which
//!   guild's services and quests they give.
//! * In dungeons, the flats of the people archives are people ([`is_dungeon_person`]).

use crate::arena2::{FlatsConfig, GameReferences, TextureKey, INDIVIDUAL_FACTION_KIND};
use crate::dungeon_textures::DaggerfallRandom;
use crate::unity_install::UnityText;

/// A person's gender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gender {
    Male,
    Female,
}

impl Gender {
    /// As a person's flags give it inside buildings (`StaticNPC.SetLayoutData`).
    pub fn from_flags(flags: u8) -> Self {
        match flags & FEMALE_FLAG {
            0 => Self::Male,
            _ => Self::Female,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Male => "Male",
            Self::Female => "Female",
        }
    }
}

/// The flag bit that makes a person a woman.
const FEMALE_FLAG: u8 = 32;

/// The gender of a person outdoors: their image's in `FLATS.CFG` (a "2" in
/// its gender field makes a woman), or their flags' when it has no record.
pub fn outdoor_gender(texture: TextureKey, flags: u8, flats: &FlatsConfig) -> Gender {
    match flats.get(texture) {
        Some(info) if info.is_female() => Gender::Female,
        Some(_) => Gender::Male,
        None => Gender::from_flags(flags),
    }
}

/// The faction of children.
pub const CHILDREN_FACTION_ID: u16 = 514;

/// The images of children (`TextureReader.IsChildNPCTexture`): archive and records.
const CHILD_TEXTURES: [(u16, &[u8]); 8] = [
    (181, &[3]),
    (182, &[4, 5, 6, 18, 36, 37, 38, 42, 43, 52, 53]),
    (184, &[15]),
    (186, &[4, 5, 6, 7, 19, 37, 38, 39, 43, 44, 53, 54]),
    (197, &[3]),
    (334, &[2, 3, 6, 9, 12]),
    (346, &[2, 3, 12, 15, 16, 18]),
    (357, &[5, 6, 7, 8]),
];

/// Whether a person drawn with `texture`, of faction `faction_id`, is a child.
pub fn is_child(texture: TextureKey, faction_id: u16) -> bool {
    faction_id == CHILDREN_FACTION_ID
        || CHILD_TEXTURES
            .iter()
            .any(|(archive, records)| *archive == texture.archive && records.contains(&texture.record))
}

/// The name banks of Daggerfall Unity's `NameGen.txt` (`NameHelper.BankTypes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameBank {
    Breton,
    Redguard,
    Nord,
    DarkElf,
    HighElf,
    WoodElf,
    Khajiit,
    Imperial,
}

impl NameBank {
    /// In the bank order of `NameHelper.BankTypes`.
    const ALL: [Self; 8] = [
        Self::Breton,
        Self::Redguard,
        Self::Nord,
        Self::DarkElf,
        Self::HighElf,
        Self::WoodElf,
        Self::Khajiit,
        Self::Imperial,
    ];

    /// The bank of the people of region `region_index` (`PlayerGPS.GetNameBankOfCurrentRegion`).
    pub fn of_region(region_index: usize) -> Self {
        let bank = REGION_RACES.get(region_index).copied().unwrap_or(0);
        Self::ALL.get(usize::from(bank)).copied().unwrap_or(Self::Breton)
    }

    /// Its name in `NameGen.txt`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Breton => "Breton",
            Self::Redguard => "Redguard",
            Self::Nord => "Nord",
            Self::DarkElf => "DarkElf",
            Self::HighElf => "HighElf",
            Self::WoodElf => "WoodElf",
            Self::Khajiit => "Khajiit",
            Self::Imperial => "Imperial",
        }
    }
}

/// The name bank of each region (`MapsFile.RegionRaces`): 0 Breton, 1 Redguard.
const REGION_RACES: [u8; 62] = [
    1, 1, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1,
];

/// One part from set `set` of `bank`, drawn as `DFRandom.rand() % length`.
fn part(bank: &[Vec<String>], set: usize, random: &mut DaggerfallRandom) -> Option<String> {
    let parts = bank.get(set).filter(|parts| !parts.is_empty())?;
    Some(parts[random.next() as usize % parts.len()].clone())
}

/// A first name from `bank` (`NameHelper.FirstName`): two parts (men's sets
/// 0 and 1, women's 2 and 3); a Redguard's three, and a fourth always for a
/// woman and three times in four for a man.
pub(crate) fn first_name(
    unity: &UnityText,
    bank: NameBank,
    gender: Gender,
    random: &mut DaggerfallRandom,
) -> Option<String> {
    let parts = unity.name_bank(bank.key())?;
    if bank != NameBank::Redguard {
        let first = if gender == Gender::Male { 0 } else { 2 };
        return Some(part(parts, first, random)? + &part(parts, first + 1, random)?);
    }
    let mut name = part(parts, 0, random)? + &part(parts, 1, random)? + &part(parts, 2, random)?;
    if gender == Gender::Female || random.next() % 100 < 75 {
        name += &part(parts, if gender == Gender::Male { 3 } else { 4 }, random)?;
    }
    Some(name)
}

/// A surname from `bank` (`NameHelper.Surname`): two parts from sets 4 and
/// 5; a Nord's from the men's first-name sets and a fixed ending; none for a Redguard.
pub(crate) fn surname(unity: &UnityText, bank: NameBank, random: &mut DaggerfallRandom) -> Option<String> {
    let parts = unity.name_bank(bank.key())?;
    match bank {
        NameBank::Redguard => Some(String::new()),
        NameBank::Nord => {
            let ending = unity.string("nordSurnameImmutableSuffix").unwrap_or("sen");
            Some(part(parts, 0, random)? + &part(parts, 1, random)? + ending)
        }
        _ => Some(part(parts, 4, random)? + &part(parts, 5, random)?),
    }
}

/// A full name from `bank`, as `DFRandom.srand(seed)` then
/// `NameHelper.FullName` make it: the first name, then the surname, if any.
pub fn full_name(unity: &UnityText, bank: NameBank, gender: Gender, seed: i32) -> Option<String> {
    let mut random = DaggerfallRandom::new(seed);
    let first = first_name(unity, bank, gender, &mut random)?;
    let last = surname(unity, bank, &mut random)?;
    Some(if last.is_empty() { first } else { format!("{first} {last}") })
}

/// The key Daggerfall Unity gives building `record` of the cell at
/// `column` (from the west) and `row` (from the south) of a location's grid
/// (`BuildingDirectory.MakeBuildingKey`). Quests and saved games refer to
/// buildings by it.
pub fn building_key(column: usize, row: usize, record: usize) -> i32 {
    let key = (i32::from(column as u8) << 16) + (i32::from(row as u8) << 8) + i32::from(record as u8);
    if key == 0 {
        BUILDING_KEY_0
    } else {
        key
    }
}

/// The key that stands for a computed key of 0.
const BUILDING_KEY_0: i32 = 1 << 24;

/// The seed of the name of a person whose record is at `record_offset` in
/// its cell's data, in the building with `building_key` (0 outdoors), in
/// the location at `index_in_region` in its region's list
/// (`StaticNPC.SetLayoutData`: position ^ buildingKey + locationIndex).
pub fn name_seed(record_offset: u32, building_key: i32, index_in_region: usize) -> i32 {
    (record_offset as i32) ^ building_key.wrapping_add(index_in_region as i32)
}

/// The races of people (Daggerfall Unity's `Races`, less the creatures).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Race {
    Breton,
    Redguard,
    Nord,
    DarkElf,
    HighElf,
    WoodElf,
    Khajiit,
    Argonian,
}

impl Race {
    pub fn name(self) -> &'static str {
        match self {
            Self::Breton => "Breton",
            Self::Redguard => "Redguard",
            Self::Nord => "Nord",
            Self::DarkElf => "Dark Elf",
            Self::HighElf => "High Elf",
            Self::WoodElf => "Wood Elf",
            Self::Khajiit => "Khajiit",
            Self::Argonian => "Argonian",
        }
    }

    /// The race of a faction's race number (`RaceTemplate.GetRaceFromFactionRace`), if it is a person's.
    fn of_faction_race(race: i32) -> Option<Self> {
        Some(match race {
            0 => Self::Nord,
            1 => Self::Khajiit,
            2 => Self::Redguard,
            3 => Self::Breton,
            4 => Self::Argonian,
            5 => Self::WoodElf,
            6 => Self::HighElf,
            7 => Self::DarkElf,
            _ => return None,
        })
    }

    /// The race of the people of region `region_index` (`PlayerGPS.GetRaceOfCurrentRegion`).
    pub fn of_region(region_index: usize) -> Self {
        match REGION_RACES.get(region_index) {
            Some(1) => Self::Redguard,
            _ => Self::Breton,
        }
    }
}

/// A person's race, and where it comes from (`StaticNPC.GetRaceFromFaction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonRace {
    /// Their faction's.
    Faction(Race),
    /// Their region's, their faction having none of a person's.
    Region(Race),
    /// Their region's, which is not known here.
    RegionUnknown,
}

impl PersonRace {
    /// For example "Breton", or "Redguard (the region's people)".
    pub fn describe(self) -> String {
        match self {
            Self::Faction(race) => race.name().to_string(),
            Self::Region(race) => format!("{} (the region's people)", race.name()),
            Self::RegionUnknown => "The region's people's (Breton or Redguard)".to_string(),
        }
    }
}

/// The archives of the images of people in dungeons (`RDBLayout.NPCFlatArchives`).
const DUNGEON_PEOPLE_ARCHIVES: [u16; 13] = [334, 346, 357, 175, 176, 177, 178, 179, 180, 181, 182, 183, 184];

/// Whether Daggerfall Unity makes a person of a dungeon's flat drawn with
/// `texture`: it goes by the image's archive alone.
pub fn is_dungeon_person(texture: TextureKey) -> bool {
    DUNGEON_PEOPLE_ARCHIVES.contains(&texture.archive)
}

/// A person's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonName {
    /// A named person, of an individual faction: the faction's name.
    Named(String),
    /// Made up from the region's name bank, as Daggerfall Unity makes it.
    MadeUp(String),
    /// Not known here, and why.
    Unknown(&'static str),
}

impl PersonName {
    /// The name, if known.
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Named(name) | Self::MadeUp(name) => Some(name),
            Self::Unknown(_) => None,
        }
    }
}

/// Who a person is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonDetails {
    pub gender: Gender,
    pub child: bool,
    pub name: PersonName,
    pub race: PersonRace,
    /// Their faction's social group and guild group, if it is in `FACTION.TXT`.
    pub groups: Option<(i32, i32)>,
    /// What `FLATS.CFG` says their image shows, for example "beautiful maiden".
    pub caption: Option<String>,
}

/// Where a person is, which their name is made from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersonPlace {
    /// The region the location is in, whose name bank names them.
    pub region_index: usize,
    /// The location's place in its region's list.
    pub index_in_region: usize,
    /// The key of the building they are in; 0 outdoors.
    pub building_key: i32,
}

/// Who a person drawn with `texture`, of faction `faction_id` and `gender`,
/// is. Their name is made from where their record is (`record_offset`) in
/// `place`, or `place` says why it cannot be made here.
pub fn person_details(
    texture: TextureKey,
    faction_id: u16,
    gender: Gender,
    record_offset: u32,
    place: Result<PersonPlace, &'static str>,
    unity: Option<&UnityText>,
    references: &GameReferences,
) -> PersonDetails {
    let faction = references.factions.get(&faction_id);
    // A faction missing from the file reads as race 0 (Nord) in Daggerfall Unity, as here.
    let faction_race = match faction_id {
        0 => None,
        _ => Race::of_faction_race(faction.map_or(0, |faction| faction.race)),
    };
    let race = match (faction_race, place) {
        (Some(race), _) => PersonRace::Faction(race),
        (None, Ok(place)) => PersonRace::Region(Race::of_region(place.region_index)),
        (None, Err(_)) => PersonRace::RegionUnknown,
    };
    let name = match (faction, place, unity) {
        (Some(faction), ..) if faction.kind == INDIVIDUAL_FACTION_KIND => {
            PersonName::Named(faction.name.clone())
        }
        (_, Err(why), _) => PersonName::Unknown(why),
        (_, _, None) => {
            PersonName::Unknown("Needs Daggerfall Unity's folder (File > Set Daggerfall Unity folder…)")
        }
        (_, Ok(place), Some(unity)) => {
            let seed = name_seed(record_offset, place.building_key, place.index_in_region);
            match full_name(unity, NameBank::of_region(place.region_index), gender, seed) {
                Some(name) => PersonName::MadeUp(name),
                None => PersonName::Unknown("Daggerfall Unity's NameGen.txt lacks this region's name bank"),
            }
        }
    };
    PersonDetails {
        gender,
        child: is_child(texture, faction_id),
        name,
        race,
        groups: faction.map(|faction| (faction.social_group, faction.guild_group)),
        caption: references.flats.get(texture).map(|info| info.caption.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genders_come_from_flags_inside_and_flats_cfg_outdoors() {
        assert_eq!(Gender::from_flags(32), Gender::Female);
        assert_eq!(Gender::from_flags(0x1f), Gender::Male);
        let flats = FlatsConfig::parse("175 0\nmaiden\n?2\n0\n0\n1\n\n175 1\nwarrior\n1\n0\n0\n1\n");
        assert_eq!(outdoor_gender(TextureKey::new(175, 0), 0, &flats), Gender::Female);
        // The file's record wins over the flags.
        assert_eq!(outdoor_gender(TextureKey::new(175, 1), 32, &flats), Gender::Male);
        assert_eq!(outdoor_gender(TextureKey::new(176, 0), 32, &flats), Gender::Female);
    }

    #[test]
    fn races_from_the_faction_or_the_region() {
        assert_eq!(Race::of_faction_race(3), Some(Race::Breton));
        assert_eq!(Race::of_faction_race(18), None, "a vampire is no person's race");
        assert_eq!(Race::of_region(0), Race::Redguard);
        assert_eq!(Race::of_region(17), Race::Breton);
        assert!(is_dungeon_person(TextureKey::new(334, 2)) && !is_dungeon_person(TextureKey::new(210, 0)));
    }

    #[test]
    fn children_by_image_or_faction() {
        assert!(is_child(TextureKey::new(182, 4), 0));
        assert!(is_child(TextureKey::new(175, 0), CHILDREN_FACTION_ID));
        assert!(!is_child(TextureKey::new(182, 3), 0));
    }

    #[test]
    fn building_keys_pack_the_cell_and_the_record() {
        assert_eq!(building_key(2, 1, 3), (2 << 16) + (1 << 8) + 3);
        assert_eq!(building_key(0, 0, 0), 1 << 24, "a key of 0 stands for itself");
        // Position ^ (key + index): addition first, as C# reads it.
        assert_eq!(name_seed(0x100, 0x10, 1), 0x100 ^ 0x11);
    }

    #[test]
    fn names_follow_the_banks_rules() {
        let text = r#"{
            "Breton": {"sets": [{"parts": ["Al"]}, {"parts": ["ric"]}, {"parts": ["El"]}, {"parts": ["la"]},
                                {"parts": ["Dun"]}, {"parts": ["moor"]}]},
            "Redguard": {"sets": [{"parts": ["A"]}, {"parts": ["b"]}, {"parts": ["c"]}, {"parts": ["d"]}, {"parts": ["e"]}]}
        }"#;
        let unity = UnityText::from_parts(Default::default(), text).unwrap();
        assert_eq!(full_name(&unity, NameBank::Breton, Gender::Male, 7).as_deref(), Some("Alric Dunmoor"));
        assert_eq!(full_name(&unity, NameBank::Breton, Gender::Female, 7).as_deref(), Some("Ella Dunmoor"));
        // A Redguard woman always has the fourth part, from her own set, and no surname.
        assert_eq!(full_name(&unity, NameBank::Redguard, Gender::Female, 7).as_deref(), Some("Abce"));
        assert_eq!(NameBank::of_region(17), NameBank::Breton);
        assert_eq!(NameBank::of_region(0), NameBank::Redguard);
    }
}
