//! The wall and floor textures Daggerfall Unity gives a dungeon.
//!
//! Daggerfall's dungeon models are drawn with a standard set of dungeon
//! texture archives (119, 120, 122, 123, 124 and 168). Inside a dungeon,
//! Daggerfall Unity swaps each of those for an archive from a table of six
//! that it picks for the dungeon ([`DungeonTextures`]), so dungeons look
//! different from one another. How the table is picked depends on the
//! player's `RandomDungeonTextures` setting ([`TextureMode`]), on the
//! dungeon's ids and on the climate of its map pixel:
//!
//! * Classic (0): Daggerfall's own way: a pseudo-random table from the
//!   dungeon's location id, with sets that suit the climate.
//! * By climate (1 and 2): the same, with the climate picking the set more
//!   closely. Mode 1 keeps the classic tables for the main story dungeons.
//! * Random (3 and 4): any of the dungeon sets, picked from the dungeon's map
//!   id with Unity's random number generator. Mode 3 keeps the classic tables
//!   for the main story dungeons.
//!
//! Archive 74 (building doors) is swapped for its climate's variant too.
//!
//! The swap only changes which texture is drawn: the models' texture
//! coordinates stay those of the standard textures, as in the game. The exit
//! door and tapestries without an action keep their standard textures,
//! because the game lays them out without the swap.
//!
//! This follows `DaggerfallDungeon.UseLocationDungeonTextureTable` and
//! `DungeonTextureTables` in Daggerfall Unity.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::arena2::TextureKey;
use crate::scene::{SceneObject, SceneObjectKind, EXIT_DOOR_MODEL_ID, TAPESTRY_MODEL_IDS};

/// The standard dungeon texture archives that a dungeon's table replaces, in table order.
pub const STANDARD_TABLE: [u16; 6] = [119, 120, 122, 123, 124, 168];

/// Building door textures, which are swapped for their climate's variant.
const BUILDING_DOOR_ARCHIVE: u16 = 74;

/// `CLIMATE.PAK` values (Daggerfall Unity's `MapsFile.Climates`).
const OCEAN: u8 = 223;
const DESERT: u8 = 224;
const SWAMP: u8 = 228;
const HAUNTED_WOODLANDS: u8 = 232;
/// The climate Daggerfall Unity uses when there is none (Woodlands).
pub const DEFAULT_CLIMATE: u8 = 231;

/// Map ids of the main story dungeons (`DaggerfallDungeon.IsMainStoryDungeon`).
pub const MAIN_STORY_MAP_IDS: [i32; 14] = [
    187_853_213,   // Privateer's Hold
    630_439_035,   // Wayrest
    1_291_010_263, // Daggerfall
    6_634_853,     // Sentinel
    19_021_260,    // Orsinium
    728_811_286,   // Shedungent
    701_948_302,   // Scourg Barrow
    83_032_363,    // Woodborne Hall
    1001,          // Mantellan Crux
    207_828_842,   // Lysandus' Tomb
    9_570_447,     // Castle Necromoghan
    2_352_284,     // Tristore Laboratory
    336_619_236,   // Castle Llugwych
    43_196_334,    // Direnni Tower
];

/// Daggerfall Unity's `RandomDungeonTextures` setting: how a dungeon's texture table is picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TextureMode {
    /// 0: Daggerfall's own algorithm.
    Classic,
    /// 1: by climate, with the classic tables for the main story dungeons.
    ClimateExceptMainStory,
    /// 2: by climate for every dungeon.
    Climate,
    /// 3: random, with the classic tables for the main story dungeons.
    RandomExceptMainStory,
    /// 4: random for every dungeon.
    Random,
}

impl TextureMode {
    pub const ALL: [Self; 5] = [
        Self::Classic,
        Self::ClimateExceptMainStory,
        Self::Climate,
        Self::RandomExceptMainStory,
        Self::Random,
    ];

    /// The mode for the setting's value; Daggerfall Unity treats unknown values as random.
    pub fn from_setting(value: i32) -> Self {
        match value {
            0 => Self::Classic,
            1 => Self::ClimateExceptMainStory,
            2 => Self::Climate,
            3 => Self::RandomExceptMainStory,
            _ => Self::Random,
        }
    }

    /// The setting's value for the mode.
    pub fn setting(self) -> i32 {
        Self::ALL.iter().position(|mode| *mode == self).unwrap_or(0) as i32
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::ClimateExceptMainStory => "By climate, classic for main story dungeons",
            Self::Climate => "By climate",
            Self::RandomExceptMainStory => "Random, classic for main story dungeons",
            Self::Random => "Random",
        }
    }

    /// Whether the editor's tables for the mode might differ from the game's:
    /// the random modes use Unity's random number generator, which the editor
    /// imitates without having been checked against the game.
    pub fn is_approximate(self) -> bool {
        matches!(self, Self::RandomExceptMainStory | Self::Random)
    }
}

