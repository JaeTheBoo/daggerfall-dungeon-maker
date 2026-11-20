//! The buildings of Daggerfall's settlements, as Daggerfall Unity sees them:
//! which of a location's own buildings each building of its cells is, what
//! the game calls it, where its entrances are, and its inside laid out.
//!
//! * A location lists its named buildings (shops, taverns, temples, guild
//!   halls, its palace) with their name seed, faction and quality. Daggerfall
//!   Unity hands them out to the named buildings of its cells in order, each
//!   to the next building of the same type (`RMBLayout.GetLocationBuildingData`):
//!   [`settlement_buildings`].
//! * The name comes from the seed, the type and the faction
//!   (`FormulaHelper.GenerateBuildingName`), with word lists from Daggerfall
//!   Unity's own install: [`BuildingNamer`].
//! * A building is entered through the faces of its models drawn with door
//!   textures, and a dungeon through faces drawn with dungeon door textures:
//!   [`door_boxes`].
//! * Its inside is laid out as `DaggerfallInterior` does: [`interior_layout`],
//!   with the rooms of the few insides stored by their floor raised to fit.
//!   It is left through the same kind of faces of its own models:
//!   [`interior_exits`]; walking in, the player lands in front of one of
//!   them: [`arrival_exit`].
//! * A few doors drawn with dungeon door textures stand where an exit of
//!   their building's own inside is, and so lead into it as the data means
//!   them: [`dungeon_textured_entrances`].

use std::collections::HashMap;
use std::ops::Range;

use glam::{Mat4, Vec3};

use crate::arena2::{
    native_to_world, Arena2Library, DoorKind, Faction, GameReferences, InteriorFlat, LocationBuilding,
    LocationDefinition, ModelMesh, RmbBlock, RmbInterior, TextureKey, PROVINCE_FACTION_KIND, REGION_NAMES,
    WORLD_SCALE,
};
use crate::dungeon_textures::DaggerfallRandom;
use crate::people::{first_name, Gender, NameBank};
use crate::picking::Ray;
use crate::scene::{
    box_corners, FlatData, ModelData, Scene, SceneObject, SceneObjectKind, MARKER_TEXTURE_ARCHIVE,
};
use crate::unity_install::UnityText;

/// Building types (Daggerfall Unity's `DFLocation.BuildingTypes`).
pub const ALCHEMIST: u8 = 0;
pub const HOUSE_FOR_SALE: u8 = 1;
pub const ARMORER: u8 = 2;
pub const BANK: u8 = 3;
pub const BOOKSELLER: u8 = 5;
pub const CLOTHING_STORE: u8 = 6;
pub const FURNITURE_STORE: u8 = 7;
pub const GEM_STORE: u8 = 8;
pub const GENERAL_STORE: u8 = 9;
pub const LIBRARY: u8 = 10;
pub const GUILD_HALL: u8 = 11;
pub const PAWN_SHOP: u8 = 12;
pub const WEAPON_SMITH: u8 = 13;
pub const TEMPLE: u8 = 14;
pub const TAVERN: u8 = 15;
pub const PALACE: u8 = 16;
pub const CITY_WALL: u8 = 23;

/// The building of the Order of the Raven that Daggerfall Unity, as the
/// original game, makes a guild hall of that order, whatever its type.
const RAVEN_BUILDING_NAME: &str = "KRAVE01.HS2";
const RAVEN_FACTION_ID: u16 = 414;

/// Whether buildings of `building_type` take their name seed, faction and
/// quality from the location's list (`RMBLayout.IsNamedBuilding`).
pub fn is_named_building(building_type: u8) -> bool {
    matches!(
        building_type,
        ALCHEMIST
            | ARMORER
            | BANK
            | BOOKSELLER
            | CLOTHING_STORE
            | FURNITURE_STORE
            | GEM_STORE
            | GENERAL_STORE
            | LIBRARY
            | GUILD_HALL
            | PAWN_SHOP
            | WEAPON_SMITH
            | TEMPLE
            | TAVERN
            | PALACE
    )
}

/// Every building of `location`'s cells (`blocks`, in the location's order;
/// `None` for one that could not be read) as Daggerfall Unity sees them:
/// each named building takes the next unused building of the same type from
/// the location's list. Others keep the data their cell gives them.
pub fn settlement_buildings(
    location: &LocationDefinition,
    blocks: &[Option<&RmbBlock>],
) -> Vec<Vec<LocationBuilding>> {
    let named: Vec<&LocationBuilding> = location
        .exterior
        .buildings
        .iter()
        .filter(|building| is_named_building(building.building_type))
        .collect();
    let mut used = vec![false; named.len()];
    blocks
        .iter()
        .map(|block| {
            let Some(block) = block else { return Vec::new() };
            block
                .buildings
                .iter()
                .map(|building| {
                    let mut data = building.data;
                    if !is_named_building(data.building_type) {
                        return data;
                    }
                    // A building the list runs out for gets nothing from it, as in the game.
                    let found = (0..named.len())
                        .find(|&index| !used[index] && named[index].building_type == data.building_type);
                    let from_list = found.map_or_else(LocationBuilding::default, |index| {
                        used[index] = true;
                        *named[index]
                    });
                    data.name_seed = from_list.name_seed;
                    data.faction_id = from_list.faction_id;
                    data.sector = from_list.sector;
                    data.quality = from_list.quality;
                    if building.other_name == RAVEN_BUILDING_NAME {
                        data.building_type = GUILD_HALL;
                        data.faction_id = RAVEN_FACTION_ID;
                    }
                    data
                })
                .collect()
        })
        .collect()
}

/// Names buildings as Daggerfall Unity does.
pub struct BuildingNamer<'names> {
    /// Daggerfall Unity's word lists; without them, shops and taverns have no name.
    pub unity: Option<&'names UnityText>,
    /// Factions (temples and guild halls are named after theirs) and text records (palaces).
    pub references: &'names GameReferences,
}

