//! Shows a settlement or an outdoor cell from Daggerfall, read-only: a
//! location's RMB blocks on their grid (or one block alone), standing on their
//! ground, with the ground tiles, trees and building textures of the
//! location's climate, in summer or winter, as Daggerfall Unity draws them.
//!
//! Hovering a cell highlights it and its outline; double-clicking one opens it
//! on its own. Buildings that can be entered show a green box round each
//! entrance; double-clicking one opens its inside in a window of its own.
//! Clicking an object selects it: the properties panel then tells what it is
//! (a building's name, quality and faction in the settlement, a person's
//! faction, an arrival point). Otherwise it tells what the location is (its
//! region, type, place on the map, whether it is a port, its buildings and the
//! dungeon under it) and lists its cells; for an outdoor cell, it lists its
//! buildings and every settlement that uses it. Another climate or the
//! season can be chosen there, to see how the cell looks elsewhere, or with snow.

use std::collections::HashMap;

use eframe::{egui, egui_wgpu};
use glam::Vec3;

use super::interior_view::{
    building_key_label, faction_groups_row, faction_label, object_rows, shop_stock_row, BuildingPlace,
    InteriorKey, ObjectPlace,
};
use super::room_browser::show_room_name_field;
use super::viewport_tools::{
    aspect_ratio, handle_camera_input, paint_scene, size_in_pixels, EditorResources, ViewportProjection,
};
use crate::arena2::{
    building_type_name, rmb_block_kind, ruler_title_name, Arena2Library, Climate, DoorKind, GameReferences,
    LocationBuilding, LocationDefinition, RmbBlock, Season, TextureKey, AUTOMAP_SIDE,
    INDIVIDUAL_FACTION_KIND, PROVINCE_FACTION_KIND, RMB_BLOCK_SIDE,
};
use crate::buildings::{
    building_access, door_boxes, dungeon_textured_entrances, people_leave_when_closed, settlement_buildings,
    BuildingNamer, InteriorSummary, OutsideDoor,
};
use crate::camera::OrbitCamera;
use crate::custom_names::CustomNames;
use crate::exterior::ClimateTextures;
use crate::people::{building_key, outdoor_gender, person_details, PersonDetails, PersonPlace};
use crate::picking::{intersect_horizontal_plane, pick_object, PickOptions};
use crate::render::{EntranceBox, EntranceKind, FrameRequest, RoomOutline, ViewSettings, ViewportTarget};
use crate::scene::{
    box_corners, Scene, SceneObject, SceneObjectKind, MARKER_TEXTURE_ARCHIVE, START_MARKER_RECORD,
};

/// Height of the cell outlines, just above the ground.
const OUTLINE_HEIGHT: f32 = 0.03;
/// What the season list does.
const SEASON_HOVER: &str =
    "The game draws locations with snow in the winter months: the ground, the trees and \
     the buildings' snowy textures (deserts have no winter). This only changes the view.";
/// Colour of notes about the climate.
const NOTE_COLOR: egui::Color32 = egui::Color32::from_rgb(170, 200, 255);

/// What the view shows.
#[derive(Debug, Clone, PartialEq)]
pub enum ExteriorSubject {
    /// A location, by index in the library's location list.
    Settlement(usize),
    /// An outdoor cell on its own, by RMB block name.
    Cell(String),
}

/// Something the user asked to open from the view or its properties panel.
#[derive(Debug, Clone, PartialEq)]
pub enum ExteriorOpen {
    /// Open the outdoor cell with this RMB block name.
    Cell(String),
    /// Open the location at this index in the library's location list.
    Settlement(usize),
    /// Open the dungeon at this index in the library's dungeon list.
    Dungeon(usize),
    /// Open the dungeon under the location at this index in the library's location list.
    DungeonUnder(usize),
    /// Show the inside of a building in a window of its own, in `climate`.
    Interior { key: InteriorKey, place: Option<BuildingPlace>, climate: Climate },
}

/// What the properties panel needs besides the view itself.
pub struct ExteriorDetails<'details> {
    /// Every location of the game, which [`ExteriorSubject::Settlement`] and `used_in` index.
    pub locations: &'details [LocationDefinition],
    /// The dungeon under the settlement shown: its index in the library's
    /// dungeon list, and its name.
    pub dungeon: Option<(usize, &'details str)>,
    /// For an outdoor cell: the locations using it, as indices into `locations`.
    pub used_in: &'details [usize],
    /// For an outdoor cell: the dungeons under the locations using it, by
    /// index in the library's dungeon list, and their names.
    pub cell_dungeons: &'details [(usize, &'details str)],
    /// Factions and other game data objects refer to by number.
    pub references: &'details GameReferences,
}

/// The settlement shown, for naming its buildings.
#[derive(Debug, Clone)]
struct Settlement {
    index: usize,
    name: String,
    region_index: usize,
    region_name: &'static str,
    /// Its place in its region's list of locations.
    index_in_region: usize,
}

/// One building of the view's cells.
struct ExteriorBuilding {
    /// Its cell, as an index into the view's blocks, and its place in the cell.
    block: usize,
    record: usize,
    /// Its data as the game sees it here (in a settlement, from its list).
    data: LocationBuilding,
    /// What the game calls it here, if anything.
    name: Option<String>,
    has_interior: bool,
    summary: InteriorSummary,
    /// Its outside models, as indices into the scene; the first is the building itself.
    objects: Vec<usize>,
    /// The boxes round its entrances, in world space.
    entrances: Vec<[Vec3; 8]>,
}

impl ExteriorBuilding {
    /// Its name, or else its type.
    fn label(&self) -> String {
        self.name.clone().unwrap_or_else(|| building_type_name(self.data.building_type))
    }
}

/// A door into a dungeon: faces of a model drawn with dungeon door textures.
struct DungeonDoor {
    /// The scene object (a model) the door is part of.
    object: usize,
    /// The box round it, in world space.
    corners: [Vec3; 8],
}

/// The boxes round a model's doors, in its own space.
#[derive(Default)]
struct ModelDoors {
    /// Into a building.
    building: Vec<(Vec3, Vec3)>,
    /// Into a dungeon.
    dungeon: Vec<(Vec3, Vec3)>,
}

impl ModelDoors {
    fn of(model_id: u32, library: &mut Arena2Library) -> Self {
        library
            .model(model_id)
            .map(|mesh| Self {
                building: door_boxes(&mesh, DoorKind::Building),
                dungeon: door_boxes(&mesh, DoorKind::DungeonEntrance),
            })
            .unwrap_or_default()
    }
}

/// Colours of the town map, by what the game draws (`ExteriorAutomap`):
/// temples and guild halls, shops, taverns, and every other building.
const TOWN_MAP_TEMPLE: egui::Color32 = egui::Color32::from_rgb(80, 130, 230);
const TOWN_MAP_SHOP: egui::Color32 = egui::Color32::from_rgb(230, 150, 40);
const TOWN_MAP_TAVERN: egui::Color32 = egui::Color32::from_rgb(90, 180, 70);
const TOWN_MAP_HOUSE: egui::Color32 = egui::Color32::from_rgb(160, 115, 85);
/// The ground under the town map.
const TOWN_MAP_GROUND: egui::Color32 = egui::Color32::from_rgb(28, 30, 34);
const TOWN_MAP_LEGEND: [(egui::Color32, &str); 4] = [
    (TOWN_MAP_TEMPLE, "Temples, guild halls"),
    (TOWN_MAP_SHOP, "Shops"),
    (TOWN_MAP_TAVERN, "Taverns"),
    (TOWN_MAP_HOUSE, "Houses, palaces, others"),
];

/// The colour the game draws a town map byte in: the building type there
/// plus 1, as `ExteriorAutomap` colours it; `None` for nothing drawn (0, and
/// the ships and special types its map leaves out).
fn town_map_color(byte: u8) -> Option<egui::Color32> {
    match byte {
        12 | 15 => Some(TOWN_MAP_TEMPLE),
        1 | 3 | 4 | 6 | 7 | 9 | 10 | 11 | 13 | 14 => Some(TOWN_MAP_SHOP),
        16 => Some(TOWN_MAP_TAVERN),
        2 | 5 | 8 | 17..=24 => Some(TOWN_MAP_HOUSE),
        _ => None,
    }
}

