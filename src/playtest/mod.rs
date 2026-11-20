//! Testing what the editor shows in Daggerfall Unity: a dungeon cell, a
//! dungeon, a settlement or an outdoor cell, with the player put at its
//! entrance and no-clip on, to look round as in the game.
//!
//! Nothing of the user's own Daggerfall Unity changes. Tests run in a copy
//! of it the program keeps ([`game_copy`]), in Daggerfall Unity's portable
//! mode and without the user's mods. A small mod of the program's own
//! ([`script`]) starts a new game there as soon as the title screen is up,
//! puts the player where the test's plan says ([`Arrival`]) and turns
//! no-clip on.
//!
//! What is tested is written into the copy as loose world data, as an
//! export of loose files is ([`crate::export`]): the game's own dungeons
//! and settlements need none where they are, a dungeon of the project where
//! it is placed is exported as it would be, and anything tested on its own
//! becomes a new location ([`json::Location`]) on an empty spot of the world
//! map ([`places::empty_map_pixel`]) whose land the test mod makes flat and
//! bare. A dungeon cell on its own is a dungeon of that one cell, of the kind
//! of the first of Daggerfall's dungeons that uses it; if it has nowhere to
//! put the player, a start marker is added for the test.
//!
//! Loose world data cannot carry the user's imported models, a dungeon's
//! own music or sky, or objects' sizes. A test with any of these also gets
//! a small mod of them, built as an export's `.dfmod` is and kept for later
//! tests ([`content`]), so the test shows them as the game will.

pub mod content;
pub mod game_copy;
pub mod places;
pub mod script;

use std::path::PathBuf;
use std::process::Child;
use std::sync::mpsc;

use glam::{Vec2, Vec3};

use crate::arena2::{
    region_name, Arena2Library, DungeonDefinition, LocationDefinition, LocationExterior, LocationType,
    MapPixel, Season, RDB_BLOCK_SIDE,
};
use crate::encounters::DungeonType;
use crate::export::{json, ModExtras, ModFiles, ScaledCell};
use crate::music::DungeonSong;
use crate::project::{CellCategory, DungeonLocation, ItemId, Project};
use crate::scene::{
    FlatData, Scene, SceneObject, SceneObjectKind, ADDED_OBJECTS_GROUP, MARKER_TEXTURE_ARCHIVE,
};

/// The dungeon type a dungeon cell on its own gets when none of
/// Daggerfall's dungeons uses it: a human stronghold.
const DEFAULT_DUNGEON_TYPE: u8 = 2;
/// First block number given to the test's own cells (`N9999900.RDB` and on),
/// in the range exports use for new cells; the test copy has no other mods.
const FIRST_TEST_BLOCK_NUMBER: u32 = 9_999_900;

/// Where a test puts the player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Arrival {
    /// Inside the dungeon, at its enter marker (else its start marker), as
    /// the game puts a new character in Privateer's Hold.
    InsideDungeon,
    /// Outside, at the location's dungeon entrance, as on leaving it.
    DungeonEntrance,
    /// At the edge of the location, as travelling there does.
    Travel,
    /// At this spot of the location, in metres from its south-west corner
    /// (x to the east, y to the north), facing north.
    Spot(Vec2),
}

/// Whether something is tested on its own or where it is in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// On an empty spot of the map, on flat, bare land.
    OnItsOwn,
    /// Where it is on the world map.
    WhereItIs,
}

/// Everything one test needs, apart from the games' folders.
#[derive(Debug, Clone)]
pub struct TestPlan {
    /// What is tested, for messages: "Daggerfall on its own".
    pub title: String,
    pub map_pixel: MapPixel,
    pub arrival: Arrival,
    /// Whether the land is flat and bare (for something on its own).
    pub flat_land: bool,
    /// The season to start in (the middle of summer, or of winter); `None`
    /// for the game's own start date, in winter.
    pub season: Option<Season>,
    /// The world data files: name inside `WorldData`, and contents.
    pub world_data: Vec<(String, String)>,
    /// What only a `.dfmod` carries (imported models, a dungeon's own music
    /// and sky, scaled objects), as a mod to build and install with the
    /// test mod; `None` if there is none.
    pub content: Option<ModFiles>,
    /// What the test leaves out or changes, to tell the user.
    pub notes: Vec<String>,
}