impl BuildingNamer<'_> {
    /// The name Daggerfall Unity gives `building` in the location
    /// `location_name` of region `region_index`, or `None` for buildings it
    /// does not name (houses…) and those whose words are not available.
    pub fn name(
        &self,
        building: &LocationBuilding,
        location_name: &str,
        region_index: usize,
    ) -> Option<String> {
        let mut random = DaggerfallRandom::new(i32::from(building.name_seed));
        let list = |key: &str| self.unity.map(|unity| unity.list(key)).unwrap_or_default();
        let string = |key: &str, fallback: &'static str| {
            self.unity.and_then(|unity| unity.string(key)).unwrap_or(fallback).to_string()
        };
        // A shop's name: the second half is drawn first, as in the game.
        let shop = |random: &mut DaggerfallRandom, kind: &str| -> Option<(String, String, bool)> {
            let second = pick(random, &list(kind))?;
            Some((pick(random, &list("StoresA"))?, second, false))
        };
        let faction = |id: u16| self.references.factions.get(&id);
        let (mut first, second, singleton) = match building.building_type {
            HOUSE_FOR_SALE => return Some(string("houseForSale", "House for sale")),
            TAVERN => {
                let second = pick(&mut random, &list("TavernsB"))?;
                (pick(&mut random, &list("TavernsA"))?, second, false)
            }
            GENERAL_STORE => shop(&mut random, "GeneralStoresB")?,
            WEAPON_SMITH => shop(&mut random, "WeaponStoresB")?,
            ARMORER => shop(&mut random, "ArmorStoresB")?,
            BOOKSELLER => shop(&mut random, "BookStoresB")?,
            CLOTHING_STORE => shop(&mut random, "ClothingStoresB")?,
            ALCHEMIST => shop(&mut random, "AlchemyStoresB")?,
            GEM_STORE => shop(&mut random, "GemStoresB")?,
            PAWN_SHOP => shop(&mut random, "PawnStoresB")?,
            FURNITURE_STORE => shop(&mut random, "FurnitureStoresB")?,
            LIBRARY => shop(&mut random, "LibraryStoresB")?,
            BANK => {
                let region = REGION_NAMES.get(region_index).copied().unwrap_or_default();
                (string("theBankOf", "The Bank of"), region.to_string(), false)
            }
            GUILD_HALL => (faction(building.faction_id)?.name.clone(), String::new(), true),
            // A temple is named after the first faction under its own.
            TEMPLE => {
                let child = faction(building.faction_id)?.children.first().copied()?;
                (faction(child)?.name.clone(), String::new(), true)
            }
            PALACE => (self.palace_name(location_name), String::new(), true),
            CITY_WALL => (string("cityWall", "City Wall"), String::new(), true),
            _ => return None,
        };

        first = first.replace("%cn", location_name);
        if first.contains("%ef") {
            // The game draws a number it does not use before making the name.
            random.next();
            let bank = NameBank::of_region(region_index);
            let first_name = first_name(self.unity?, bank, Gender::Male, &mut random)?;
            first = first.replace("%ef", &first_name);
        }
        if first.contains("%rt") {
            first = first.replace("%rt", &self.ruler_title(region_index));
        }
        let name = if singleton { first } else { format!("{first} {second}") };
        let name = name.trim().to_string();
        (!name.is_empty()).then_some(name)
    }

    /// The name of the palace of `location_name`: the three great cities'
    /// own (from `TEXT.RSC`), otherwise "Palace".
    fn palace_name(&self, location_name: &str) -> String {
        let record = match location_name {
            "Daggerfall" => Some(475),
            "Wayrest" => Some(476),
            "Sentinel" => Some(477),
            _ => None,
        };
        let own = record
            .and_then(|record| self.references.texts.as_ref()?.get(record))
            .and_then(|text| text.lines().next())
            .map(|line| line.trim_end_matches('.').to_string());
        own.unwrap_or_else(|| {
            self.unity.and_then(|unity| unity.string("palace")).unwrap_or("Palace").to_string()
        })
    }

    /// The title of the ruler of region `region_index`, for example "King".
    fn ruler_title(&self, region_index: usize) -> String {
        let factions = &self.references.factions;
        let province = |region: i32| {
            factions
                .values()
                .find(|faction| faction.kind == PROVINCE_FACTION_KIND && faction.region == region)
        };
        let ruler = province(region_index as i32)
            .or_else(|| province(-1))
            .map_or(-1, |faction: &Faction| faction.ruler);
        let (key, fallback) = match ruler {
            1 => ("King", "King"),
            2 => ("Queen", "Queen"),
            3 => ("Duke", "Duke"),
            4 => ("Duchess", "Duchess"),
            5 => ("Marquis", "Marquis"),
            6 => ("Marquise", "Marquise"),
            7 => ("Count", "Count"),
            8 => ("Countess", "Countess"),
            9 => ("Baron", "Baron"),
            10 => ("Baroness", "Baroness"),
            12 => ("Lady", "Lady"),
            _ => ("Lord", "Lord"),
        };
        self.unity.and_then(|unity| unity.string(key)).unwrap_or(fallback).to_string()
    }
}

/// One of `words`, drawn as `DFRandom.random_range(0, words.Length)` draws it.
fn pick(random: &mut DaggerfallRandom, words: &[&str]) -> Option<String> {
    let index = random.range(0, words.len() as i32);
    words.get(index as usize).map(|word| word.to_string())
}

/// Hours each type of building opens and closes (`PlayerActivate.openHours`
/// and `closeHours`): open from the first hour up to, not including, the
/// second; 0 to 25 is always, 0 to 0 never.
const OPEN_HOURS: [u8; 25] = [7, 8, 9, 8, 0, 9, 10, 10, 9, 6, 9, 11, 9, 9, 0, 0, 10, 0, 6, 6, 6, 6, 6, 6, 0];
const CLOSE_HOURS: [u8; 25] =
    [22, 16, 19, 15, 25, 21, 19, 20, 18, 23, 23, 23, 20, 20, 25, 25, 16, 0, 18, 18, 18, 18, 18, 18, 25];

/// Building types (continued).
pub const HOUSE_1: u8 = 17;
pub const HOUSE_2: u8 = 18;
pub const HOUSE_4: u8 = 20;
pub const SHIP: u8 = 24;

/// Whether Daggerfall Unity counts `building_type` as a shop (`RMBLayout.IsShop`).
pub fn is_shop(building_type: u8) -> bool {
    matches!(
        building_type,
        ALCHEMIST
            | ARMORER
            | BOOKSELLER
            | CLOTHING_STORE
            | FURNITURE_STORE
            | GEM_STORE
            | GENERAL_STORE
            | PAWN_SHOP
            | WEAPON_SMITH
    )
}

/// When the player can walk into a building without picking its lock, as
/// Daggerfall Unity decides it (`PlayerActivate.BuildingIsUnlocked`). Any
/// building is open to its owner and, for a residence, while a quest sends
/// the player there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingAccess {
    /// Open from `open` up to `close` o'clock; for a shop, never on Sun's Rest.
    Hours { open: u8, close: u8, shop: bool },
    /// Open at all hours.
    Always,
    /// A guild hall: open from `open` up to `close`, and at any time to members ranked high enough.
    GuildHall { open: u8, close: u8 },
    /// A Thieves Guild or Dark Brotherhood house: open to members only.
    MembersOnly,
    /// A ship: open to its owner only.
    OwnerOnly,
    /// Never open: the player must pick the lock or force the door.
    Never,
}