/// Which textures the editor draws dungeons with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextureSetting {
    /// Daggerfall's standard dungeon textures, unswapped.
    Standard,
    /// The game's, picked by the `RandomDungeonTextures` value in Daggerfall
    /// Unity's own settings file (classic if it cannot be read).
    #[default]
    AsInGame,
    /// The game's, as picked in `mode`.
    Mode(TextureMode),
}

impl TextureSetting {
    /// The mode to pick tables with, or `None` for the standard textures.
    /// `game_mode` is the mode Daggerfall Unity's settings file gives.
    pub fn mode(self, game_mode: Option<TextureMode>) -> Option<TextureMode> {
        match self {
            Self::Standard => None,
            Self::AsInGame => Some(game_mode.unwrap_or(TextureMode::Classic)),
            Self::Mode(mode) => Some(mode),
        }
    }
}

/// What Daggerfall Unity picks a dungeon's textures from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DungeonIdentity {
    /// The location's map id (`MapTableData.MapId`).
    pub map_id: i32,
    /// The location id of the dungeon's own record (`Dungeon.RecordElement.Header.LocationId`).
    pub dungeon_location_id: u16,
    /// The `CLIMATE.PAK` value of the dungeon's map pixel (223-232), if known.
    pub climate: Option<u8>,
}

/// The textures a dungeon is drawn with in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DungeonTextures {
    /// The archives that take the place of [`STANDARD_TABLE`]'s, in order.
    pub table: [u16; 6],
    /// What is added to archive 74 for the climate: 0, 100, 300 or 400.
    pub climate_offset: u16,
}

impl DungeonTextures {
    /// The textures Daggerfall Unity gives the dungeon `identity` with the setting `mode`.
    pub fn for_dungeon(identity: &DungeonIdentity, mode: TextureMode) -> Self {
        let climate = identity.climate.filter(|climate| (OCEAN..=HAUNTED_WOODLANDS).contains(climate));
        let climate = climate.unwrap_or(DEFAULT_CLIMATE);
        let main_story = MAIN_STORY_MAP_IDS.contains(&identity.map_id);
        let seed = i32::from(identity.dungeon_location_id);
        let table = match mode {
            // Main story dungeons keep Daggerfall's own tables unless every dungeon is changed.
            TextureMode::Classic
            | TextureMode::ClimateExceptMainStory
            | TextureMode::RandomExceptMainStory
                if main_story =>
            {
                classic_table(seed, TextureMode::Classic, climate)
            }
            TextureMode::Classic | TextureMode::ClimateExceptMainStory | TextureMode::Climate => {
                classic_table(seed, mode, climate)
            }
            TextureMode::RandomExceptMainStory | TextureMode::Random => random_table(identity.map_id),
        };
        Self { table, climate_offset: climate_base_offset(climate) }
    }

    /// The texture the game draws in place of `key`.
    pub fn apply(&self, key: TextureKey) -> TextureKey {
        let archive = match key.archive {
            BUILDING_DOOR_ARCHIVE => BUILDING_DOOR_ARCHIVE + self.climate_offset,
            archive => match STANDARD_TABLE.iter().position(|&standard| standard == archive) {
                Some(slot) => self.table[slot],
                None => archive,
            },
        };
        TextureKey { archive, ..key }
    }
}

/// Whether the game draws `object` with its dungeon's textures. The exit door,
/// and tapestries and banners without an action, keep the standard ones.
pub fn uses_dungeon_textures(object: &SceneObject) -> bool {
    match object.kind {
        // The user's imported models keep their own materials, in the game too.
        SceneObjectKind::Model { model_id, .. } => {
            model_id != EXIT_DOOR_MODEL_ID
                && !(TAPESTRY_MODEL_IDS.contains(&model_id) && !object.has_action())
                && !crate::custom_models::is_custom_model(model_id)
        }
        _ => false,
    }
}