impl TestPlan {
    /// The plan file the test mod reads: one `name=value` per line.
    pub fn plan_file(&self) -> String {
        let mut lines = vec![
            format!("# Daggerfall Dungeon Maker test: {}", self.title),
            format!("map_x={}", self.map_pixel.x),
            format!("map_y={}", self.map_pixel.y),
            format!("flat={}", self.flat_land),
        ];
        let arrival = match self.arrival {
            Arrival::InsideDungeon => "inside",
            Arrival::DungeonEntrance => "entrance",
            Arrival::Travel => "travel",
            Arrival::Spot(spot) => {
                lines.push(format!("spot_x={}", spot.x));
                lines.push(format!("spot_z={}", spot.y));
                "spot"
            }
        };
        lines.push(format!("arrival={arrival}"));
        if let Some(season) = self.season {
            lines.push(format!("season={}", season.name().to_lowercase()));
        }
        lines.join("\n") + "\n"
    }
}

/// A cell of a dungeon tested on its own.
#[derive(Debug, Clone)]
pub struct TestCell {
    pub grid_x: i32,
    pub grid_z: i32,
    pub is_start: bool,
    pub block: TestBlock,
}

/// What a tested cell is made of.
#[derive(Debug, Clone)]
pub enum TestBlock {
    /// One of Daggerfall's blocks, by name.
    Daggerfall(String),
    /// A cell's objects (a project cell, or one changed for the test), and its kind.
    Objects { category: CellCategory, objects: Vec<SceneObject> },
}

/// A dungeon to test on its own.
#[derive(Debug, Clone)]
pub struct DungeonOnItsOwn {
    pub name: String,
    pub dungeon_type: Option<DungeonType>,
    pub location_type: LocationType,
    /// What stands above it; a small dungeon's entrance if `None`.
    pub exterior: Option<LocationExterior>,
    pub cells: Vec<TestCell>,
    /// Where on the map to look for an empty spot near.
    pub near: Option<MapPixel>,
    /// The game's song it plays, if one is chosen.
    pub song: Option<DungeonSong>,
    /// Its own music file, and its sky's image, if it has them.
    pub custom_music: Option<PathBuf>,
    pub skybox: Option<PathBuf>,
}

/// A test of the dungeon cell `name` on its own, made of `objects`, of
/// kind `category`; `based_on` is the Daggerfall block it was copied from.
pub fn dungeon_cell(
    name: &str,
    objects: &[SceneObject],
    category: CellCategory,
    based_on: Option<&str>,
    library: &mut Arena2Library,
) -> Result<TestPlan, String> {
    // The first of Daggerfall's dungeons using the block gives the kind of dungeon and its entrance.
    let block_name = based_on.unwrap_or(name);
    let definitions = library.dungeons();
    let user = definitions.iter().find(|definition| {
        definition.blocks.iter().any(|block| block.block_name.eq_ignore_ascii_case(block_name))
    });
    let mut plan = dungeon_on_its_own(
        DungeonOnItsOwn {
            name: format!("Test of {}", name.trim_end_matches(".RDB")),
            dungeon_type: user
                .and_then(|definition| definition.dungeon_type)
                .or(DungeonType::from_raw(DEFAULT_DUNGEON_TYPE)),
            location_type: LocationType::SmallDungeon,
            exterior: user.and_then(|definition| definition.exterior.clone()),
            cells: vec![TestCell {
                grid_x: 0,
                grid_z: 0,
                is_start: true,
                block: TestBlock::Objects { category, objects: objects.to_vec() },
            }],
            near: user.and_then(|definition| definition.map_pixel),
            song: None,
            custom_music: None,
            skybox: None,
        },
        library,
    )?;
    plan.title = format!("{name} on its own");
    Ok(plan)
}

/// A test of the Daggerfall dungeon `definition`.
pub fn daggerfall_dungeon(
    definition: &DungeonDefinition,
    setting: Setting,
    library: &mut Arena2Library,
) -> Result<TestPlan, String> {
    match setting {
        Setting::WhereItIs => {
            let pixel = definition
                .map_pixel
                .ok_or_else(|| format!("Where {} is on the map is not known", definition.name))?;
            Ok(TestPlan {
                title: format!("{} where it is", definition.name),
                map_pixel: pixel,
                arrival: Arrival::DungeonEntrance,
                flat_land: false,
                season: None,
                world_data: Vec::new(),
                content: None,
                notes: Vec::new(),
            })
        }
        Setting::OnItsOwn => {
            let cells = definition
                .blocks
                .iter()
                .map(|block| TestCell {
                    grid_x: block.grid_x,
                    grid_z: block.grid_z,
                    is_start: block.is_starting_block,
                    block: TestBlock::Daggerfall(block.block_name.clone()),
                })
                .collect();
            let mut plan = dungeon_on_its_own(
                DungeonOnItsOwn {
                    name: definition.name.clone(),
                    dungeon_type: definition.dungeon_type,
                    location_type: definition.location_type.unwrap_or(LocationType::LargeDungeon),
                    exterior: definition.exterior.clone(),
                    cells,
                    near: definition.map_pixel,
                    song: song_of(definition),
                    custom_music: None,
                    skybox: None,
                },
                library,
            )?;
            plan.title = format!("{} on its own", definition.name);
            Ok(plan)
        }
    }
}