/// When building type `building_type` of faction `faction_id` is open.
pub fn building_access(building_type: u8, faction_id: u16) -> BuildingAccess {
    let hours = |shop: bool| {
        let index = usize::from(building_type);
        match (OPEN_HOURS.get(index).copied(), CLOSE_HOURS.get(index).copied()) {
            (Some(0), Some(close)) if close >= 24 => BuildingAccess::Always,
            (Some(open), Some(close)) if open < close => BuildingAccess::Hours { open, close, shop },
            _ => BuildingAccess::Never,
        }
    };
    match building_type {
        GUILD_HALL => match hours(false) {
            BuildingAccess::Hours { open, close, .. } => BuildingAccess::GuildHall { open, close },
            other => other,
        },
        HOUSE_2 if faction_id != 0 => BuildingAccess::MembersOnly,
        HOUSE_1..=HOUSE_4 => hours(false),
        building_type if is_shop(building_type) => hours(true),
        building_type if building_type <= PALACE => hours(false),
        SHIP => BuildingAccess::OwnerOnly,
        _ => BuildingAccess::Never,
    }
}

impl BuildingAccess {
    /// For example "7:00 to 22:00, not on Sun's Rest".
    pub fn describe(self) -> String {
        match self {
            Self::Hours { open, close, shop: false } => format!("{open}:00 to {close}:00"),
            Self::Hours { open, close, shop: true } => format!("{open}:00 to {close}:00, not on Sun's Rest"),
            Self::Always => "At all hours".to_string(),
            Self::GuildHall { open, close } => {
                format!("{open}:00 to {close}:00; at any time to members ranked high enough")
            }
            Self::MembersOnly => "To the guild's members only".to_string(),
            Self::OwnerOnly => "To its owner only".to_string(),
            Self::Never => "Never: the lock must be picked or the door forced".to_string(),
        }
    }
}

/// Whether Daggerfall Unity takes the people out of a building of
/// `building_type` while it is closed (`DaggerfallInterior.AddPeople`):
/// shops, and every other type up to House 4 that keeps hours, except for
/// members of the guild a guild hall or a guild's house belongs to.
pub fn people_leave_when_closed(building_type: u8) -> bool {
    building_type <= HOUSE_4
        && matches!(
            building_access(building_type, 0),
            BuildingAccess::Hours { .. } | BuildingAccess::GuildHall { .. } | BuildingAccess::Never
        )
}

/// What a shop sells, as Daggerfall Unity stocks its shelves
/// (`DaggerfallLoot.StockShopShelf`, with `DaggerfallLootDataTables`): item
/// groups and how likely each item of a group is to be stocked, and what
/// it always or sometimes adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopStock {
    /// Item groups, most likely first, with their chance number: an item of
    /// the group no rarer than the shop's quality is stocked
    /// `chance × 5 × (21 − rarity) / 100` times in 100.
    pub groups: Vec<(&'static str, u8)>,
    /// What the shop adds besides.
    pub extras: &'static [&'static str],
}

/// The names of the item groups shops stock (`ItemGroups`), by number.
fn item_group_name(group: u8) -> Option<&'static str> {
    Some(match group {
        2 => "armor",
        3 => "weapons",
        4 => "magic items",
        6 => "men's clothing",
        7 => "books",
        9 => "odds and ends",
        10 => "religious items",
        11 => "maps",
        12 => "women's clothing",
        13 => "paintings",
        14 => "gems",
        15 | 16 => "plant ingredients",
        17..=19 => "creature ingredients",
        20 | 22 => "other ingredients",
        21 => "metal ingredients",
        23 => "transportation",
        25 => "jewellery",
        _ => return None,
    })
}

/// What a shop of `building_type` sells; `None` for a building that is no
/// shop. Daggerfall Unity has no stock for furniture stores: theirs is empty.
pub fn shop_stock(building_type: u8) -> Option<ShopStock> {
    // Item group and chance pairs (`DaggerfallLootDataTables.itemGroups…`).
    let (pairs, extras): (&[u8], &'static [&'static str]) = match building_type {
        ALCHEMIST => (
            &[
                0x0E, 0x1E, 0x0F, 0x32, 0x10, 0x32, 0x11, 0x1E, 0x12, 0x14, 0x13, 0x14, 0x14, 0x3C, 0x15,
                0x28, 0x16, 0x1E,
            ],
            &["a potion recipe, one time in four"],
        ),
        ARMORER => (&[0x02, 0x50, 0x03, 0x14], &[]),
        BOOKSELLER => (&[0x07, 0x28, 0x0B, 0x05], &[]),
        CLOTHING_STORE => (&[0x06, 0x32, 0x0C, 0x32], &[]),
        GEM_STORE => (&[0x0E, 0x28, 0x19, 0x32], &[]),
        GENERAL_STORE => (
            &[0x03, 0x14, 0x06, 0x0A, 0x07, 0x0A, 0x09, 0x32, 0x17, 0x00, 0x0C, 0x0A, 0x04, 0x00],
            &["a horse", "a small cart"],
        ),
        PAWN_SHOP => (
            &[
                0x02, 0x0A, 0x03, 0x0A, 0x04, 0x0A, 0x07, 0x0A, 0x09, 0x14, 0x0D, 0x05, 0x0E, 0x0A, 0x19,
                0x0A, 0x0A, 0x0A,
            ],
            &[],
        ),
        WEAPON_SMITH => (&[0x02, 0x1E, 0x03, 0x46], &[]),
        FURNITURE_STORE => (&[], &[]),
        _ => return None,
    };
    // Groups of the same name keep their best chance; groups of chance 0 are never stocked.
    let mut groups: Vec<(&'static str, u8)> = Vec::new();
    for &[group, chance] in pairs.as_chunks::<2>().0 {
        let Some(name) = item_group_name(group) else { continue };
        if chance == 0 {
            continue;
        }
        match groups.iter_mut().find(|(known, _)| *known == name) {
            Some((_, best)) => *best = (*best).max(chance),
            None => groups.push((name, chance)),
        }
    }
    groups.sort_by_key(|&(_, chance)| std::cmp::Reverse(chance));
    Some(ShopStock { groups, extras })
}