/// The building type drawn at pixel (`x`, `y`) of the town map (from the
/// top left), if one is.
fn town_map_pixel(blocks: &[ExteriorBlock], x: usize, y: usize) -> Option<u8> {
    let rows = blocks.iter().map(|cell| cell.row + 1).max().unwrap_or(1);
    let (column, row) = (x / AUTOMAP_SIDE, rows - 1 - (y / AUTOMAP_SIDE).min(rows - 1));
    let cell = blocks.iter().find(|cell| cell.column == column && cell.row == row)?;
    let byte = *cell.block.as_ref()?.automap.get((y % AUTOMAP_SIDE) * AUTOMAP_SIDE + x % AUTOMAP_SIDE)?;
    town_map_color(byte).map(|_| byte - 1)
}

/// The town map of `blocks`: each cell's 64 × 64 map at its place on the
/// location's grid, north up (cells' rows count from the south, their
/// maps' rows from the north).
fn town_map_image(blocks: &[ExteriorBlock]) -> egui::ColorImage {
    let columns = blocks.iter().map(|cell| cell.column + 1).max().unwrap_or(1);
    let rows = blocks.iter().map(|cell| cell.row + 1).max().unwrap_or(1);
    let width = columns * AUTOMAP_SIDE;
    let mut image = egui::ColorImage::new([width, rows * AUTOMAP_SIDE], TOWN_MAP_GROUND);
    for cell in blocks {
        let Some(block) = &cell.block else { continue };
        let top = (rows - 1 - cell.row) * AUTOMAP_SIDE;
        let left = cell.column * AUTOMAP_SIDE;
        for (index, &byte) in block.automap.iter().enumerate() {
            if let Some(color) = town_map_color(byte) {
                let (y, x) = (index / AUTOMAP_SIDE, index % AUTOMAP_SIDE);
                image.pixels[(top + y) * width + left + x] = color;
            }
        }
    }
    image
}

/// A row of who rules region `region_index`: its province's ruler's title
/// (`FACTION.TXT`), with the named people of the province (its court) on
/// hover. A region without a province of its own takes the title of the
/// "Random Ruler", as Daggerfall Unity titles it.
fn ruler_row(ui: &mut egui::Ui, region_index: usize, references: &GameReferences) {
    let factions = &references.factions;
    let province = |region: i32| {
        factions.values().find(|faction| faction.kind == PROVINCE_FACTION_KIND && faction.region == region)
    };
    let own = province(region_index as i32);
    let Some(faction) = own.or_else(|| province(-1)) else { return };
    let title = ruler_title_name(faction.ruler);
    let court: Vec<&str> = faction
        .children
        .iter()
        .filter_map(|child| factions.get(child))
        .filter(|child| child.kind == INDIVIDUAL_FACTION_KIND)
        .map(|child| child.name.as_str())
        .collect();
    let response = ui.label("Ruler");
    let hover = match court.is_empty() {
        true => "The title of the region's ruler (FACTION.TXT), which palaces' and some shops' names use"
            .to_string(),
        false => format!(
            "The title of the region's ruler (FACTION.TXT), which palaces' and some shops' names use.\n\
             The province's named people (its court): {}",
            court.join(", ")
        ),
    };
    response.on_hover_text(hover);
    match own {
        Some(_) => ui.label(format!("{title} of {}", faction.name)),
        None => {
            ui.add(egui::Label::new(format!("{title}: the region has no province faction of its own")).wrap())
        }
    };
    ui.end_row();
    if !court.is_empty() {
        ui.label("Court").on_hover_text("The named people of the province's faction, whom quests can use");
        ui.add(egui::Label::new(court.join(", ")).wrap());
        ui.end_row();
    }
}

/// The models of a settlement's gate, open and shut (`RMBLayout.CityGateOpenModelID`, `…ClosedModelID`).
const CITY_GATE_OPEN_MODEL_ID: u32 = 446;
const CITY_GATE_SHUT_MODEL_ID: u32 = 447;

/// Whether `model_id` is a settlement's gate, open or shut.
fn is_city_gate(model_id: u32) -> bool {
    model_id == CITY_GATE_OPEN_MODEL_ID || model_id == CITY_GATE_SHUT_MODEL_ID
}

/// Most dungeons listed for a dungeon door of a cell built into many locations.
const LISTED_CELL_DUNGEONS: usize = 8;

/// One outdoor cell of the view.
pub struct ExteriorBlock {
    /// The block's name as the location gives it.
    pub name: String,
    /// The block drawn: the same, or Daggerfall Unity's substitute for a name that does not exist.
    pub found_name: String,
    /// World position of the block's south-west corner.
    pub origin: Vec3,
    /// Column from the west and row from the south on the location's grid.
    pub column: usize,
    pub row: usize,
    /// The block as read, `None` if it could not be.
    block: Option<RmbBlock>,
}

/// State of the view.
pub struct ExteriorView {
    pub subject: ExteriorSubject,
    /// The location's name, or the cell's block name.
    pub title: String,
    /// Who each person outdoors is, by cell and flat.
    people: HashMap<(usize, usize), PersonDetails>,
    /// Who each person outdoors is, by scene object.
    person_of_object: HashMap<usize, PersonDetails>,
    /// The town map drawn from the cells' maps, made when first shown.
    town_map: Option<super::kept_texture::KeptTexture>,
    /// Whether the scene was built as at night (gates shut), for the game's lighting.
    scene_at_night: bool,
    blocks: Vec<ExteriorBlock>,
    /// The climate the cells are drawn in.
    climate: Climate,
    /// The season they are drawn in.
    season: Season,
    /// The climate the game gives the location; `None` for a cell on its own.
    location_climate: Option<Climate>,
    /// Every cell's objects in world space, each with its cell's index as `group_index`.
    scene: Scene,
    outlines: Vec<RoomOutline>,
    camera: OrbitCamera,
    view_settings: ViewSettings,
    target: ViewportTarget,
    hovered_block: Option<usize>,
    /// Cell whose row is hovered in the properties panel, highlighted like a hovered cell.
    list_hovered_block: Option<usize>,
    last_pointer_position: Option<egui::Pos2>,
    /// The settlement shown; `None` for a cell on its own.
    settlement: Option<Settlement>,
    /// Each cell's buildings' data and names, as the game sees them here.
    building_data: Vec<Vec<(LocationBuilding, Option<String>)>>,
    buildings: Vec<ExteriorBuilding>,
    /// The building each scene object belongs to, as an index into `buildings`.
    building_of_object: Vec<Option<usize>>,
    /// The boxes round each model's doors, in its own space, by model id.
    door_box_cache: HashMap<u32, ModelDoors>,
    /// The doors into the location's dungeon.
    dungeon_doors: Vec<DungeonDoor>,
    /// The doors drawn with dungeon door textures that lead into their
    /// building's own inside, by building (its cell's block name and its place there).
    inside_door_cache: HashMap<(String, usize), Vec<OutsideDoor>>,
    /// How many such doors each scene object has.
    doors_into_inside: HashMap<usize, usize>,
    selected_object: Option<usize>,
    hovered_object: Option<usize>,
    /// Whether the boxes round buildings' entrances are drawn (View menu).
    show_entrances: bool,
    /// Whether the boxes round dungeons' entrances are drawn (View menu).
    show_dungeon_entrances: bool,
    /// Set to point the camera at the selection when the view is next drawn.
    frame_selected: bool,
}

impl ExteriorView {
    /// Loads the settlement `location` (at `location_index` in the library's
    /// list), drawn in `climate`, the climate the game gives it.
    pub fn settlement(
        location_index: usize,
        location: &LocationDefinition,
        climate: Climate,
        namer: &BuildingNamer<'_>,
        library: &mut Arena2Library,
    ) -> (Self, Vec<String>) {
        let exterior = &location.exterior;
        let settlement = Settlement {
            index: location_index,
            name: location.name.clone(),
            region_index: location.region_index,
            index_in_region: location.index_in_region,
            region_name: location.region_name,
        };
        Self::load(
            ExteriorSubject::Settlement(location_index),
            location.name.clone(),
            &exterior.block_names,
            exterior.width,
            climate,
            Some((settlement, location, namer)),
            library,
        )
    }