/// A test of the project dungeon `id` of `project`.
pub fn project_dungeon(
    project: &Project,
    id: ItemId,
    setting: Setting,
    library: &mut Arena2Library,
) -> Result<TestPlan, String> {
    let dungeon = project.dungeon(id).ok_or("The dungeon is no longer in the project")?;
    let name = match dungeon.name.trim() {
        "" => "Unnamed dungeon".to_string(),
        name => name.to_string(),
    };
    match setting {
        Setting::WhereItIs => {
            let pixel = dungeon.location.map_pixel.ok_or_else(|| {
                format!("{name} is not placed on the world map yet, so it can only be tested on its own")
            })?;
            // Exported as an export for a .dfmod would write it: the world
            // data loose, and the rest (models, music, sky, scaled objects)
            // in the test's content mod.
            let mod_files = crate::export::build_mod(
                project,
                library,
                &content::details(),
                "Dungeon Maker test content",
                &[id],
                true,
            )
            .map_err(|error| format!("{name} cannot be tested where it is yet:\n{error}"))?;
            let prefix = format!("{}/", crate::export::WORLD_DATA_FOLDER);
            let world_data = mod_files
                .files
                .iter()
                .filter_map(|file| {
                    Some((file.path.strip_prefix(&prefix)?.to_string(), file.contents.clone()))
                })
                .collect();
            let model_tags = library.model_tags();
            let notes = unscaled_notes(
                dungeon
                    .rooms
                    .iter()
                    .filter_map(|placement| project.room(placement.room?))
                    .map(|room| room.objects.as_slice()),
                &model_tags,
            );
            Ok(TestPlan {
                title: format!("{name} where it is"),
                map_pixel: pixel,
                arrival: Arrival::DungeonEntrance,
                flat_land: false,
                season: None,
                world_data,
                content: content::of_export(mod_files),
                notes,
            })
        }
        Setting::OnItsOwn => {
            let definitions = library.dungeons();
            let mut notes = Vec::new();
            let exterior = dungeon
                .location
                .entrance
                .as_ref()
                .and_then(|entrance| definitions.iter().find(|definition| entrance.is(definition)))
                .and_then(|definition| definition.exterior.clone());
            let mut cells = Vec::new();
            for placement in &dungeon.rooms {
                let block = match placement.room {
                    Some(room_id) => {
                        let room = project
                            .room(room_id)
                            .ok_or_else(|| format!("A cell of {name} is missing from the project"))?;
                        TestBlock::Objects { category: room.category, objects: room.objects.clone() }
                    }
                    None => TestBlock::Daggerfall(placement.block_name.clone()),
                };
                cells.push(TestCell {
                    grid_x: placement.grid_x,
                    grid_z: placement.grid_z,
                    is_start: placement.is_starting_block,
                    block,
                });
            }
            let mut plan = dungeon_on_its_own(
                DungeonOnItsOwn {
                    name: name.clone(),
                    dungeon_type: dungeon.dungeon_type,
                    location_type: dungeon.location.location_type,
                    exterior,
                    cells,
                    near: dungeon.location.map_pixel,
                    // The song chosen, else the one of the dungeon it replaces.
                    song: dungeon.location.music.or_else(|| {
                        let replaced = dungeon.location.replaces.as_ref()?;
                        song_of(definitions.iter().find(|definition| replaced.is(definition))?)
                    }),
                    custom_music: own_file(&dungeon.location, Extra::Music, &mut notes),
                    skybox: own_file(&dungeon.location, Extra::Sky, &mut notes),
                },
                library,
            )?;
            plan.title = format!("{name} on its own");
            plan.notes.splice(0..0, notes);
            Ok(plan)
        }
    }
}