impl ShopStock {
    /// For example "Armor, weapons", or what it adds; "Nothing" for none.
    pub fn describe(&self) -> String {
        let parts: Vec<&str> =
            self.groups.iter().map(|(name, _)| *name).chain(self.extras.iter().copied()).collect();
        let text = match parts.is_empty() {
            true => "nothing: Daggerfall Unity has no stock for this kind of shop".to_string(),
            false => parts.join(", "),
        };
        let mut characters = text.chars();
        characters.next().map(|first| first.to_uppercase().chain(characters).collect()).unwrap_or_default()
    }
}

/// What Daggerfall Unity makes of a piece of furniture inside a building
/// (`DaggerfallInterior.AddFurnitureAction`, and its ladders).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FurnitureUse {
    /// A shop's shelf, holding its wares.
    ShopShelf,
    /// A bookshelf to read from (in libraries, guild halls and temples).
    Bookshelf,
    /// A shelf with nothing on it (outside shops, libraries, guild halls and temples).
    EmptyShelf,
    /// A chest, wardrobe or drawer the player can search.
    Container,
    /// A ladder the player can climb.
    Ladder,
}

impl FurnitureUse {
    pub fn name(self) -> &'static str {
        match self {
            Self::ShopShelf => "Shop shelf",
            Self::Bookshelf => "Bookshelf",
            Self::EmptyShelf => "Empty shelf",
            Self::Container => "Container",
            Self::Ladder => "Ladder",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ShopShelf => "Holds the shop's wares, to buy",
            Self::Bookshelf => "Books the player can read",
            Self::EmptyShelf => {
                "Nothing: only shops' shelves hold wares, and only libraries', guild halls' and temples' books"
            }
            Self::Container => "A chest, wardrobe or drawer the player can search",
            Self::Ladder => "The player can climb it",
        }
    }
}

/// The first model of the furniture groups.
const FURNITURE_MODEL_BASE: u32 = 41000;
/// Models that are shelves, less [`FURNITURE_MODEL_BASE`].
const SHELF_MODELS: [u32; 27] = [
    5, 6, 11, 12, 13, 14, 15, 16, 17, 18, 19, 26, 28, 29, 31, 35, 36, 37, 40, 41, 42, 44, 46, 47, 48, 49, 808,
];
/// Models that are containers, less [`FURNITURE_MODEL_BASE`], besides every model of group 418.
const CONTAINER_MODELS: [u32; 13] = [3, 4, 7, 8, 27, 32, 33, 34, 35, 37, 38, 50, 51];
/// The group of models that are all containers (41800 to 41899).
const CONTAINER_GROUP: u32 = 418;
/// The model of ladders.
pub const LADDER_MODEL_ID: u32 = 41409;

/// What Daggerfall Unity makes of prop `model_id` in a building of
/// `building_type`, if anything. Only props count: the caller checks.
pub fn furniture_use(model_id: u32, building_type: u8) -> Option<FurnitureUse> {
    if model_id == LADDER_MODEL_ID {
        return Some(FurnitureUse::Ladder);
    }
    let index = model_id.checked_sub(FURNITURE_MODEL_BASE)?;
    if SHELF_MODELS.contains(&index) {
        return Some(match building_type {
            building_type if is_shop(building_type) => FurnitureUse::ShopShelf,
            LIBRARY | GUILD_HALL | TEMPLE => FurnitureUse::Bookshelf,
            _ => FurnitureUse::EmptyShelf,
        });
    }
    (model_id / 100 == CONTAINER_GROUP || CONTAINER_MODELS.contains(&index))
        .then_some(FurnitureUse::Container)
}

/// Padding added around each door's faces, in world units, so the box shows round a flat door.
const DOOR_BOX_PADDING: f32 = 0.08;

/// The box round each door of `mesh` of kind `kind` (its faces drawn with
/// that kind of door texture: into a building, or into a dungeon), in the
/// model's own space: minimum and maximum corners. Faces touching each other
/// make one door.
pub fn door_boxes(mesh: &ModelMesh, kind: DoorKind) -> Vec<(Vec3, Vec3)> {
    let mut triangles: Vec<[Vec3; 3]> = Vec::new();
    for submesh in &mesh.submeshes {
        if DoorKind::from_texture(submesh.texture) != Some(kind) {
            continue;
        }
        for triangle in submesh.indices.as_chunks::<3>().0 {
            let corner = |index: u32| submesh.vertices[index as usize].position;
            triangles.push([corner(triangle[0]), corner(triangle[1]), corner(triangle[2])]);
        }
    }
    // Join triangles sharing a corner into doors.
    let mut door_of: Vec<usize> = (0..triangles.len()).collect();
    fn root(door_of: &mut [usize], mut index: usize) -> usize {
        while door_of[index] != index {
            door_of[index] = door_of[door_of[index]];
            index = door_of[index];
        }
        index
    }
    let shares_corner = |first: &[Vec3; 3], second: &[Vec3; 3]| {
        first.iter().any(|a| second.iter().any(|b| a.distance_squared(*b) < 1e-6))
    };
    for first in 0..triangles.len() {
        for second in first + 1..triangles.len() {
            if shares_corner(&triangles[first], &triangles[second]) {
                let (a, b) = (root(&mut door_of, first), root(&mut door_of, second));
                door_of[a] = b;
            }
        }
    }
    let mut boxes: Vec<(usize, Vec3, Vec3)> = Vec::new();
    for (index, triangle) in triangles.iter().enumerate() {
        let door = root(&mut door_of, index);
        let (minimum, maximum) =
            triangle.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(low, high), point| {
                (low.min(*point), high.max(*point))
            });
        match boxes.iter_mut().find(|(known, ..)| *known == door) {
            Some((_, low, high)) => {
                *low = low.min(minimum);
                *high = high.max(maximum);
            }
            None => boxes.push((door, minimum, maximum)),
        }
    }
    boxes
        .into_iter()
        .map(|(_, minimum, maximum)| {
            (minimum - Vec3::splat(DOOR_BOX_PADDING), maximum + Vec3::splat(DOOR_BOX_PADDING))
        })
        .collect()
}

/// An exit inside a building: faces of one of its models drawn with
/// building door textures, which take the player back outdoors, out of the
/// building's door nearest to them (the static doors of Daggerfall Unity's
/// `DaggerfallInterior`). The doors that open and close inside never lead out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteriorExit {
    /// The scene object (a model) the exit is part of.
    pub object: usize,
    /// Its box in the model's own space: minimum and maximum corners.
    pub minimum: Vec3,
    pub maximum: Vec3,
    /// Where the model stands: from its own space to the inside's.
    pub matrix: Mat4,
}

impl InteriorExit {
    /// Its box's corners in the inside's space, as [`box_corners`] orders them.
    pub fn corners(&self) -> [Vec3; 8] {
        box_corners(self.minimum, self.maximum).map(|corner| self.matrix.transform_point3(corner))
    }