/// Daggerfall's own table (`DungeonTextureTables.RandomTextureTableClassic`):
/// five archives picked with Daggerfall's random numbers from `seed`, from the
/// set the climate gives, and the sewer archive of the climate.
fn classic_table(seed: i32, mode: TextureMode, climate: u8) -> [u16; 6] {
    // Which of the archive sets each climate uses.
    const SET_OF_CLIMATE: [usize; 10] = [0, 0, 1, 4, 4, 0, 3, 3, 3, 0];
    // Climates' terrain kinds (`TravelTimeCalculator.climateIndices`), from Ocean.
    const TERRAIN_OF_CLIMATE: [usize; 10] = [0, 0, 0, 1, 2, 3, 4, 5, 5, 5];
    let sets: [u16; 5] =
        if mode == TextureMode::Classic { [19, 119, 319, 419, 119] } else { [19, 119, 119, 319, 419] };
    let climate = if climate == OCEAN { SWAMP } else { climate };
    let by_climate = usize::from(climate - DESERT);
    let set = match mode {
        TextureMode::Classic => SET_OF_CLIMATE[TERRAIN_OF_CLIMATE[usize::from(climate - OCEAN)]],
        _ => SET_OF_CLIMATE[by_climate],
    };
    let mut random = DaggerfallRandom::new(seed);
    let mut table = [0; 6];
    for slot in table.iter_mut().take(5) {
        // An offset of 2 would be a missing archive, so it becomes 4.
        let offset = match random.next() % 5 {
            2 => 4,
            offset => offset,
        };
        *slot = sets[set] + offset as u16;
    }
    // The sewer textures always follow the climate.
    table[5] = 68 + 100 * SET_OF_CLIMATE[by_climate] as u16;
    table
}

/// Daggerfall Unity's own random table (`RandomTextureTableAlternate`): any
/// dungeon set for each slot, picked from `seed` with Unity's random numbers,
/// with a sewer set in the last slot.
fn random_table(seed: i32) -> [u16; 6] {
    const VALID: [u16; 24] = [
        19, 20, 22, 23, 24, 68, 119, 120, 122, 123, 124, 168, 319, 320, 322, 323, 324, 368, 419, 420, 422,
        423, 424, 468,
    ];
    const SEWERS: [u16; 4] = [68, 168, 368, 468];
    let mut random = UnityRandom::new(seed);
    let mut table = [0; 6];
    for slot in &mut table {
        *slot = VALID[random.range(VALID.len() as u32)];
    }
    if table[5] % 100 != 68 {
        table[5] = SEWERS[random.range(SEWERS.len() as u32)];
    }
    table
}

/// What the climate adds to archive 74 (`MapsFile.GetWorldClimateSettings`):
/// desert 0, mountain 100, temperate 300, swamp 400.
fn climate_base_offset(climate: u8) -> u16 {
    match climate {
        224 | 225 | 229 => 0,
        226 => 100,
        223 | 227 | 228 => 400,
        _ => 300,
    }
}

/// Daggerfall's random numbers (`DFRandom`).
struct DaggerfallRandom {
    next: u64,
}

impl DaggerfallRandom {
    /// As `DFRandom.srand(int)`.
    fn new(seed: i32) -> Self {
        Self { next: u64::from(seed as u32) }
    }

    /// As `DFRandom.rand()`: 0 to 32767.
    fn next(&mut self) -> u32 {
        self.next = self.next.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        ((self.next >> 16) & 0x7FFF) as u32
    }
}

/// Unity's random numbers (`UnityEngine.Random`, a xorshift128 generator),
/// as they are commonly described; not checked against the game.
struct UnityRandom {
    state: [u32; 4],
}

impl UnityRandom {
    /// As `Random.InitState(seed)`.
    fn new(seed: i32) -> Self {
        let first = seed as u32;
        let second = first.wrapping_mul(1_812_433_253).wrapping_add(1);
        let third = second.wrapping_mul(1_812_433_253).wrapping_add(1);
        let fourth = third.wrapping_mul(1_812_433_253).wrapping_add(1);
        Self { state: [first, second, third, fourth] }
    }

    fn next(&mut self) -> u32 {
        let [x, y, z, w] = self.state;
        let t = x ^ (x << 11);
        let next = w ^ (w >> 19) ^ t ^ (t >> 8);
        self.state = [y, z, w, next];
        next
    }

    /// As `Random.Range(0, count)` for integers: 0 to `count - 1`.
    fn range(&mut self, count: u32) -> usize {
        (self.next() % count) as usize
    }
}

/// The `RandomDungeonTextures` value in Daggerfall Unity's settings file, if
/// the file is where Daggerfall Unity keeps it on this computer and has one.
pub fn game_texture_mode() -> Option<TextureMode> {
    let text = std::fs::read_to_string(game_settings_path()?).ok()?;
    parse_texture_mode(&text)
}

/// The `RandomDungeonTextures` value in the text of a settings file.
fn parse_texture_mode(text: &str) -> Option<TextureMode> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "RandomDungeonTextures")
            .then(|| value.trim().parse().ok().map(TextureMode::from_setting))?
    })
}