/// A test of `dungeon` on its own: a new location holding it on an empty
/// spot of the map, with the player inside at its start.
pub fn dungeon_on_its_own(dungeon: DungeonOnItsOwn, library: &mut Arena2Library) -> Result<TestPlan, String> {
    let DungeonOnItsOwn {
        name,
        dungeon_type,
        location_type,
        exterior,
        mut cells,
        near,
        song,
        custom_music,
        skybox,
    } = dungeon;
    if cells.is_empty() {
        return Err(format!("{name} has no cells to test"));
    }
    // One start cell: the one marked, else the first.
    let start = cells.iter().position(|cell| cell.is_start).unwrap_or(0);
    for (index, cell) in cells.iter_mut().enumerate() {
        cell.is_start = index == start;
    }
    let mut notes = Vec::new();
    // The game needs somewhere in the start cell to put the player.
    if let Some(objects) = cell_objects(&cells[start].block, library) {
        if !has_arrival_marker(&objects) {
            let category = match &cells[start].block {
                TestBlock::Objects { category, .. } => *category,
                TestBlock::Daggerfall(name) => CellCategory::of_copy(Some(name.as_str())),
            };
            let mut objects = objects;
            add_start_marker(&mut objects);
            cells[start].block = TestBlock::Objects { category, objects };
            notes.push("its start cell had no start marker, so one was added for the test".to_string());
        }
    }

    let world = library.world_map().ok_or("MAPS.BSA was not found, so the world map cannot be read")?;
    let definitions = library.dungeons();
    let model_tags = library.model_tags();
    let near = near.unwrap_or(MapPixel::new(crate::arena2::MAP_WIDTH / 2, crate::arena2::MAP_HEIGHT / 2));
    let pixel =
        places::empty_map_pixel(&world, None, near).ok_or("There is no empty spot on the map to test in")?;
    let region_index = world.region_at(pixel).ok_or("The empty spot found is in the sea")?;
    let mut used_ids: std::collections::HashSet<u16> = library.location_ids().iter().copied().collect();
    let location_id =
        crate::export::new_location_id(pixel, &mut used_ids).ok_or("There are no free location ids")?;

    let mut world_data = Vec::new();
    let mut blocks = Vec::new();
    // The user's models the cells use, and each cell's scaled objects, for the content mod.
    let mut models = std::collections::BTreeSet::new();
    let mut scaled_cells: Vec<(String, i32, i32, Vec<SceneObject>)> = Vec::new();
    let mut missing_models = 0;
    for (index, cell) in cells.iter().enumerate() {
        let block_name = match &cell.block {
            TestBlock::Daggerfall(block_name) => block_name.clone(),
            TestBlock::Objects { category, objects } => {
                let block_name =
                    format!("{}{:07}.RDB", category.letter(), FIRST_TEST_BLOCK_NUMBER + index as u32);
                // As an export for a .dfmod: the scaled objects are left out
                // of the cell for the content mod's script to put in place.
                // Models no longer imported cannot be shown at all.
                let missing: std::collections::HashSet<usize> = (0..objects.len())
                    .filter(|&index| {
                        imported_model(&objects[index])
                            .is_some_and(|id| library.custom_model_files(id).is_none())
                    })
                    .collect();
                missing_models += missing.len();
                models.extend(
                    (0..objects.len())
                        .filter(|index| !missing.contains(index))
                        .filter_map(|index| imported_model(&objects[index])),
                );
                let scaled: Vec<usize> = crate::export::scaled_objects(objects, &model_tags)
                    .into_iter()
                    .filter(|index| !missing.contains(index))
                    .collect();
                if !scaled.is_empty() {
                    let scaled_objects = scaled.iter().map(|&index| objects[index].clone()).collect();
                    scaled_cells.push((block_name.clone(), cell.grid_x, cell.grid_z, scaled_objects));
                }
                let options = json::CellOptions {
                    left_out: missing.into_iter().chain(scaled).collect(),
                    positions: std::collections::HashMap::new(),
                };
                let position = crate::export::NEW_BLOCK_POSITION_BASE
                    + index as i64 * crate::export::NEW_BLOCK_POSITION_STRIDE;
                let block =
                    json::Block::from_cell_with(&block_name, position, objects, &model_tags, &options);
                world_data.push((format!("{block_name}.json"), crate::export::to_json(&block)));
                block_name
            }
        };
        blocks.push(json::DungeonBlock {
            x: cell.grid_x as i8,
            z: cell.grid_z as i8,
            is_starting_block: cell.is_start,
            block_name,
            water_level: 0,
            castle_block: false,
        });
    }
    if missing_models > 0 {
        notes.push(format!("{missing_models} model(s) are left out because they are no longer imported"));
    }
    notes.extend(unscaled_notes(
        cells.iter().filter_map(|cell| match &cell.block {
            TestBlock::Objects { objects, .. } => Some(objects.as_slice()),
            TestBlock::Daggerfall(_) => None,
        }),
        &model_tags,
    ));
    let exterior = exterior
        .or_else(|| small_entrance(&definitions))
        .ok_or("No dungeon entrance was found in the game data to put above the dungeon")?;
    let location = json::Location::new_dungeon(json::NewDungeon {
        name: &name,
        region_index,
        pixel,
        location_type,
        dungeon_type: dungeon_type.map_or(DEFAULT_DUNGEON_TYPE, DungeonType::raw),
        discovered: true,
        location_id,
        replaces: None,
        // The song it plays where it is, else one picked from its id as for any new dungeon.
        music_seed: song.map_or(u32::from(location_id), |song| {
            crate::music::seed_for_song(song, region_index, location_id)
        }),
        climate: world.climate_at(pixel),
        exterior: &exterior,
        blocks,
    });
    world_data.push((location_file_name(&name, region_index), crate::export::to_json(&location)));

    // What only a .dfmod carries, for this location, which its scripts know by its region and name.
    let content_name = content::file_name();
    let mut extras = ModExtras::default();
    extras.add_custom_models(models, library)?;
    if let Some(source) = &custom_music {
        extras.add_music(&content_name, region_index, &name, source);
    }
    if let Some(source) = &skybox {
        extras.add_sky(&content_name, region_index, &name, source);
    }
    let scaled: Vec<ScaledCell<'_>> = scaled_cells
        .iter()
        .map(|(block_name, grid_x, grid_z, objects)| ScaledCell {
            block_name,
            grid_x: *grid_x,
            grid_z: *grid_z,
            objects,
        })
        .collect();
    extras.add_scaled(region_index, &name, &scaled);
    Ok(TestPlan {
        title: name,
        map_pixel: pixel,
        arrival: Arrival::InsideDungeon,
        flat_land: true,
        season: None,
        world_data,
        content: content::of_extras(extras),
        notes,
    })
}