    /// Its middle, in the inside's space.
    pub fn center(&self) -> Vec3 {
        self.matrix.transform_point3((self.minimum + self.maximum) * 0.5)
    }

    /// How far along `ray` its box is hit, if it is.
    pub fn hit_distance(&self, ray: &Ray) -> Option<f32> {
        // Distances along the ray stay as they are in the model's space while
        // its direction is not made unit length again.
        let to_model = self.matrix.inverse();
        let origin = to_model.transform_point3(ray.origin);
        let direction = to_model.transform_vector3(ray.direction);
        let (mut near, mut far) = (0.0_f32, f32::INFINITY);
        for axis in 0..3 {
            let (start, step) = (origin[axis], direction[axis]);
            let (low, high) = (self.minimum[axis], self.maximum[axis]);
            if step.abs() < 1e-8 {
                if start < low || start > high {
                    return None;
                }
                continue;
            }
            let (first, second) = ((low - start) / step, (high - start) / step);
            near = near.max(first.min(second));
            far = far.min(first.max(second));
            if near > far {
                return None;
            }
        }
        Some(near)
    }
}

/// The exits inside a building, whose inside `scene` is as [`interior_layout`] lays it out.
pub fn interior_exits(scene: &Scene, library: &mut Arena2Library) -> Vec<InteriorExit> {
    let mut boxes_of_model: HashMap<u32, Vec<(Vec3, Vec3)>> = HashMap::new();
    let mut exits = Vec::new();
    for (index, object) in scene.objects.iter().enumerate() {
        let SceneObjectKind::Model { model_id, .. } = object.kind else { continue };
        if INTERIOR_DOOR_MODELS.contains(&model_id) {
            continue;
        }
        let boxes = boxes_of_model.entry(model_id).or_insert_with(|| {
            library.model(model_id).map(|mesh| door_boxes(&mesh, DoorKind::Building)).unwrap_or_default()
        });
        let matrix = object.model_matrix();
        exits.extend(boxes.iter().map(|&(minimum, maximum)| InteriorExit {
            object: index,
            minimum,
            maximum,
            matrix,
        }));
    }
    exits
}

/// A door on the outside of a building: faces of one of its models drawn
/// with building or dungeon door textures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutsideDoor {
    /// The model it belongs to, as an index into the block's models.
    pub model: usize,
    pub kind: DoorKind,
    /// Its box in the model's own space: minimum and maximum corners.
    pub minimum: Vec3,
    pub maximum: Vec3,
}

impl OutsideDoor {
    /// Its middle, in its model's space. Walking in by a door, Daggerfall
    /// Unity stands the inside in the space of the door's model
    /// (`PlayerEnterExit.TransitionInterior`), so this is where the door is
    /// inside too.
    pub fn center(&self) -> Vec3 {
        (self.minimum + self.maximum) * 0.5
    }
}

/// The doors on the outside of building `record` of `block`, into the building or into a dungeon.
pub fn outside_doors(block: &RmbBlock, record: usize, library: &mut Arena2Library) -> Vec<OutsideDoor> {
    let Some(building) = block.buildings.get(record) else { return Vec::new() };
    let mut doors = Vec::new();
    for index in building.models.clone() {
        let Some(model) = block.models.get(index) else { continue };
        let Some(mesh) = library.model(model.model_id) else { continue };
        for kind in [DoorKind::Building, DoorKind::DungeonEntrance] {
            doors.extend(door_boxes(&mesh, kind).into_iter().map(|(minimum, maximum)| OutsideDoor {
                model: index,
                kind,
                minimum,
                maximum,
            }));
        }
    }
    doors
}

/// The middles of the doors into building `record` of `block` (drawn with
/// building door textures), each in the space of the model it belongs to:
/// see [`OutsideDoor::center`].
pub fn entrance_centers(block: &RmbBlock, record: usize, library: &mut Arena2Library) -> Vec<Vec3> {
    outside_doors(block, record, library)
        .into_iter()
        .filter(|door| door.kind == DoorKind::Building)
        .map(|door| door.center())
        .collect()
}

/// How near an exit must be to a door outside to stand where it does, in metres.
const DOOR_MATCH_DISTANCE: f32 = 0.5;

/// The doors of building `record` of `block` drawn with dungeon door
/// textures that lead into the building's own inside as the data means them:
/// each stands where one of the inside's exits is (as Castle Daggerfall's
/// towers' doors do). Daggerfall Unity goes by the texture and takes the
/// player into the location's dungeon through them all the same.
pub fn dungeon_textured_entrances(
    block: &RmbBlock,
    record: usize,
    library: &mut Arena2Library,
) -> Vec<OutsideDoor> {
    if !block.buildings.get(record).is_some_and(|building| building.has_interior()) {
        return Vec::new();
    }
    let doors: Vec<OutsideDoor> = outside_doors(block, record, library)
        .into_iter()
        .filter(|door| door.kind == DoorKind::DungeonEntrance)
        .collect();
    if doors.is_empty() {
        return doors;
    }
    let layout = interior_layout(block, record, library);
    let exits: Vec<Vec3> = interior_exits(&layout.scene, library).iter().map(InteriorExit::center).collect();
    doors
        .into_iter()
        .filter(|door| exits.iter().any(|exit| exit.distance(door.center()) < DOOR_MATCH_DISTANCE))
        .collect()
}

/// Records of archive 199 Daggerfall Unity lands players by inside: rest and enter markers.
const REST_MARKER_RECORD: u8 = 4;
const ENTER_MARKER_RECORD: u8 = 8;

/// Where the enter and rest markers of `interior` are, in the inside's space.
pub fn landing_markers(interior: &RmbInterior) -> Vec<Vec3> {
    interior
        .flats
        .iter()
        .filter(|flat| {
            flat.texture.archive == MARKER_TEXTURE_ARCHIVE
                && matches!(flat.texture.record, REST_MARKER_RECORD | ENTER_MARKER_RECORD)
        })
        .map(|flat| native_to_world(flat.native_position))
        .collect()
}

/// The exit the player lands in front of on walking in by the door whose
/// middle is `door_center` (see [`entrance_centers`]), as Daggerfall Unity
/// picks it (`PlayerEnterExit.TransitionInterior`): the exit nearest to the
/// enter or rest marker nearest to the door, or nearest to the door itself
/// without one. `None` without exits: the player then lands on the marker.
pub fn arrival_exit(door_center: Vec3, landing_markers: &[Vec3], exits: &[InteriorExit]) -> Option<usize> {
    let nearer =
        |to: Vec3| move |a: &Vec3, b: &Vec3| a.distance_squared(to).total_cmp(&b.distance_squared(to));
    let from = landing_markers.iter().copied().min_by(nearer(door_center)).unwrap_or(door_center);
    let centers: Vec<Vec3> = exits.iter().map(InteriorExit::center).collect();
    (0..centers.len()).min_by(|&a, &b| nearer(from)(&centers[a], &centers[b]))
}