    /// Loads the outdoor cell `block_name` on its own, drawn in the default climate.
    pub fn cell(block_name: &str, library: &mut Arena2Library) -> (Self, Vec<String>) {
        Self::load(
            ExteriorSubject::Cell(block_name.to_string()),
            block_name.to_string(),
            &[block_name.to_string()],
            1,
            Climate::DEFAULT,
            None,
            library,
        )
    }

    fn load(
        subject: ExteriorSubject,
        title: String,
        block_names: &[String],
        width: usize,
        climate: Climate,
        settlement: Option<(Settlement, &LocationDefinition, &BuildingNamer<'_>)>,
        library: &mut Arena2Library,
    ) -> (Self, Vec<String>) {
        let location_climate = settlement.as_ref().map(|_| climate);
        let width = width.max(1);
        let mut problems = Vec::new();
        let blocks = block_names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let (column, row) = (index % width, index / width);
                let block = library
                    .rmb_block(name)
                    .map_err(|error| problems.push(format!("{name} could not be loaded: {error}")))
                    .ok();
                ExteriorBlock {
                    name: name.clone(),
                    found_name: block.as_ref().map_or_else(|| name.clone(), |block| block.name.clone()),
                    origin: Vec3::new(column as f32 * RMB_BLOCK_SIDE, 0.0, row as f32 * RMB_BLOCK_SIDE),
                    column,
                    row,
                    block,
                }
            })
            .collect::<Vec<ExteriorBlock>>();
        // What the game makes of each building here: in a settlement, its
        // named buildings take their data from its list, and get their names.
        let building_data: Vec<Vec<(LocationBuilding, Option<String>)>> = match &settlement {
            Some((place, location, namer)) => {
                let cells: Vec<Option<&RmbBlock>> = blocks.iter().map(|cell| cell.block.as_ref()).collect();
                settlement_buildings(location, &cells)
                    .into_iter()
                    .map(|buildings| {
                        buildings
                            .into_iter()
                            .map(|data| (data, namer.name(&data, &place.name, place.region_index)))
                            .collect()
                    })
                    .collect()
            }
            None => blocks
                .iter()
                .map(|cell| {
                    let buildings =
                        cell.block.as_ref().map(|block| block.buildings.as_slice()).unwrap_or_default();
                    buildings.iter().map(|building| (building.data, None)).collect()
                })
                .collect(),
        };
        // Who the people outdoors are; their names need the settlement.
        let references = library.references();
        let unity = settlement.as_ref().and_then(|(_, _, namer)| namer.unity);
        let person_place = settlement.as_ref().map(|(place, ..)| PersonPlace {
            region_index: place.region_index,
            index_in_region: place.index_in_region,
            building_key: 0,
        });
        let mut people = HashMap::new();
        for (cell_index, cell) in blocks.iter().enumerate() {
            let Some(block) = &cell.block else { continue };
            for (flat_index, flat) in block.flats.iter().enumerate() {
                if flat.faction_id == 0 || flat.texture.archive == MARKER_TEXTURE_ARCHIVE {
                    continue;
                }
                let gender = outdoor_gender(flat.texture, flat.flags, &references.flats);
                let details = person_details(
                    flat.texture,
                    flat.faction_id,
                    gender,
                    flat.record_offset,
                    person_place
                        .ok_or("Made up where the person stands: open a settlement built with this cell"),
                    unity,
                    &references,
                );
                people.insert((cell_index, flat_index), details);
            }
        }
        let mut view = Self {
            subject,
            title,
            people,
            person_of_object: HashMap::new(),
            town_map: None,
            scene_at_night: false,
            blocks,
            climate,
            season: Season::Summer,
            location_climate,
            scene: Scene::default(),
            outlines: Vec::new(),
            camera: OrbitCamera::default(),
            view_settings: ViewSettings { show_grid: false, ..ViewSettings::default() },
            target: ViewportTarget::default(),
            hovered_block: None,
            list_hovered_block: None,
            last_pointer_position: None,
            settlement: settlement.map(|(place, ..)| place),
            building_data,
            buildings: Vec::new(),
            building_of_object: Vec::new(),
            door_box_cache: HashMap::new(),
            dungeon_doors: Vec::new(),
            inside_door_cache: HashMap::new(),
            doors_into_inside: HashMap::new(),
            selected_object: None,
            hovered_object: None,
            show_entrances: true,
            show_dungeon_entrances: true,
            frame_selected: false,
        };
        view.rebuild_scene(library);
        view.frame(library);
        (view, problems)
    }

    /// Builds the world space scene and outlines from the cells, in the view's
    /// climate, with the buildings: their models and the boxes round their entrances.
    fn rebuild_scene(&mut self, library: &mut Arena2Library) {
        let mut objects: Vec<SceneObject> = Vec::new();
        let mut buildings = Vec::new();
        let mut person_of_object = HashMap::new();
        // The boxes of the doors drawn as dungeon doors that lead into their building's inside, by object.
        let mut doors_into_inside: HashMap<usize, Vec<(Vec3, Vec3)>> = HashMap::new();
        for (index, cell) in self.blocks.iter().enumerate() {
            let Some(block) = &cell.block else { continue };
            let scene = crate::exterior::cell_scene(block, self.climate, self.season, index, library);
            // The cell's scene is its ground (if it has one), its models, then its flats.
            let first_model = objects.len() + scene.objects.len() - block.models.len() - block.flats.len();
            objects.extend(
                scene
                    .objects
                    .into_iter()
                    .map(|object| SceneObject { position: object.position + cell.origin, ..object }),
            );
            // People outdoors go by their names.
            let first_flat = first_model + block.models.len();
            for flat_index in 0..block.flats.len() {
                let Some(person) = self.people.get(&(index, flat_index)) else { continue };
                let object = first_flat + flat_index;
                if let Some(name) = person.name.text() {
                    objects[object].name = name.to_string();
                }
                person_of_object.insert(object, person.clone());
            }
            for (record, building) in block.buildings.iter().enumerate() {
                let (data, name) = self
                    .building_data
                    .get(index)
                    .and_then(|data| data.get(record).cloned())
                    .unwrap_or((building.data, None));
                let models: Vec<usize> = building.models.clone().map(|model| first_model + model).collect();
                let mut entrances = Vec::new();
                // Doors drawn as dungeon doors that lead into the building's own inside are its entrances.
                let into_inside = self
                    .inside_door_cache
                    .entry((block.name.clone(), record))
                    .or_insert_with(|| dungeon_textured_entrances(block, record, library));
                for door in into_inside.iter() {
                    let object_index = first_model + door.model;
                    let matrix = objects[object_index].model_matrix();
                    entrances.push(
                        box_corners(door.minimum, door.maximum).map(|corner| matrix.transform_point3(corner)),
                    );
                    doors_into_inside.entry(object_index).or_default().push((door.minimum, door.maximum));
                }
                if building.has_interior() {
                    for &object_index in &models {
                        let object = &objects[object_index];
                        let crate::scene::SceneObjectKind::Model { model_id, .. } = object.kind else {
                            continue;
                        };
                        let doors = self
                            .door_box_cache
                            .entry(model_id)
                            .or_insert_with(|| ModelDoors::of(model_id, library));
                        let matrix = object.model_matrix();
                        entrances.extend(doors.building.iter().map(|&(minimum, maximum)| {
                            box_corners(minimum, maximum).map(|corner| matrix.transform_point3(corner))
                        }));
                    }
                }
                buildings.push(ExteriorBuilding {
                    block: index,
                    record,
                    data,
                    name,
                    has_interior: building.has_interior(),
                    summary: InteriorSummary::of(&building.interior),
                    objects: models,
                    entrances,
                });
            }
        }
        // Ids must be unique in the whole scene.
        for (number, object) in objects.iter_mut().enumerate() {
            object.id = number as u32 + 1;
        }
        // Gates stand open by day and shut at night, whichever way they are stored.
        let night = self.view_settings.game_lighting;
        let gate = if night { CITY_GATE_SHUT_MODEL_ID } else { CITY_GATE_OPEN_MODEL_ID };
        for object in &mut objects {
            if let SceneObjectKind::Model { model_id, .. } = &mut object.kind {
                if is_city_gate(*model_id) {
                    *model_id = gate;
                }
            }
        }
        self.scene_at_night = night;
        self.dungeon_doors.clear();
        for (index, object) in objects.iter().enumerate() {
            let crate::scene::SceneObjectKind::Model { model_id, .. } = object.kind else { continue };
            let doors =
                self.door_box_cache.entry(model_id).or_insert_with(|| ModelDoors::of(model_id, library));
            let matrix = object.model_matrix();
            let into_inside = doors_into_inside.get(&index);
            self.dungeon_doors.extend(
                doors
                    .dungeon
                    .iter()
                    .filter(|door| !into_inside.is_some_and(|boxes| boxes.contains(door)))
                    .map(|&(minimum, maximum)| DungeonDoor {
                        object: index,
                        corners: box_corners(minimum, maximum).map(|corner| matrix.transform_point3(corner)),
                    }),
            );
        }
        self.doors_into_inside =
            doors_into_inside.into_iter().map(|(object, boxes)| (object, boxes.len())).collect();
        self.building_of_object = vec![None; objects.len()];
        for (building_index, building) in buildings.iter().enumerate() {
            for &object in &building.objects {
                self.building_of_object[object] = Some(building_index);
            }
        }
        self.buildings = buildings;
        self.person_of_object = person_of_object;
        self.scene = Scene { block_name: self.title.clone(), objects };
        self.outlines = self
            .blocks
            .iter()
            .map(|cell| RoomOutline { side: RMB_BLOCK_SIDE, ..RoomOutline::cell(cell.origin) })
            .collect();
    }

    /// Points the camera at the whole location.
    fn frame(&mut self, library: &mut Arena2Library) {
        if let Some((minimum, maximum)) = self.scene.bounds(library) {
            self.camera.frame_bounds(minimum, maximum);
        }
    }

    /// Draws the cells in another climate or season.
    fn set_climate(&mut self, climate: Climate, season: Season, library: &mut Arena2Library) {
        if climate != self.climate || season != self.season {
            self.climate = climate;
            self.season = season;
            self.rebuild_scene(library);
        }
    }

    /// Frees the GPU images.
    pub fn release(&mut self, render_state: &egui_wgpu::RenderState) {
        self.target.release(render_state);
    }

    /// Adds the View menu entries.
    pub fn view_menu(&mut self, ui: &mut egui::Ui, library: &mut Arena2Library) {
        let label = match self.subject {
            ExteriorSubject::Settlement(_) => "Frame settlement",
            ExteriorSubject::Cell(_) => "Frame cell",
        };
        if ui.add(egui::Button::new(label).shortcut_text("Home")).clicked() {
            self.frame(library);
            ui.close_menu();
        }
        ui.separator();
        ui.checkbox(&mut self.show_entrances, "Building entrances")
            .on_hover_text("Green boxes round the entrances of the buildings that can be entered");
        ui.checkbox(&mut self.show_dungeon_entrances, "Dungeon entrances")
            .on_hover_text("Red boxes round the doors into the dungeon");
        super::room_editor::view_settings_menu(ui, &mut self.view_settings);
    }

    /// Handles the view's keyboard shortcuts: Home frames the view, Escape clears the selection.
    pub fn handle_shortcuts(&mut self, context: &egui::Context, library: &mut Arena2Library) {
        if context.wants_keyboard_input() {
            return;
        }
        let frame_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Home);
        if context.input_mut(|input| input.consume_shortcut(&frame_shortcut)) {
            self.frame(library);
        }
        if context.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.selected_object = None;
        }
    }

    /// The building object `index` belongs to, if any.
    fn building_of(&self, index: Option<usize>) -> Option<&ExteriorBuilding> {
        let building = (*self.building_of_object.get(index?)?)?;
        self.buildings.get(building)
    }

    /// What opening the inside of `building` asks the application for.
    fn interior_request(&self, building: &ExteriorBuilding) -> Option<ExteriorOpen> {
        let cell = self.blocks.get(building.block)?;
        let key = InteriorKey { block: cell.found_name.clone(), record: building.record };
        let place = self.settlement.as_ref().map(|settlement| BuildingPlace {
            location_index: settlement.index,
            location_name: settlement.name.clone(),
            region_index: settlement.region_index,
            region_name: settlement.region_name,
            index_in_region: settlement.index_in_region,
            building_key: building_key(cell.column, cell.row, building.record),
            data: building.data,
            name: building.name.clone(),
        });
        Some(ExteriorOpen::Interior { key, place, climate: self.climate })
    }

    /// The climate the cells are drawn in.
    pub fn climate(&self) -> Climate {
        self.climate
    }

    /// The season they are drawn in.
    pub fn season(&self) -> Season {
        self.season
    }

    /// Where a test of the location on its own starts the player (see
    /// [`crate::playtest`]): the clear spot nearest its middle, in metres
    /// from its south-west corner (x to the east, y to the north).
    pub fn test_spot(&self, library: &mut Arena2Library) -> glam::Vec2 {
        let columns = self.blocks.iter().map(|cell| cell.column + 1).max().unwrap_or(1);
        let rows = self.blocks.iter().map(|cell| cell.row + 1).max().unwrap_or(1);
        let size = glam::Vec2::new(columns as f32, rows as f32) * RMB_BLOCK_SIDE;
        let obstacles = crate::playtest::places::obstacles(&self.scene.objects, library);
        crate::playtest::places::clear_spot(&obstacles, size / 2.0, glam::Vec2::ZERO, size)
    }

    /// Name and kind, for example "Daggerfall (City, Daggerfall)", for the window title.
    pub fn description(&self, locations: &[LocationDefinition]) -> String {
        match &self.subject {
            ExteriorSubject::Settlement(index) => match locations.get(*index) {
                Some(location) => format!(
                    "{} ({}, {})",
                    location.name,
                    location.location_type.map_or("Location", |kind| kind.name()),
                    location.region_name
                ),
                None => self.title.clone(),
            },
            ExteriorSubject::Cell(name) => format!("{name} ({})", rmb_block_kind(name)),
        }
    }

    /// The status bar text.
    pub fn status_text(&self, custom_names: &CustomNames, locations: &[LocationDefinition]) -> String {
        let description = self.description(locations);
        if let Some(building) = self.building_of(self.hovered_object) {
            let hint = match building.has_interior {
                true => "double-click to look inside",
                false => "it has no inside",
            };
            return format!(
                "{description}: {} ({}) - {hint}",
                building.label(),
                building_type_name(building.data.building_type)
            );
        }
        match &self.subject {
            ExteriorSubject::Settlement(_) => {
                let hint = match self.hovered_block.and_then(|index| self.blocks.get(index)) {
                    Some(cell) => format!(
                        "{} ({}) - double-click to open it",
                        custom_names.room_label(&cell.found_name),
                        rmb_block_kind(&cell.found_name)
                    ),
                    None => "Hover a cell to highlight it, double-click to open it; double-click a building with a \
                             green entrance box to look inside, or a model with a red dungeon door to open the dungeon"
                        .to_string(),
                };
                format!("{description}: {} cells, read-only. {hint}", self.blocks.len())
            }
            ExteriorSubject::Cell(_) => {
                format!(
                    "{description}, read-only. Drawn in the {} climate, in {}.",
                    self.climate.name.to_lowercase(),
                    self.season.name().to_lowercase()
                )
            }
        }
    }

    /// Draws the 3D view. Returns what the user asked for (opening the cell they double-clicked).
    pub fn show(&mut self, ui: &mut egui::Ui, resources: &mut EditorResources<'_>) -> Option<ExteriorOpen> {
        // The game's lighting shows night, when the gates are shut.
        if self.view_settings.game_lighting != self.scene_at_night {
            self.rebuild_scene(resources.library);
        }
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        if handle_camera_input(&mut self.camera, ui, &response) {
            self.last_pointer_position = None;
        }
        let projection = ViewportProjection::new(&self.camera, rect);
        if !response.dragged() {
            let pointer_position = response.hover_pos();
            if pointer_position != self.last_pointer_position {
                self.last_pointer_position = pointer_position;
                (self.hovered_block, self.hovered_object) = match pointer_position {
                    Some(pointer) => self.under_pointer(&projection, pointer, resources.library),
                    None => (None, None),
                };
            }
        }
        if response.clicked_by(egui::PointerButton::Primary) {
            self.selected_object = self.hovered_object;
        }
        if std::mem::take(&mut self.frame_selected) {
            self.frame_selection(resources.library);
        }
        let is_settlement = matches!(self.subject, ExteriorSubject::Settlement(_));
        // The building under the pointer (its cell and place, and its models), and the selected one.
        let hovered_building = self
            .building_of(self.hovered_object)
            .map(|building| ((building.block, building.record), building.objects.clone()));
        let selected_building =
            self.building_of(self.selected_object).map(|building| (building.block, building.record));
        // A building under the pointer is highlighted rather than its cell.
        let highlighted = self
            .list_hovered_block
            .or(self.hovered_block.filter(|_| hovered_building.is_none()))
            .filter(|_| is_settlement);
        for (index, outline) in self.outlines.iter_mut().enumerate() {
            outline.highlighted = highlighted == Some(index);
        }
        let hovered_key = hovered_building.as_ref().map(|(key, _)| *key);
        let highlighted_objects: Vec<usize> =
            hovered_building.map(|(_, objects)| objects).unwrap_or_default();
        let mut entrance_boxes: Vec<EntranceBox> = match self.show_entrances {
            true => self
                .buildings
                .iter()
                .flat_map(|building| {
                    let key = Some((building.block, building.record));
                    let highlighted = key == hovered_key || key == selected_building;
                    building.entrances.iter().map(move |&corners| EntranceBox {
                        corners,
                        highlighted,
                        kind: EntranceKind::Entrance,
                    })
                })
                .collect(),
            false => Vec::new(),
        };
        if self.show_dungeon_entrances {
            // A dungeon door is highlighted with its model, or with the building it is part of.
            let highlighted_objects = &highlighted_objects;
            entrance_boxes.extend(self.dungeon_doors.iter().map(|door| {
                let object = Some(door.object);
                let highlighted = object == self.hovered_object
                    || object == self.selected_object
                    || highlighted_objects.contains(&door.object)
                    || (selected_building.is_some()
                        && self.building_of(object).map(|building| (building.block, building.record))
                            == selected_building);
                EntranceBox { corners: door.corners, highlighted, kind: EntranceKind::DungeonEntrance }
            }));
        }
        let request = FrameRequest {
            scene: Some(&self.scene),
            camera: &self.camera,
            selected_object: self.selected_object,
            hovered_object: self.hovered_object,
            selected_objects: &[],
            hovered_objects: &[],
            highlighted_group: highlighted,
            highlighted_groups: &[],
            selected_groups: &[],
            placement_preview: &[],
            highlighted_objects: &highlighted_objects,
            context_scene: None,
            water_planes: &[],
            room_outlines: &self.outlines,
            settings: &self.view_settings,
            grid_height: OUTLINE_HEIGHT,
            snap_grid_spacing: None,
            size_in_pixels: size_in_pixels(ui, rect),
            sky: None,
            dungeon_textures: None,
            climate_textures: Some(ClimateTextures::new(self.climate, self.season)),
            warned_objects: &[],
            entrance_boxes: &entrance_boxes,
            lighting: match self.view_settings.game_lighting {
                true => crate::lighting::GameLighting::Night,
                false => crate::lighting::GameLighting::Off,
            },
        };
        paint_scene(ui, rect, resources, &mut self.target, &request);

        // The hovered building's name, or else the hovered cell's, next to the pointer.
        let hovered = self.hovered_block.filter(|_| is_settlement).and_then(|index| self.blocks.get(index));
        let label = match (self.building_of(self.hovered_object), hovered) {
            (Some(building), _) => Some(match building.has_interior {
                true => format!("{}\nDouble-click to look inside", building.label()),
                false => format!("{}\nIt has no inside", building.label()),
            }),
            (None, Some(cell)) => Some(format!(
                "{} ({})\nDouble-click to open this cell",
                resources.custom_names.room_label(&cell.found_name),
                rmb_block_kind(&cell.found_name)
            )),
            (None, None) => None,
        };
        if let (Some(label), Some(pointer)) = (label, response.hover_pos()) {
            ui.painter_at(rect).text(
                pointer + egui::vec2(16.0, 16.0),
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::proportional(13.0),
                egui::Color32::WHITE,
            );
        }

        if !response.double_clicked_by(egui::PointerButton::Primary) {
            return None;
        }
        // A building with an inside opens it, a dungeon door the dungeon; anything else in a settlement opens its cell.
        if let Some(building) = self.building_of(self.hovered_object).filter(|building| building.has_interior)
        {
            return self.interior_request(building);
        }
        if let (ExteriorSubject::Settlement(location), Some(object)) = (&self.subject, self.hovered_object) {
            if self.dungeon_doors.iter().any(|door| door.object == object) {
                return Some(ExteriorOpen::DungeonUnder(*location));
            }
        }
        let cell = hovered?;
        Some(ExteriorOpen::Cell(cell.found_name.clone()))
    }

    /// The cell and the object under the pointer: the cell owning the object
    /// hit, or else the one whose square the pointer is over.
    fn under_pointer(
        &self,
        projection: &ViewportProjection,
        pointer: egui::Pos2,
        library: &mut Arena2Library,
    ) -> (Option<usize>, Option<usize>) {
        let ray = self.camera.ray_through(projection.screen_to_ndc(pointer), aspect_ratio(projection.rect));
        let settings = &self.view_settings;
        let options = PickOptions {
            ignore_back_faces: settings.cull_back_faces,
            include_models: settings.show_models,
            include_flats: settings.show_flats,
            include_lights: false,
            camera_right: self.camera.right(),
        };
        if let Some(hit) = pick_object(&self.scene, library, &ray, &options) {
            // The ground is part of its cell but not an object to select.
            let object = &self.scene.objects[hit.object_index];
            let selectable = object.name != "Ground";
            return (Some(object.group_index), selectable.then_some(hit.object_index));
        }
        let Some(distance) = intersect_horizontal_plane(&ray, 0.0) else { return (None, None) };
        let point = ray.point_at(distance);
        let cell = self.blocks.iter().position(|cell| {
            let local = point - cell.origin;
            (0.0..RMB_BLOCK_SIDE).contains(&local.x) && (0.0..RMB_BLOCK_SIDE).contains(&local.z)
        });
        (cell, None)
    }

    /// Draws the properties panel on the right of `context`. Call it before [`Self::show`].
    pub fn show_properties(
        &mut self,
        context: &egui::Context,
        details: &ExteriorDetails<'_>,
        custom_names: &mut CustomNames,
        library: &mut Arena2Library,
    ) -> Option<ExteriorOpen> {
        let mut action = None;
        egui::SidePanel::right("exterior_properties").resizable(true).default_width(280.0).show(
            context,
            |ui| {
                ui.heading("Properties");
                ui.separator();
                if self.selected_object.is_some() {
                    self.show_selected_object(ui, details, &mut action);
                    return;
                }
                match self.subject.clone() {
                    ExteriorSubject::Settlement(index) => {
                        if let Some(location) = details.locations.get(index) {
                            self.show_settlement_properties(
                                ui,
                                location,
                                details,
                                custom_names,
                                library,
                                &mut action,
                            );
                        }
                    }
                    ExteriorSubject::Cell(name) => {
                        self.show_cell_properties(ui, &name, details, custom_names, library, &mut action);
                    }
                }
            },
        );
        action
    }

    /// For a selected model with doors into a dungeon, where they lead.
    fn show_dungeon_door(
        &self,
        ui: &mut egui::Ui,
        index: usize,
        details: &ExteriorDetails<'_>,
        action: &mut Option<ExteriorOpen>,
    ) {
        let doors = self.dungeon_doors.iter().filter(|door| door.object == index).count();
        let into_inside = self.doors_into_inside.get(&index).copied().unwrap_or(0);
        if doors > 0 {
            ui.add_space(6.0);
            ui.strong("Dungeon entrance").on_hover_text(
                "Faces of this model drawn as a dungeon door (boxed in red): the player goes into the location's \
                 dungeon through them. Double-click the model to open the dungeon.",
            );
            egui::Grid::new("exterior_dungeon_door").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Doors");
                ui.label(doors.to_string());
                ui.end_row();
                ui.label("Leads into");
                self.dungeon_links(ui, details, action);
                ui.end_row();
            });
        }
        if into_inside > 0 {
            ui.add_space(6.0);
            ui.strong("Doors into its rooms").on_hover_text(
                "Faces of this model drawn as a dungeon door that stand where an exit of the building's own inside \
                 is: the data means them as doors into the building (boxed in green)",
            );
            egui::Grid::new("exterior_doors_into_inside").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Doors");
                ui.label(into_inside.to_string());
                ui.end_row();
                ui.label("In the game").on_hover_text("Daggerfall Unity tells doors apart by their texture alone");
                ui.add(
                    egui::Label::new(
                        "Drawn as dungeon doors, so Daggerfall Unity takes the player into the location's dungeon \
                         through them instead",
                    )
                    .wrap(),
                );
                ui.end_row();
            });
        }
    }

    /// The dungeon a dungeon door leads into: the settlement's, or those of
    /// the locations built with the cell; each a link to open it.
    fn dungeon_links(
        &self,
        ui: &mut egui::Ui,
        details: &ExteriorDetails<'_>,
        action: &mut Option<ExteriorOpen>,
    ) {
        let dungeons: Vec<(usize, &str)> = match &self.subject {
            ExteriorSubject::Settlement(_) => details.dungeon.into_iter().collect(),
            ExteriorSubject::Cell(_) => details.cell_dungeons.to_vec(),
        };
        if dungeons.is_empty() {
            let text = match self.subject {
                ExteriorSubject::Settlement(_) => "The location's dungeon",
                ExteriorSubject::Cell(_) => "The dungeon of each location built with this cell",
            };
            ui.add(egui::Label::new(text).wrap());
            return;
        }
        ui.vertical(|ui| {
            for &(dungeon_index, name) in dungeons.iter().take(LISTED_CELL_DUNGEONS) {
                if ui.link(name).on_hover_text("Open the dungeon").clicked() {
                    *action = Some(ExteriorOpen::Dungeon(dungeon_index));
                }
            }
            if dungeons.len() > LISTED_CELL_DUNGEONS {
                ui.label(
                    egui::RichText::new(format!(
                        "and {} more (see Used in)",
                        dungeons.len() - LISTED_CELL_DUNGEONS
                    ))
                    .weak(),
                );
            }
        });
    }

    /// The properties of the selected object: what it is and, for a
    /// building, what the game makes of it here and what is inside.
    fn show_selected_object(
        &mut self,
        ui: &mut egui::Ui,
        details: &ExteriorDetails<'_>,
        action: &mut Option<ExteriorOpen>,
    ) {
        let references = details.references;
        let Some(index) = self.selected_object.filter(|&index| index < self.scene.objects.len()) else {
            return;
        };
        let mut clear = false;
        ui.horizontal(|ui| {
            ui.strong("Selected object");
            clear = ui
                .small_button("Back")
                .on_hover_text("Clear the selection and show the properties (Esc)")
                .clicked();
            if ui.small_button("Frame").on_hover_text("Point the camera at it (its whole building)").clicked()
            {
                self.frame_selected = true;
            }
        });
        object_rows(
            ui,
            "exterior_object",
            &self.scene.objects[index],
            references,
            ObjectPlace::Outdoors,
            self.person_of_object.get(&index),
        );
        self.show_dungeon_door(ui, index, details, action);
        if matches!(self.scene.objects[index].kind, SceneObjectKind::Model { model_id, .. } if is_city_gate(model_id))
        {
            ui.add_space(6.0);
            ui.strong("City gate")
                .on_hover_text("Daggerfall Unity opens and shuts it by the hour (DaggerfallCityGate)");
            egui::Grid::new("exterior_city_gate").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Open");
                ui.label("6:00 to 18:00");
                ui.end_row();
                ui.label("At night");
                ui.add(
                    egui::Label::new(
                        "Shut: Daggerfall Unity draws model 447 instead of the open gate's 446 (View > Game \
                         lighting shows it shut)",
                    )
                    .wrap(),
                );
                ui.end_row();
            });
        }
        if let Some(building) = self.building_of(Some(index)) {
            ui.add_space(6.0);
            ui.strong("Building").on_hover_text("The building this model is part of");
            egui::Grid::new("exterior_building").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                if self.settlement.is_some() {
                    ui.label("Name").on_hover_text(
                        "What Daggerfall Unity calls the building in this settlement, from its name seed, \
                         type and faction",
                    );
                    match &building.name {
                        Some(name) => ui.strong(name),
                        None if crate::buildings::is_named_building(building.data.building_type) => ui.label(
                            egui::RichText::new(
                                "Needs Daggerfall Unity's folder (File > Set Daggerfall Unity folder…)",
                            )
                            .weak(),
                        ),
                        None => {
                            ui.label(egui::RichText::new("None: the game does not name this kind").weak())
                        }
                    };
                    ui.end_row();
                }
                ui.label("Type");
                ui.label(building_type_name(building.data.building_type));
                ui.end_row();
                if self.settlement.is_some() {
                    ui.label("Quality")
                        .on_hover_text("How good the building is (1 to 20): a shop's wares and prices");
                    ui.label(format!("{} of 20", building.data.quality));
                    ui.end_row();
                    ui.label("Faction").on_hover_text("The faction the building belongs to (FACTION.TXT)");
                    ui.add(egui::Label::new(faction_label(building.data.faction_id, references)).wrap());
                    ui.end_row();
                    faction_groups_row(ui, building.data.faction_id, references);
                }
                if let Some(cell) = self.blocks.get(building.block) {
                    ui.label("Cell")
                        .on_hover_text("The outdoor cell it stands in, and its place in the cell");
                    ui.label(format!("{}, building {}", cell.found_name, building.record + 1));
                    ui.end_row();
                    if self.settlement.is_some() {
                        ui.label("Building key").on_hover_text(
                            "How quests and saved games refer to the building in this settlement \
                             (BuildingDirectory.MakeBuildingKey): its cell's column and row on the settlement's \
                             grid, and its place in the cell",
                        );
                        ui.label(building_key_label(building_key(cell.column, cell.row, building.record)));
                        ui.end_row();
                    }
                }
                if building.has_interior {
                    ui.label("Open").on_hover_text(
                        "When the player can walk in without picking the lock, as Daggerfall Unity decides it \
                         (PlayerActivate.BuildingIsUnlocked). Any building is open to its owner, and a \
                         residence while a quest sends the player there.",
                    );
                    let access = building_access(building.data.building_type, building.data.faction_id);
                    ui.add(egui::Label::new(access.describe()).wrap());
                    ui.end_row();
                    let quality = self.settlement.as_ref().map(|_| building.data.quality);
                    shop_stock_row(ui, building.data.building_type, quality);
                    if people_leave_when_closed(building.data.building_type) && building.summary.people > 0 {
                        ui.label("People").on_hover_text(
                            "Daggerfall Unity takes the people out of a closed shop, and out of other buildings \
                             that keep hours while they are closed (not for members of the guild a guild hall \
                             or house belongs to)",
                        );
                        ui.label("Away while it is closed");
                        ui.end_row();
                    }
                }
                ui.label("Inside");
                match building.has_interior {
                    true => {
                        let summary = building.summary;
                        let count = |count: usize, one: &str, many: &str| {
                            format!("{count} {}", if count == 1 { one } else { many })
                        };
                        let text = [
                            count(summary.models, "model", "models"),
                            count(summary.people, "person", "people"),
                            count(summary.doors, "inside door", "inside doors"),
                            count(
                                summary.quest_markers + summary.quest_item_markers,
                                "quest marker",
                                "quest markers",
                            ),
                        ]
                        .join(", ");
                        ui.add(egui::Label::new(text).wrap());
                    }
                    false => {
                        ui.label(egui::RichText::new("None: it cannot be entered").weak());
                    }
                }
                ui.end_row();
            });
            if building.has_interior {
                ui.add_space(4.0);
                let button = ui.button("Look inside").on_hover_text(
                    "Opens the building's inside in a window of its own (or double-click the building). The \
                     green box shows its entrance.",
                );
                if button.clicked() {
                    *action = self.interior_request(building);
                }
            }
        }
        if clear {
            self.selected_object = None;
        }
    }

    /// The town map of the cells, as the game shows it, with what its colours mean.
    fn show_town_map(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        egui::CollapsingHeader::new("Town map").id_salt("exterior_town_map").default_open(true).show(
            ui,
            |ui| {
                let texture = self.town_map.get_or_insert_with(|| {
                    super::kept_texture::KeptTexture::new(ui.ctx().load_texture(
                        "town_map",
                        town_map_image(&self.blocks),
                        egui::TextureOptions::NEAREST,
                    ))
                });
                let [width, height] = texture.size().map(|side| side as f32);
                // Drawn at whole pixels, as large as fits.
                let scale = (ui.available_width() / width).clamp(1.0, 4.0).floor();
                let response = ui
                    .add(
                        egui::Image::new((texture.id(), egui::vec2(width, height) * scale))
                            .sense(egui::Sense::hover()),
                    )
                    .on_hover_text_at_pointer("The map the player sees in town, north up");
                if let Some(pointer) = response.hover_pos() {
                    let pixel = (pointer - response.rect.min) / scale;
                    if let Some(kind) = town_map_pixel(&self.blocks, pixel.x as usize, pixel.y as usize) {
                        response.on_hover_text_at_pointer(building_type_name(kind));
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    for (color, label) in TOWN_MAP_LEGEND {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 1.0, color);
                        ui.label(label);
                    }
                });
            },
        );
    }

    /// The settlement's named buildings, by name: clicking one selects it.
    fn show_named_buildings(&mut self, ui: &mut egui::Ui, action: &mut Option<ExteriorOpen>) {
        let mut named: Vec<(String, usize)> = self
            .buildings
            .iter()
            .enumerate()
            .filter(|(_, building)| crate::buildings::is_named_building(building.data.building_type))
            .map(|(index, building)| {
                (format!("{} · {}", building.label(), building_type_name(building.data.building_type)), index)
            })
            .collect();
        named.sort();
        egui::CollapsingHeader::new(format!("Named buildings ({})", named.len()))
            .id_salt("settlement_named_buildings")
            .show(ui, |ui| {
                if named.is_empty() {
                    ui.label(egui::RichText::new("None").weak());
                }
                for (label, index) in named {
                    let building = &self.buildings[index];
                    let hint = match building.has_interior {
                        true => "Click to select it, double-click to look inside",
                        false => "Click to select it",
                    };
                    let response = ui.selectable_label(false, label).on_hover_text(hint);
                    if response.clicked() {
                        self.selected_object = building.objects.first().copied();
                        self.frame_selected = true;
                    }
                    if response.double_clicked() && building.has_interior {
                        *action = self.interior_request(building);
                    }
                }
            });
    }

    /// The properties of a settlement: what and where it is, its buildings and its cells.
    fn show_settlement_properties(
        &mut self,
        ui: &mut egui::Ui,
        location: &LocationDefinition,
        details: &ExteriorDetails<'_>,
        custom_names: &CustomNames,
        library: &mut Arena2Library,
        action: &mut Option<ExteriorOpen>,
    ) {
        egui::Grid::new("settlement_property_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Name");
            ui.label(&location.name);
            ui.end_row();

            ui.label("Region");
            ui.label(location.region_name);
            ui.end_row();

            ruler_row(ui, location.region_index, details.references);

            ui.label("Type").on_hover_text("The kind of location, which decides its icon on the travel map");
            ui.label(location.location_type.map_or("Unknown", |kind| kind.name()));
            ui.end_row();

            ui.label("Source").on_hover_text("Daggerfall's own locations are shown, not changed");
            ui.label("Daggerfall (read-only)");
            ui.end_row();

            ui.label("Map").on_hover_text("Its pixel on the world map (column, row)");
            ui.label(location.map_pixel.to_string());
            ui.end_row();

            ui.label("Map id").on_hover_text(
                "The location's map id as stored (MapTableData.MapId), which Daggerfall Unity's location files \
                 and quests refer to it by",
            );
            ui.label(location.map_id.to_string());
            ui.end_row();

            ui.label("Location id").on_hover_text(
                "The id of the location's record, which the dungeon under it names as its exterior",
            );
            ui.label(location.location_id.to_string());
            ui.end_row();

            ui.label("Index").on_hover_text(
                "Its place in its region's list of locations (LocationIndex): with the region, how Daggerfall \
                 Unity's location files name it; it also seeds the names of its people",
            );
            ui.label(location.index_in_region.to_string());
            ui.end_row();

            ui.label("Travel map")
                .on_hover_text("Whether the travel map shows it from the start of the game");
            ui.label(if location.discovered { "Shown from the start" } else { "Found while playing" });
            ui.end_row();

            ui.label("Climate").on_hover_text(
                "The climate decides the ground, the trees and the buildings' textures, as the game draws \
                 them. Choose another to see the location in it (this only changes the view).",
            );
            self.show_climate_choice(ui, library);
            ui.end_row();

            ui.label("Season").on_hover_text(SEASON_HOVER);
            self.show_season_choice(ui, library);
            ui.end_row();

            ui.label("Size").on_hover_text("Cells (RMB blocks) from west to east, and from south to north");
            ui.label(format!("{} × {} cells", location.exterior.width, location.exterior.height));
            ui.end_row();

            ui.label("Port").on_hover_text("Whether it is a port town, whose bank sells ships");
            ui.label(if location.exterior.port_town { "Yes: its bank sells ships" } else { "No" });
            ui.end_row();

            let arrivals = self.arrival_points();
            ui.label("Arrival points").on_hover_text(
                "Start markers among the cells' loose flats: fast travel here puts the player at the one \
                 nearest to where they come in. Without any, at the edge they come in from.",
            );
            ui.label(match arrivals {
                0 => "None".to_string(),
                count => count.to_string(),
            });
            ui.end_row();

            ui.label("Dungeon").on_hover_text("The dungeon under the location, if it has one");
            match details.dungeon {
                Some((dungeon_index, name)) => {
                    if ui.link(name).on_hover_text("Open the dungeon").clicked() {
                        *action = Some(ExteriorOpen::Dungeon(dungeon_index));
                    }
                }
                None if location.has_dungeon => {
                    ui.label("Yes");
                }
                None => {
                    ui.label("None");
                }
            }
            ui.end_row();
        });
        self.show_climate_note(ui);

        self.show_town_map(ui);

        ui.add_space(6.0);
        let buildings: Vec<u8> =
            location.exterior.buildings.iter().map(|building| building.building_type).collect();
        show_buildings(ui, "settlement_buildings", &buildings);
        self.show_named_buildings(ui, action);

        ui.add_space(6.0);
        ui.strong("Cells")
            .on_hover_text("Every cell (RMB block) of the location, row by row from the south-west");
        ui.label(egui::RichText::new("Hover to highlight, double-click to open").weak());
        let mut hovered_row = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (index, cell) in self.blocks.iter().enumerate() {
                let response = ui
                    .horizontal(|ui| {
                        let highlighted = self.hovered_block == Some(index);
                        let response =
                            ui.selectable_label(highlighted, custom_names.room_label(&cell.found_name));
                        ui.label(egui::RichText::new(rmb_block_kind(&cell.found_name)).weak().small());
                        response
                    })
                    .inner;
                let substitute = match cell.found_name == cell.name {
                    true => String::new(),
                    false => {
                        format!("\nThe location names {}, which the game replaces with this cell", cell.name)
                    }
                };
                let response = response.on_hover_text(format!(
                    "{}, column {} from the west, row {} from the south{substitute}\nDouble-click to open it",
                    cell.found_name,
                    cell.column + 1,
                    cell.row + 1
                ));
                if response.hovered() {
                    hovered_row = Some(index);
                }
                if response.double_clicked() {
                    *action = Some(ExteriorOpen::Cell(cell.found_name.clone()));
                }
            }
        });
        self.list_hovered_block = hovered_row;
    }

    /// The properties of an outdoor cell: what it holds, and the settlements using it.
    fn show_cell_properties(
        &mut self,
        ui: &mut egui::Ui,
        name: &str,
        details: &ExteriorDetails<'_>,
        custom_names: &mut CustomNames,
        library: &mut Arena2Library,
        action: &mut Option<ExteriorOpen>,
    ) {
        let block = self.blocks.first().and_then(|cell| cell.block.as_ref());
        let buildings = block.map(RmbBlock::building_types).unwrap_or_default();
        let (models, flats) = block.map_or((0, 0), |block| (block.models.len(), block.flats.len()));
        egui::Grid::new("outdoor_cell_property_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Cell").on_hover_text("The RMB block's name in BLOCKS.BSA");
            ui.label(name);
            ui.end_row();

            ui.label("Name").on_hover_text("Your own name for the cell, shown in the editor only");
            show_room_name_field(ui, name, custom_names);
            ui.end_row();

            ui.label("Kind").on_hover_text("What the cell holds, from the first letters of its name");
            ui.label(rmb_block_kind(name));
            ui.end_row();

            ui.label("Source").on_hover_text("Daggerfall's own cells are shown, not changed");
            ui.label("Daggerfall (read-only)");
            ui.end_row();

            ui.label("Climate").on_hover_text(
                "The climate decides the ground, the trees and the buildings' textures, as the game draws \
                 them where the cell is used. Choose one to see the cell in it.",
            );
            self.show_climate_choice(ui, library);
            ui.end_row();

            ui.label("Season").on_hover_text(SEASON_HOVER);
            self.show_season_choice(ui, library);
            ui.end_row();

            ui.label("Objects")
                .on_hover_text("Models (buildings and scenery) and flats (sprites) in the cell");
            ui.label(format!("{models} models, {flats} flats"));
            ui.end_row();

            let interiors = self.buildings.iter().filter(|building| building.has_interior).count();
            ui.label("Interiors").on_hover_text(
                "Buildings with an inside to enter. Their entrances have green boxes; double-click one to look inside.",
            );
            ui.label(interiors.to_string());
            ui.end_row();

            let arrivals = self.arrival_points();
            ui.label("Arrival points").on_hover_text(
                "Start markers among the cell's loose flats: fast travel to a settlement puts the player at \
                 the nearest one",
            );
            ui.label(arrivals.to_string());
            ui.end_row();
        });

        ui.add_space(6.0);
        show_buildings(ui, "outdoor_cell_buildings", &buildings);
        self.show_town_map(ui);

        ui.add_space(6.0);
        let used_in = details.used_in;
        ui.strong(format!("Used in ({})", used_in.len()))
            .on_hover_text("Every location of the game built with this cell. Click one to open it.");
        if used_in.is_empty() {
            ui.label(egui::RichText::new("No location uses this cell").weak());
            return;
        }
        let row_height = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
            ui,
            row_height,
            used_in.len(),
            |ui, rows| {
                for &location_index in &used_in[rows] {
                    let Some(location) = details.locations.get(location_index) else { continue };
                    let label = format!(
                        "{} · {} · {}",
                        location.name,
                        location.location_type.map_or("Location", |kind| kind.name()),
                        location.region_name
                    );
                    if ui.selectable_label(false, label).on_hover_text("Click to open the location").clicked()
                    {
                        *action = Some(ExteriorOpen::Settlement(location_index));
                    }
                }
            },
        );
    }

    /// Points the camera at the selected object: at its whole building, if it is part of one.
    fn frame_selection(&mut self, library: &mut Arena2Library) {
        let Some(index) = self.selected_object else { return };
        let objects =
            self.building_of(Some(index)).map_or_else(|| vec![index], |building| building.objects.clone());
        let (mut minimum, mut maximum) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for object in objects.iter().filter_map(|&object| self.scene.objects.get(object)) {
            let corners = match object.kind {
                crate::scene::SceneObjectKind::Model { model_id, .. } => match library.model(model_id) {
                    Some(mesh) => box_corners(mesh.bounds_min, mesh.bounds_max)
                        .map(|corner| object.model_matrix().transform_point3(corner)),
                    None => continue,
                },
                _ => box_corners(object.position - Vec3::ONE, object.position + Vec3::ONE),
            };
            for corner in corners {
                minimum = minimum.min(corner);
                maximum = maximum.max(corner);
            }
        }
        if minimum.x <= maximum.x {
            self.camera.frame_bounds(minimum, maximum);
        }
    }

    /// How many start markers (arrival points after fast travel) the cells have.
    fn arrival_points(&self) -> usize {
        let start = TextureKey::new(MARKER_TEXTURE_ARCHIVE, START_MARKER_RECORD);
        self.blocks
            .iter()
            .filter_map(|cell| cell.block.as_ref())
            .map(|block| block.flats.iter().filter(|flat| flat.texture == start).count())
            .sum()
    }

    /// A drop-down list choosing the climate the cells are drawn in.
    fn show_climate_choice(&mut self, ui: &mut egui::Ui, library: &mut Arena2Library) {
        let mut chosen = self.climate;
        egui::ComboBox::from_id_salt("exterior_climate").selected_text(chosen.name).show_ui(ui, |ui| {
            for climate in Climate::ALL {
                let label = match Some(climate) == self.location_climate {
                    true => format!("{} (its own)", climate.name),
                    false => climate.name.to_string(),
                };
                ui.selectable_value(&mut chosen, climate, label);
            }
        });
        self.set_climate(chosen, self.season, library);
    }

    /// A drop-down list choosing the season the cells are drawn in.
    fn show_season_choice(&mut self, ui: &mut egui::Ui, library: &mut Arena2Library) {
        let mut chosen = self.season;
        egui::ComboBox::from_id_salt("exterior_season").selected_text(chosen.name()).show_ui(ui, |ui| {
            for season in Season::ALL {
                ui.selectable_value(&mut chosen, season, season.name());
            }
        });
        self.set_climate(self.climate, chosen, library);
    }

    /// Says when the location is drawn in a climate other than its own.
    fn show_climate_note(&self, ui: &mut egui::Ui) {
        if let Some(own) = self.location_climate.filter(|own| *own != self.climate) {
            ui.colored_label(
                NOTE_COLOR,
                format!(
                    "Drawn in the {} climate; the game draws it in its own, {}.",
                    self.climate.name, own.name
                ),
            );
        }
    }
}

/// Lists `buildings` (building type numbers) by type, the most common first.
fn show_buildings(ui: &mut egui::Ui, id: &str, buildings: &[u8]) {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for &building in buildings {
        let name = building_type_name(building);
        match counts.iter_mut().find(|(known, _)| *known == name) {
            Some((_, count)) => *count += 1,
            None => counts.push((name, 1)),
        }
    }
    counts.sort_by(|first, second| second.1.cmp(&first.1).then_with(|| first.0.cmp(&second.0)));
    let header = format!("Buildings ({})", buildings.len());
    egui::CollapsingHeader::new(header).id_salt(id).default_open(buildings.len() <= 8).show(ui, |ui| {
        if counts.is_empty() {
            ui.label(egui::RichText::new("None: only scenery").weak());
        }
        for (name, count) in counts {
            ui.label(match count {
                1 => name,
                _ => format!("{name} × {count}"),
            });
        }
    });
}