/// A test of the settlement `location`. On its own it is drawn in the
/// climate `climate` (a `CLIMATE.PAK` value), with the player at `spot`
/// (see [`Arrival::Spot`]); `dungeon` is the dungeon under it, if any.
pub fn settlement(
    location: &LocationDefinition,
    dungeon: Option<&DungeonDefinition>,
    setting: Setting,
    climate: u8,
    spot: Vec2,
    library: &mut Arena2Library,
) -> Result<TestPlan, String> {
    match setting {
        Setting::WhereItIs => Ok(TestPlan {
            title: format!("{} where it is", location.name),
            map_pixel: location.map_pixel,
            arrival: Arrival::Travel,
            flat_land: false,
            season: None,
            world_data: Vec::new(),
            content: None,
            notes: Vec::new(),
        }),
        Setting::OnItsOwn => {
            let blocks = dungeon
                .map(|dungeon| {
                    dungeon
                        .blocks
                        .iter()
                        .map(|block| json::DungeonBlock {
                            x: block.grid_x as i8,
                            z: block.grid_z as i8,
                            is_starting_block: block.is_starting_block,
                            block_name: block.block_name.clone(),
                            water_level: 0,
                            castle_block: false,
                        })
                        .collect()
                })
                .unwrap_or_default();
            exterior_on_its_own(ExteriorOnItsOwn {
                name: location.name.clone(),
                location_type: location.location_type.unwrap_or(LocationType::Hamlet),
                exterior: location.exterior.clone(),
                dungeon_type: dungeon.and_then(|dungeon| dungeon.dungeon_type),
                dungeon_blocks: blocks,
                climate,
                spot,
                near: location.map_pixel,
                library,
            })
        }
    }
}