/// The model Daggerfall Unity leaves out of some interiors, where it stands
/// over the stairs and traps the player upstairs (`DaggerfallInterior.IsBadInteriorModel`).
const BAD_INTERIOR_MODEL_ID: u32 = 31000;
/// The cells and buildings it leaves that model out of. Daggerfall Unity
/// names the cells by their place in `BLOCKS.BSA`; these are their names
/// there (its comment calls the 160th "RENRGS00.RMB", which is GENRGS00.RMB).
const BAD_INTERIOR_MODEL_BUILDINGS: [(&str, &[usize]); 27] = [
    ("ARMRGL01.RMB", &[1]),
    ("GENRGM00.RMB", &[8, 9, 10, 11]),
    ("GENRGL01.RMB", &[2, 3, 4]),
    ("TVRNGL04.RMB", &[2, 5, 6]),
    ("FIGHGL00.RMB", &[1]),
    ("RESIGL00.RMB", &[0, 1, 2]),
    ("RESIGL04.RMB", &[0, 1, 2, 3]),
    ("RESIGL03.RMB", &[0, 1]),
    ("RESIGM01.RMB", &[6, 7]),
    ("GENRGS00.RMB", &[0]),
    ("TVRNGL02.RMB", &[5, 8]),
    ("GENRBM00.RMB", &[2, 3, 4, 5]),
    ("LIBRBL00.RMB", &[2]),
    ("CUSTGA07.RMB", &[1, 2, 12]),
    ("CUSTGA08.RMB", &[0, 5, 6]),
    ("CUSTGA10.RMB", &[0]),
    ("CUSTGA11.RMB", &[10]),
    ("CUSTGA13.RMB", &[5]),
    ("CUSTGA34.RMB", &[12]),
    ("CUSTGA32.RMB", &[10, 12]),
    ("CUSTGA29.RMB", &[12, 13]),
    ("RESIBL02.RMB", &[7, 8]),
    ("RESIBM01.RMB", &[6, 7]),
    ("RESIGL02.RMB", &[7, 8]),
    ("CUSTAA02.RMB", &[0, 1, 2, 3]),
    ("ARMRBL01.RMB", &[1]),
    ("CUSTGA05.RMB", &[1, 2, 3, 4]),
];

/// Whether Daggerfall Unity leaves model `model_id` out of building `record` of cell `block_name`.
pub fn is_left_out_of_interior(block_name: &str, record: usize, model_id: u32) -> bool {
    model_id == BAD_INTERIOR_MODEL_ID
        && BAD_INTERIOR_MODEL_BUILDINGS
            .iter()
            .any(|(block, records)| block.eq_ignore_ascii_case(block_name) && records.contains(&record))
}

/// The first of the five models of the doors inside buildings.
const INTERIOR_DOOR_MODEL_BASE: u32 = 9000;
/// The five models of the doors inside buildings.
const INTERIOR_DOOR_MODELS: Range<u32> = INTERIOR_DOOR_MODEL_BASE..INTERIOR_DOOR_MODEL_BASE + 5;

/// What a marker inside a building is for, by its record in archive 199.
pub fn interior_marker_name(record: u8) -> &'static str {
    match record {
        4 => "Rest marker",
        8 => "Enter marker",
        11 => "Quest marker",
        18 => "Quest item marker",
        19 => "Treasure marker",
        20 => "Random flat marker",
        21 => "Ladder bottom",
        22 => "Ladder top",
        _ => "Unused marker",
    }
}

/// What the game does with a marker inside a building, by its record in archive 199.
pub fn interior_marker_use(record: u8) -> &'static str {
    match record {
        4 => "Where the player can rest",
        8 => "Where the player appears on walking in",
        11 => "Where quests put the people and enemies they place in the building",
        18 => "Where quests put the items they place in the building",
        19 => "Treasure: in taverns and Thieves Guild or Dark Brotherhood buildings, a pile of loot",
        21 | 22 => "Where a ladder is climbed",
        20 => "Its image is labelled \"random flat\"; Daggerfall Unity has no use for it",
        _ => "Daggerfall Unity has no use for this marker",
    }
}

/// The inside of a building, laid out.
pub struct InteriorLayout {
    pub scene: Scene,
    /// Whether its rooms and doors are stored by their floor rather than
    /// their middle, and were raised to fit: see [`interior_layout`].
    pub raised: bool,
    /// The objects of the scene that are props (furniture and clutter).
    pub props: Vec<usize>,
}

/// Half a storey: the height of the middle of a room above its floor, in metres (63 native units).
const HALF_STOREY: f32 = 63.0 * WORLD_SCALE;
/// How near to half a storey under the doors outside the exits must be for
/// an inside's rooms to count as stored by their floor, in metres.
const HALF_STOREY_TOLERANCE: f32 = 0.2;

/// The inside of building `record` of `block` as Daggerfall Unity lays it
/// out (`DaggerfallInterior`): its models (props standing on their stored
/// bottom), its doors, flats and markers, and its people. Every object is in
/// group 0; people keep their faction in their flat data.
///
/// A few buildings (Castle Daggerfall's towers among them) store their rooms
/// and doors by their floor, as flats and props are stored, rather than by
/// their middle as every other building does. Laid out as stored, they sit
/// half a storey too low: the furniture floats above the floors and every
/// exit is under the door outside it. Daggerfall Unity lays them out so;
/// here they are told by their exits and raised to fit.
pub fn interior_layout(block: &RmbBlock, record: usize, library: &mut Arena2Library) -> InteriorLayout {
    let Some(building) = block.buildings.get(record) else {
        let scene = Scene { block_name: block.name.clone(), objects: Vec::new() };
        return InteriorLayout { scene, raised: false, props: Vec::new() };
    };
    // Props come first among the objects, in the order of the models kept.
    let props: Vec<usize> = building
        .interior
        .models
        .iter()
        .filter(|model| !is_left_out_of_interior(&block.name, record, model.model_id))
        .enumerate()
        .filter(|(_, model)| model.is_prop)
        .map(|(index, _)| index)
        .collect();
    let stored = lay_out_interior(&block.name, record, &building.interior, false, library);
    let exits: Vec<Vec3> = interior_exits(&stored, library).iter().map(InteriorExit::center).collect();
    let doors = outside_doors(block, record, library);
    let half_storey_under = |door: &OutsideDoor| {
        let center = door.center();
        exits
            .iter()
            .min_by(|a, b| a.distance_squared(center).total_cmp(&b.distance_squared(center)))
            .is_some_and(|exit| (center.y - exit.y - HALF_STOREY).abs() < HALF_STOREY_TOLERANCE)
    };
    if doors.is_empty() || !doors.iter().all(half_storey_under) {
        return InteriorLayout { scene: stored, raised: false, props };
    }
    let scene = lay_out_interior(&block.name, record, &building.interior, true, library);
    InteriorLayout { scene, raised: true, props }
}

