//! Shows a settlement or an outdoor cell from Daggerfall, read-only: a
//! location's RMB blocks on their grid (or one block alone), standing on their
//! ground, with the ground tiles, trees and building textures of the
//! location's climate, in summer or winter, as Daggerfall Unity draws them.
//!
//! Hovering a cell highlights it and its outline; double-clicking one opens it
//! on its own. The properties panel on the right tells what the location is
//! (its region, type, place on the map, buildings and the dungeon under it)
//! and lists its cells; for an outdoor cell, it lists its buildings and every
//! settlement that uses it. Another climate or the season can be chosen
//! there, to see how the cell looks elsewhere, or with snow.

use eframe::{egui, egui_wgpu};
use glam::Vec3;

use super::room_browser::show_room_name_field;
use super::viewport_tools::{
    aspect_ratio, handle_camera_input, paint_scene, size_in_pixels, EditorResources, ViewportProjection,
};
use crate::arena2::{
    building_type_name, rmb_block_kind, Arena2Library, Climate, LocationDefinition, RmbBlock, Season,
    RMB_BLOCK_SIDE,
};
use crate::camera::OrbitCamera;
use crate::custom_names::CustomNames;
use crate::exterior::ClimateTextures;
use crate::picking::{intersect_horizontal_plane, pick_object, PickOptions};
use crate::render::{FrameRequest, RoomOutline, ViewSettings, ViewportTarget};
use crate::scene::{Scene, SceneObject};

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
}

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
}

impl ExteriorView {
    /// Loads the settlement `location` (at `location_index` in the library's
    /// list), drawn in `climate`, the climate the game gives it.
    pub fn settlement(
        location_index: usize,
        location: &LocationDefinition,
        climate: Climate,
        library: &mut Arena2Library,
    ) -> (Self, Vec<String>) {
        let exterior = &location.exterior;
        Self::load(
            ExteriorSubject::Settlement(location_index),
            location.name.clone(),
            &exterior.block_names,
            exterior.width,
            climate,
            Some(climate),
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
        location_climate: Option<Climate>,
        library: &mut Arena2Library,
    ) -> (Self, Vec<String>) {
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
            .collect();
        let mut view = Self {
            subject,
            title,
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
        };
        view.rebuild_scene(library);
        view.frame(library);
        (view, problems)
    }

    /// Builds the world space scene and outlines from the cells, in the view's climate.
    fn rebuild_scene(&mut self, library: &mut Arena2Library) {
        let mut objects: Vec<SceneObject> = Vec::new();
        for (index, cell) in self.blocks.iter().enumerate() {
            let Some(block) = &cell.block else { continue };
            let scene = crate::exterior::cell_scene(block, self.climate, self.season, index, library);
            objects.extend(
                scene
                    .objects
                    .into_iter()
                    .map(|object| SceneObject { position: object.position + cell.origin, ..object }),
            );
        }
        // Ids must be unique in the whole scene.
        for (number, object) in objects.iter_mut().enumerate() {
            object.id = number as u32 + 1;
        }
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
        super::room_editor::view_settings_menu(ui, &mut self.view_settings);
    }

    /// Handles the view's keyboard shortcuts.
    pub fn handle_shortcuts(&mut self, context: &egui::Context, library: &mut Arena2Library) {
        let frame_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Home);
        if !context.wants_keyboard_input()
            && context.input_mut(|input| input.consume_shortcut(&frame_shortcut))
        {
            self.frame(library);
        }
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
        match &self.subject {
            ExteriorSubject::Settlement(_) => {
                let hint = match self.hovered_block.and_then(|index| self.blocks.get(index)) {
                    Some(cell) => format!(
                        "{} ({}) - double-click to open it",
                        custom_names.room_label(&cell.found_name),
                        rmb_block_kind(&cell.found_name)
                    ),
                    None => "Hover a cell to highlight it, double-click to open it".to_string(),
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
                self.hovered_block = pointer_position
                    .and_then(|pointer| self.block_under_pointer(&projection, pointer, resources.library));
            }
        }
        let is_settlement = matches!(self.subject, ExteriorSubject::Settlement(_));
        let highlighted = self.list_hovered_block.or(self.hovered_block).filter(|_| is_settlement);
        for (index, outline) in self.outlines.iter_mut().enumerate() {
            outline.highlighted = highlighted == Some(index);
        }
        let request = FrameRequest {
            scene: Some(&self.scene),
            camera: &self.camera,
            selected_object: None,
            hovered_object: None,
            selected_objects: &[],
            hovered_objects: &[],
            highlighted_group: highlighted,
            highlighted_groups: &[],
            selected_groups: &[],
            placement_preview: &[],
            highlighted_objects: &[],
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
        };
        paint_scene(ui, rect, resources, &mut self.target, &request);

        // The hovered cell's name next to the pointer.
        let hovered = self.hovered_block.filter(|_| is_settlement).and_then(|index| self.blocks.get(index));
        if let (Some(cell), Some(pointer)) = (hovered, response.hover_pos()) {
            let label = format!(
                "{} ({})\nDouble-click to open this cell",
                resources.custom_names.room_label(&cell.found_name),
                rmb_block_kind(&cell.found_name)
            );
            ui.painter_at(rect).text(
                pointer + egui::vec2(16.0, 16.0),
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::proportional(13.0),
                egui::Color32::WHITE,
            );
        }

        let double_clicked = response.double_clicked_by(egui::PointerButton::Primary);
        let cell = double_clicked.then_some(hovered).flatten()?;
        Some(ExteriorOpen::Cell(cell.found_name.clone()))
    }

    /// The cell under the pointer: the one owning the object hit, or else the
    /// one whose square the pointer is over.
    fn block_under_pointer(
        &self,
        projection: &ViewportProjection,
        pointer: egui::Pos2,
        library: &mut Arena2Library,
    ) -> Option<usize> {
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
            return Some(self.scene.objects[hit.object_index].group_index);
        }
        let point = ray.point_at(intersect_horizontal_plane(&ray, 0.0)?);
        self.blocks.iter().position(|cell| {
            let local = point - cell.origin;
            (0.0..RMB_BLOCK_SIDE).contains(&local.x) && (0.0..RMB_BLOCK_SIDE).contains(&local.z)
        })
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

            ui.label("Type").on_hover_text("The kind of location, which decides its icon on the travel map");
            ui.label(location.location_type.map_or("Unknown", |kind| kind.name()));
            ui.end_row();

            ui.label("Source").on_hover_text("Daggerfall's own locations are shown, not changed");
            ui.label("Daggerfall (read-only)");
            ui.end_row();

            ui.label("Map").on_hover_text("Its pixel on the world map (column, row)");
            ui.label(location.map_pixel.to_string());
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

        ui.add_space(6.0);
        let buildings: Vec<u8> =
            location.exterior.buildings.iter().map(|building| building.building_type).collect();
        show_buildings(ui, "settlement_buildings", &buildings);

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
        let buildings = block.map(|block| block.building_types.clone()).unwrap_or_default();
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
        });

        ui.add_space(6.0);
        show_buildings(ui, "outdoor_cell_buildings", &buildings);

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