/// A test of the outdoor cell `block_name` on its own, in the climate
/// `climate` (a `CLIMATE.PAK` value), with the player at `spot`.
pub fn outdoor_cell(
    block_name: &str,
    climate: u8,
    spot: Vec2,
    library: &mut Arena2Library,
) -> Result<TestPlan, String> {
    let block =
        library.rmb_block(block_name).map_err(|error| format!("Could not read {block_name}: {error}"))?;
    let exterior = LocationExterior {
        width: 1,
        height: 1,
        block_names: vec![block_name.to_string()],
        buildings: block.buildings.iter().map(|building| building.data).collect(),
        port_town: false,
    };
    let near = MapPixel::new(crate::arena2::MAP_WIDTH / 2, crate::arena2::MAP_HEIGHT / 2);
    exterior_on_its_own(ExteriorOnItsOwn {
        name: block_name.trim_end_matches(".RMB").to_string(),
        location_type: LocationType::Hamlet,
        exterior,
        dungeon_type: None,
        dungeon_blocks: Vec::new(),
        climate,
        spot,
        near,
        library,
    })
}

/// What [`exterior_on_its_own`] makes a location of.
struct ExteriorOnItsOwn<'library> {
    name: String,
    location_type: LocationType,
    exterior: LocationExterior,
    dungeon_type: Option<DungeonType>,
    dungeon_blocks: Vec<json::DungeonBlock>,
    climate: u8,
    spot: Vec2,
    near: MapPixel,
    library: &'library mut Arena2Library,
}

/// A test of an outdoor location on its own: a new location on an empty
/// spot of the map in its climate, on flat, bare land.
fn exterior_on_its_own(request: ExteriorOnItsOwn<'_>) -> Result<TestPlan, String> {
    let ExteriorOnItsOwn {
        name,
        location_type,
        exterior,
        dungeon_type,
        dungeon_blocks,
        climate,
        spot,
        near,
        library,
    } = request;
    let world = library.world_map().ok_or("MAPS.BSA was not found, so the world map cannot be read")?;
    let pixel = places::empty_map_pixel(&world, Some(climate), near)
        .or_else(|| places::empty_map_pixel(&world, None, near))
        .ok_or("There is no empty spot on the map to test in")?;
    let region_index = world.region_at(pixel).ok_or("The empty spot found is in the sea")?;
    let mut used_ids: std::collections::HashSet<u16> = library.location_ids().iter().copied().collect();
    let location_id =
        crate::export::new_location_id(pixel, &mut used_ids).ok_or("There are no free location ids")?;
    let mut notes = Vec::new();
    let pixel_climate = world.climate_at(pixel);
    if pixel_climate != Some(climate) {
        notes.push(format!(
            "no empty spot has the climate chosen, so it is in the climate of {}",
            region_name(region_index)
        ));
    }
    let location = json::Location::new_dungeon(json::NewDungeon {
        name: &name,
        region_index,
        pixel,
        location_type,
        dungeon_type: dungeon_type.map_or(u8::MAX, DungeonType::raw),
        discovered: true,
        location_id,
        replaces: None,
        music_seed: u32::from(location_id),
        climate: pixel_climate,
        exterior: &exterior,
        blocks: dungeon_blocks,
    });
    Ok(TestPlan {
        title: format!("{name} on its own"),
        map_pixel: pixel,
        arrival: Arrival::Spot(spot),
        flat_land: true,
        season: None,
        world_data: vec![(location_file_name(&name, region_index), crate::export::to_json(&location))],
        content: None,
        notes,
    })
}

/// The name of a new location's file, which Daggerfall Unity finds by its
/// start and its region at the end.
fn location_file_name(name: &str, region_index: usize) -> String {
    format!("locationnew-{}-{region_index}.json", crate::export::file_safe_name(name, "Test"))
}

/// The objects of the cell `block`, or `None` if it cannot be read.
fn cell_objects(block: &TestBlock, library: &Arena2Library) -> Option<Vec<SceneObject>> {
    match block {
        TestBlock::Objects { objects, .. } => Some(objects.clone()),
        TestBlock::Daggerfall(name) => {
            library.rdb_block(name).ok().map(|block| Scene::from_rdb_block(&block).objects)
        }
    }
}

/// Whether `objects` have a marker Daggerfall Unity can put the player at:
/// an enter marker, or else a start marker.
fn has_arrival_marker(objects: &[SceneObject]) -> bool {
    objects.iter().any(|object| {
        object.is_marker_record(crate::scene::ENTER_MARKER_RECORD)
            || object.is_marker_record(crate::scene::START_MARKER_RECORD)
    })
}