/// Lays out `interior` (see [`interior_layout`]), with its rooms and doors
/// standing on their stored height when `by_floor`, else centred on it.
fn lay_out_interior(
    block_name: &str,
    record: usize,
    interior: &RmbInterior,
    by_floor: bool,
    library: &mut Arena2Library,
) -> Scene {
    let mut objects: Vec<SceneObject> = Vec::new();
    let mut add = |name: String, kind: SceneObjectKind, position: Vec3, yaw_degrees: f32| {
        objects.push(SceneObject {
            id: objects.len() as u32 + 1,
            name,
            group_index: 0,
            kind,
            position,
            rotation_degrees: Vec3::new(0.0, yaw_degrees, 0.0),
            visible: true,
            next: None,
            scale: None,
            prefab: None,
            folder: None,
        });
    };
    for model in &interior.models {
        if is_left_out_of_interior(block_name, record, model.model_id) {
            continue;
        }
        let native = model.native_position;
        let bottom = library.model(model.model_id).map_or(0.0, |mesh| mesh.bounds_min.y);
        let position = match (model.is_prop, by_floor) {
            // A prop's stored height is its bottom's, upwards.
            (true, _) => {
                Vec3::new(native.x as f32, native.y as f32, native.z as f32) * WORLD_SCALE - Vec3::Y * bottom
            }
            (false, false) => native_to_world(native),
            (false, true) => native_to_world(native) - Vec3::Y * bottom,
        };
        add(
            format!("Model {}", model.model_id),
            SceneObjectKind::Model { model_id: model.model_id, data: ModelData::default() },
            position,
            model.yaw_degrees,
        );
    }
    for door in &interior.doors {
        let model_id = INTERIOR_DOOR_MODEL_BASE + u32::from(door.model_index % 5);
        let raise = match by_floor {
            true => library.model(model_id).map_or(0.0, |mesh| -mesh.bounds_min.y),
            false => 0.0,
        };
        add(
            "Door".to_string(),
            SceneObjectKind::Model { model_id, data: ModelData::default() },
            native_to_world(door.native_position) + Vec3::Y * raise,
            door.yaw_degrees,
        );
    }
    let mut add_flat =
        |flat: &InteriorFlat, name: String, add: &mut dyn FnMut(String, SceneObjectKind, Vec3, f32)| {
            // Flats stand on their position; the renderer centres them on it.
            let height = library.billboard_world_size(flat.texture).map_or(0.0, |size| size.y);
            let data =
                FlatData { flags: u16::from(flat.flags), faction_or_mobile_id: flat.faction_id, action: 0 };
            add(
                name,
                SceneObjectKind::Flat { texture: flat.texture, data },
                native_to_world(flat.native_position) + Vec3::Y * (height / 2.0),
                0.0,
            );
        };
    for flat in &interior.flats {
        let name = match flat.texture.archive {
            MARKER_TEXTURE_ARCHIVE => interior_marker_name(flat.texture.record).to_string(),
            _ => format!("Flat {}.{}", flat.texture.archive, flat.texture.record),
        };
        add_flat(flat, name, &mut add);
    }
    for person in &interior.people {
        add_flat(person, format!("Person {}.{}", person.texture.archive, person.texture.record), &mut add);
    }
    Scene { block_name: block_name.to_string(), objects }
}

/// Counts of what is inside a building, for its properties.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InteriorSummary {
    pub models: usize,
    pub flats: usize,
    pub people: usize,
    pub doors: usize,
    pub spawn_points: usize,
    pub enter_markers: usize,
    pub quest_markers: usize,
    pub quest_item_markers: usize,
    pub treasure_markers: usize,
}