/// Where Daggerfall Unity keeps its settings (Unity's `persistentDataPath`).
fn game_settings_path() -> Option<PathBuf> {
    const COMPANY: &str = "Daggerfall Workshop";
    const PRODUCT: &str = "Daggerfall Unity";
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let folder = if cfg!(windows) {
        home?.join("AppData").join("LocalLow")
    } else if cfg!(target_os = "macos") {
        home?.join("Library").join("Application Support")
    } else {
        match std::env::var_os("XDG_CONFIG_HOME") {
            Some(config) => PathBuf::from(config),
            None => home?.join(".config"),
        }
        .join("unity3d")
    };
    Some(folder.join(COMPANY).join(PRODUCT).join("settings.ini"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daggerfalls_random_numbers_match_dfrandom() {
        // srand(1): (1103515245 + 12345) >> 16 = 16838.
        let mut random = DaggerfallRandom::new(1);
        assert_eq!(random.next(), 16838);
        // The state keeps growing past 32 bits, as DFRandom's ulong does; only the low bits count.
        let mut random = DaggerfallRandom::new(-1);
        let low = (0xFFFF_FFFFu64 * 1_103_515_245 + 12_345) >> 16 & 0x7FFF;
        assert_eq!(u64::from(random.next()), low);
    }

    #[test]
    fn classic_tables_use_the_climates_set() {
        // Classic only uses the 19 and 119 sets ("swamp and woodland texture sets
        // unused"): woodlands get 19, 20, 22 or 23 (offsets 0, 1, 3 and 4; never the
        // missing 21), but the sewers follow
        // the climate, which for woodlands is the temperate set: 368.
        let woodland = classic_table(26_220, TextureMode::Classic, 231);
        assert!(woodland[..5].iter().all(|archive| [19, 20, 22, 23].contains(archive)), "{woodland:?}");
        assert_eq!(woodland[5], 368);
        // Swamps get the 119 set and swamp sewers.
        let swamp = classic_table(26_220, TextureMode::Classic, 228);
        assert!(swamp[..5].iter().all(|archive| [119, 120, 122, 123].contains(archive)), "{swamp:?}");
        assert_eq!(swamp[5], 468);
        let desert = classic_table(26_220, TextureMode::Classic, 224);
        assert_eq!(desert[5], 68);
        // The same seed always gives the same table.
        assert_eq!(classic_table(26_220, TextureMode::Classic, 231), woodland);
        // By climate, swamps use the 419 set.
        let swamp = classic_table(26_220, TextureMode::Climate, 228);
        assert!(swamp[..5].iter().all(|archive| [419, 420, 422, 423].contains(archive)), "{swamp:?}");
    }

    #[test]
    fn main_story_dungeons_keep_classic_tables_unless_every_dungeon_changes() {
        let hold = DungeonIdentity { map_id: 187_853_213, dungeon_location_id: 1234, climate: Some(228) };
        let classic = DungeonTextures::for_dungeon(&hold, TextureMode::Classic);
        assert_eq!(DungeonTextures::for_dungeon(&hold, TextureMode::ClimateExceptMainStory), classic);
        assert_eq!(DungeonTextures::for_dungeon(&hold, TextureMode::RandomExceptMainStory), classic);
        let other = DungeonIdentity { map_id: 5, ..hold };
        assert_eq!(
            DungeonTextures::for_dungeon(&other, TextureMode::RandomExceptMainStory),
            DungeonTextures::for_dungeon(&other, TextureMode::Random)
        );
        let random = DungeonTextures::for_dungeon(&other, TextureMode::Random);
        assert_eq!(random.table[5] % 100, 68, "the last slot is always sewers");
    }

    #[test]
    fn swaps_standard_archives_and_building_doors() {
        let textures = DungeonTextures { table: [19, 20, 23, 24, 19, 68], climate_offset: 400 };
        assert_eq!(textures.apply(TextureKey::new(120, 3)), TextureKey::new(20, 3));
        assert_eq!(textures.apply(TextureKey::new(168, 7)), TextureKey::new(68, 7));
        assert_eq!(textures.apply(TextureKey::new(74, 2)), TextureKey::new(474, 2));
        assert_eq!(textures.apply(TextureKey::new(121, 0)), TextureKey::new(121, 0), "not in the table");
    }

    #[test]
    fn reads_the_setting_from_the_settings_file() {
        let text = "[Video]\nRandomDungeonTextures = 2\n";
        assert_eq!(parse_texture_mode(text), Some(TextureMode::Climate));
        assert_eq!(parse_texture_mode("RandomDungeonTextures=0"), Some(TextureMode::Classic));
        assert_eq!(parse_texture_mode("Other = 1"), None);
    }
}