/// Adds a start marker to `objects` at the marker nearest the middle of
/// the cell (markers stand where the player can), else at the object
/// nearest it, else at the middle.
fn add_start_marker(objects: &mut Vec<SceneObject>) {
    let middle = Vec3::new(RDB_BLOCK_SIDE / 2.0, 0.0, RDB_BLOCK_SIDE / 2.0);
    let distance = |object: &&SceneObject| {
        Vec2::new(object.position.x - middle.x, object.position.z - middle.z).length_squared()
    };
    let is_marker = |object: &&SceneObject| matches!(object.kind, SceneObjectKind::Flat { texture, .. } if texture.archive == MARKER_TEXTURE_ARCHIVE);
    let nearest = |objects: &[SceneObject], wanted: &dyn Fn(&&SceneObject) -> bool| {
        objects
            .iter()
            .filter(wanted)
            .min_by(|first, second| distance(first).total_cmp(&distance(second)))
            .map(|object| object.position)
    };
    let position = nearest(objects, &is_marker).or_else(|| nearest(objects, &|_| true)).unwrap_or(middle);
    let id = objects.iter().map(|object| object.id).max().unwrap_or(0) + 1;
    objects.push(SceneObject {
        id,
        name: "Start marker (added for the test)".to_string(),
        group_index: ADDED_OBJECTS_GROUP,
        kind: SceneObjectKind::Flat {
            texture: crate::arena2::TextureKey {
                archive: MARKER_TEXTURE_ARCHIVE,
                record: crate::scene::START_MARKER_RECORD,
            },
            data: FlatData::default(),
        },
        position,
        rotation_degrees: Vec3::ZERO,
        visible: true,
        next: None,
        scale: None,
        prefab: None,
        folder: None,
    });
}

/// A dungeon entrance of the game to put above a dungeon that has none
/// chosen: the first one-cell entrance of a small dungeon.
fn small_entrance(definitions: &[DungeonDefinition]) -> Option<LocationExterior> {
    let entrances =
        || definitions.iter().filter_map(|definition| Some((definition, definition.exterior.as_ref()?)));
    entrances()
        .find(|(definition, exterior)| {
            definition.location_type == Some(LocationType::SmallDungeon) && exterior.block_names.len() == 1
        })
        .or_else(|| entrances().next())
        .map(|(_, exterior)| exterior.clone())
}

/// The id of the user's model `object` shows, if it shows one.
fn imported_model(object: &SceneObject) -> Option<u32> {
    match object.kind {
        SceneObjectKind::Model { model_id, .. } if crate::custom_models::is_custom_model(model_id) => {
            Some(model_id)
        }
        _ => None,
    }
}

/// The scaled objects of cells of `objects` that stay at their usual size,
/// as in an export (the game cannot scale objects with behaviour of their
/// own), as a note for the user.
fn unscaled_notes<'objects>(
    cells: impl Iterator<Item = &'objects [SceneObject]>,
    model_tags: &std::collections::HashMap<u32, String>,
) -> Vec<String> {
    let mut unscaled = 0;
    for objects in cells {
        let scaled = crate::export::scaled_objects(objects, model_tags);
        unscaled += (0..objects.len())
            .filter(|index| !scaled.contains(index))
            .filter(|&index| {
                let object = &objects[index];
                object.scale_or_one() != Vec3::ONE && !matches!(object.kind, SceneObjectKind::Light { .. })
            })
            .count();
    }
    match unscaled {
        0 => Vec::new(),
        _ => vec![format!(
            "{unscaled} scaled object(s) are at their usual size, as in an export (the game cannot scale \
             objects with actions, doors or loot)"
        )],
    }
}

/// Which file of its own a dungeon has.
#[derive(Debug, Clone, Copy)]
enum Extra {
    Music,
    Sky,
}

/// The music file or sky image (`extra`) of `location`, if it has one that
/// can be used; if it has one that cannot, a note in `notes` says why.
fn own_file(location: &DungeonLocation, extra: Extra, notes: &mut Vec<String>) -> Option<PathBuf> {
    let (wanted, file, problem) = match extra {
        Extra::Music => (
            location.custom_music,
            &location.custom_music_file,
            crate::export::custom_music_problem(location),
        ),
        Extra::Sky => (location.skybox, &location.skybox_file, crate::export::skybox_problem(location)),
    };
    if !wanted {
        return None;
    }
    match problem {
        Some(problem) => {
            let mut letters = problem.chars();
            let problem: String =
                letters.next().map(|first| first.to_lowercase().chain(letters).collect()).unwrap_or_default();
            notes.push(format!("{problem}, so the test goes without it"));
            None
        }
        None => Some(PathBuf::from(file.trim())),
    }
}