impl InteriorSummary {
    pub fn of(interior: &RmbInterior) -> Self {
        let markers = |record: u8| {
            interior
                .flats
                .iter()
                .filter(|flat| flat.texture == TextureKey::new(MARKER_TEXTURE_ARCHIVE, record))
                .count()
        };
        Self {
            models: interior.models.len(),
            flats: interior
                .flats
                .iter()
                .filter(|flat| flat.texture.archive != MARKER_TEXTURE_ARCHIVE)
                .count(),
            people: interior.people.len(),
            doors: interior.doors.len(),
            spawn_points: interior.spawn_points.len(),
            enter_markers: markers(8),
            quest_markers: markers(11),
            quest_item_markers: markers(18),
            treasure_markers: markers(19),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena2::{LocationExterior, RmbBuilding};

    fn building(building_type: u8, name_seed: u16) -> LocationBuilding {
        LocationBuilding { name_seed, faction_id: 0, sector: 0, building_type, quality: 5 }
    }

    #[test]
    fn hands_the_locations_buildings_to_its_cells_in_order() {
        let block = |types: &[u8]| RmbBlock {
            buildings: types
                .iter()
                .map(|&kind| RmbBuilding { data: building(kind, 0), ..RmbBuilding::default() })
                .collect(),
            ..RmbBlock::default()
        };
        let first = block(&[TAVERN, 17, GENERAL_STORE]);
        let mut second = block(&[TAVERN, TAVERN]);
        second.buildings[1].other_name = RAVEN_BUILDING_NAME.to_string();
        let location = LocationDefinition {
            region_index: 17,
            region_name: "Daggerfall",
            name: "Test".to_string(),
            location_type: None,
            map_pixel: crate::arena2::MapPixel::new(0, 0),
            location_id: 0,
            index_in_region: 0,
            map_id: 0,
            exterior: LocationExterior {
                buildings: vec![
                    building(GENERAL_STORE, 7),
                    building(TAVERN, 11),
                    building(TAVERN, 12),
                    building(17, 99),
                ],
                ..LocationExterior::default()
            },
            has_dungeon: false,
            discovered: true,
        };
        let buildings = settlement_buildings(&location, &[Some(&first), None, Some(&second)]);
        assert_eq!(buildings[0].iter().map(|building| building.name_seed).collect::<Vec<_>>(), [11, 0, 7]);
        assert!(buildings[1].is_empty());
        assert_eq!(buildings[2][0].name_seed, 12);
        // The list has no third tavern; the Raven building is the order's guild hall.
        assert_eq!((buildings[2][1].name_seed, buildings[2][1].building_type), (0, GUILD_HALL));
        assert_eq!(buildings[2][1].faction_id, RAVEN_FACTION_ID);
    }

    #[test]
    fn leaves_out_the_models_daggerfall_unity_leaves_out() {
        assert!(is_left_out_of_interior("genrgm00.rmb", 9, 31000));
        assert!(!is_left_out_of_interior("GENRGM00.RMB", 7, 31000));
        assert!(!is_left_out_of_interior("GENRGM00.RMB", 9, 31001));
        // Daggerfall Unity's 160th cell, which its comment misnames RENRGS00.RMB.
        assert!(is_left_out_of_interior("GENRGS00.RMB", 0, 31000));
        assert!(is_left_out_of_interior("CUSTGA05.RMB", 4, 31000));
    }

    #[test]
    fn opening_hours_follow_daggerfall_unitys_rules() {
        assert_eq!(building_access(ALCHEMIST, 0), BuildingAccess::Hours { open: 7, close: 22, shop: true });
        assert_eq!(building_access(BANK, 0), BuildingAccess::Hours { open: 8, close: 15, shop: false });
        assert_eq!(building_access(TAVERN, 0), BuildingAccess::Always);
        assert_eq!(building_access(GUILD_HALL, 40), BuildingAccess::GuildHall { open: 11, close: 23 });
        assert_eq!(building_access(HOUSE_1, 0), BuildingAccess::Never, "House 1 opens from 0 to 0");
        assert_eq!(building_access(HOUSE_2, 0), BuildingAccess::Hours { open: 6, close: 18, shop: false });
        assert_eq!(building_access(HOUSE_2, 42), BuildingAccess::MembersOnly);
        assert_eq!(building_access(22, 0), BuildingAccess::Never, "House 6");
        assert_eq!(building_access(SHIP, 0), BuildingAccess::OwnerOnly);
        assert!(people_leave_when_closed(ALCHEMIST) && people_leave_when_closed(PALACE));
        assert!(!people_leave_when_closed(TAVERN) && !people_leave_when_closed(22));
    }

    #[test]
    fn shops_sell_by_their_type() {
        let armorer = shop_stock(ARMORER).unwrap();
        assert_eq!(armorer.groups, [("armor", 80), ("weapons", 20)]);
        assert_eq!(armorer.describe(), "Armor, weapons");
        // Two plant ingredient groups make one; a general store's magic items are never stocked.
        assert!(
            shop_stock(ALCHEMIST)
                .unwrap()
                .groups
                .iter()
                .filter(|(name, _)| *name == "plant ingredients")
                .count()
                == 1
        );
        let general = shop_stock(GENERAL_STORE).unwrap();
        assert!(general.groups.iter().all(|(name, _)| *name != "magic items"));
        assert!(general.describe().ends_with("a horse, a small cart"));
        assert!(shop_stock(FURNITURE_STORE).unwrap().describe().starts_with("Nothing"));
        assert!(shop_stock(TAVERN).is_none());
    }

    #[test]
    fn furniture_by_model_and_building() {
        assert_eq!(furniture_use(41005, ALCHEMIST), Some(FurnitureUse::ShopShelf));
        assert_eq!(furniture_use(41005, LIBRARY), Some(FurnitureUse::Bookshelf));
        assert_eq!(furniture_use(41005, TAVERN), Some(FurnitureUse::EmptyShelf));
        // 35 and 37 are shelves before they are containers.
        assert_eq!(furniture_use(41035, TAVERN), Some(FurnitureUse::EmptyShelf));
        assert_eq!(furniture_use(41003, TAVERN), Some(FurnitureUse::Container));
        assert_eq!(furniture_use(41815, TAVERN), Some(FurnitureUse::Container));
        assert_eq!(furniture_use(41808, ALCHEMIST), Some(FurnitureUse::ShopShelf));
        assert_eq!(furniture_use(LADDER_MODEL_ID, TAVERN), Some(FurnitureUse::Ladder));
        assert_eq!(furniture_use(41100, TAVERN), None);
        assert_eq!(furniture_use(31000, TAVERN), None);
    }

    /// An exit one metre square, in a model turned a quarter round and moved.
    fn exit_at(object: usize, position: Vec3) -> InteriorExit {
        InteriorExit {
            object,
            minimum: Vec3::new(-0.5, 0.0, -0.05),
            maximum: Vec3::new(0.5, 1.0, 0.05),
            matrix: Mat4::from_rotation_translation(
                glam::Quat::from_rotation_y(90f32.to_radians()),
                position,
            ),
        }
    }

    #[test]
    fn exits_are_hit_where_their_turned_box_is() {
        let exit = exit_at(0, Vec3::new(10.0, 0.0, 0.0));
        assert!((exit.center() - Vec3::new(10.0, 0.5, 0.0)).length() < 1e-5);
        // Turned a quarter round, the exit's width runs along z.
        let towards = |from: Vec3| Ray { origin: from, direction: (exit.center() - from).normalize() };
        let distance = exit.hit_distance(&towards(Vec3::new(0.0, 0.5, 0.0))).expect("hit head on");
        assert!((distance - 9.95).abs() < 1e-4, "{distance}");
        let beside = Ray { origin: Vec3::new(0.0, 0.5, 0.8), direction: Vec3::X };
        assert!(exit.hit_distance(&beside).is_none(), "beside the turned exit");
        let within = Ray { origin: Vec3::new(0.0, 0.5, 0.4), direction: Vec3::X };
        assert!(exit.hit_distance(&within).is_some());
        let away = Ray { origin: Vec3::new(0.0, 0.5, 0.0), direction: -Vec3::X };
        assert!(exit.hit_distance(&away).is_none(), "behind the ray");
    }

    #[test]
    fn walking_in_lands_at_the_exit_nearest_the_marker_nearest_the_door() {
        let exits = [exit_at(0, Vec3::new(0.0, 0.0, 0.0)), exit_at(1, Vec3::new(20.0, 0.0, 0.0))];
        let door = Vec3::new(1.0, 0.5, 0.0);
        // No marker: the exit nearest the door.
        assert_eq!(arrival_exit(door, &[], &exits), Some(0));
        // The marker nearest the door is by the far exit.
        let markers = [Vec3::new(18.0, 0.0, 0.0), Vec3::new(-30.0, 0.0, 0.0)];
        assert_eq!(arrival_exit(door, &markers, &exits), Some(1));
        assert_eq!(arrival_exit(door, &markers, &[]), None);
    }
}