/// The song the game plays in Daggerfall's dungeon `definition`.
fn song_of(definition: &DungeonDefinition) -> Option<DungeonSong> {
    let classic = definition.classic.as_ref()?;
    Some(crate::music::song_for_seed(classic.dungeon_music_seed, definition.region_index))
}

/// What a test job needs besides its plan.
#[derive(Debug, Clone)]
pub struct TestRequest {
    pub plan: TestPlan,
    /// The user's Daggerfall Unity folder.
    pub install: PathBuf,
    /// The folder holding the `ARENA2` folder the editor uses, if it is one.
    pub daggerfall_folder: Option<PathBuf>,
    /// The Unity editor to build the test mod with, if it has to be built.
    pub unity_editor: Option<PathBuf>,
}

/// News from a running test job.
#[derive(Debug)]
pub enum TestEvent {
    /// What the job is doing now.
    Step(String),
    /// The game started, as this process.
    Started(Child),
    /// The job stopped before starting the game.
    Failed(String),
}

/// Readies the test copy and starts a test in it, on another thread.
pub struct TestJob {
    events: mpsc::Receiver<TestEvent>,
}

impl TestJob {
    pub fn start(request: TestRequest) -> Self {
        let (sender, events) = mpsc::channel();
        std::thread::spawn(move || {
            let step = |text: String| {
                let _ = sender.send(TestEvent::Step(text));
            };
            let event = match run(&request, &step) {
                Ok(child) => TestEvent::Started(child),
                Err(error) => TestEvent::Failed(error),
            };
            let _ = sender.send(event);
        });
        Self { events }
    }

    /// The events that arrived since the last call.
    pub fn poll(&self) -> Vec<TestEvent> {
        self.events.try_iter().collect()
    }
}

/// Readies the test copy for `request` and starts it.
fn run(request: &TestRequest, step: &dyn Fn(String)) -> Result<Child, String> {
    let copy = game_copy::GameCopy::prepare(&request.install, step)?;
    let unity_editor = request.unity_editor.as_deref();
    let mut bundles = vec![script::built_mod(&game_copy::mod_cache_folder(), unity_editor, step)?];
    if let Some(content) = &request.plan.content {
        bundles.push(content::built(&game_copy::content_cache_folder(), unity_editor, content, step)?);
    }
    step("Writing the test into the test copy of Daggerfall Unity…".to_string());
    copy.install_mods(&bundles)?;
    copy.write_settings(&request.install, request.daggerfall_folder.as_deref())?;
    copy.write_world_data(&request.plan.world_data)?;
    step("Starting Daggerfall Unity…".to_string());
    copy.launch(&request.plan.plan_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_name_the_place_and_the_arrival() {
        let plan = TestPlan {
            title: "CUSTAA05 on its own".to_string(),
            map_pixel: MapPixel::new(120, 340),
            arrival: Arrival::Spot(Vec2::new(51.5, 40.0)),
            flat_land: true,
            season: Some(Season::Summer),
            world_data: Vec::new(),
            content: None,
            notes: Vec::new(),
        };
        let text = plan.plan_file();
        for line in [
            "map_x=120",
            "map_y=340",
            "flat=true",
            "arrival=spot",
            "spot_x=51.5",
            "spot_z=40",
            "season=summer",
        ] {
            assert!(text.lines().any(|candidate| candidate == line), "{line} in {text}");
        }
    }

    #[test]
    fn a_start_marker_is_added_at_the_middle_marker() {
        let marker = |record: u8, x: f32| SceneObject {
            id: 3,
            name: String::new(),
            group_index: 0,
            kind: SceneObjectKind::Flat {
                texture: crate::arena2::TextureKey { archive: MARKER_TEXTURE_ARCHIVE, record },
                data: FlatData::default(),
            },
            position: Vec3::new(x, 1.0, RDB_BLOCK_SIDE / 2.0),
            rotation_degrees: Vec3::ZERO,
            visible: true,
            next: None,
            scale: None,
            prefab: None,
            folder: None,
        };
        let mut objects = vec![marker(crate::scene::RANDOM_ENEMY_MARKER_RECORD, 5.0), marker(11, 24.0)];
        assert!(!has_arrival_marker(&objects));
        add_start_marker(&mut objects);
        assert!(has_arrival_marker(&objects));
        let added = objects.last().unwrap();
        assert_eq!((added.id, added.position), (4, Vec3::new(24.0, 1.0, RDB_BLOCK_SIDE / 2.0)));
    }
}
