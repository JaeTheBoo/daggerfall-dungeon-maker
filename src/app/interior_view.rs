//! The inside of one of Daggerfall's buildings, read-only, laid out as
//! Daggerfall Unity lays it out: its rooms and furniture, its doors, its
//! people, and the markers where the player walks in and where quests put
//! people and items. Shown in the main window (opened from the Interiors
//! list) or in a window of its own (opened from a building in a settlement
//! or outdoor cell).
//!
//! Clicking an object selects it and shows what it is: a person's faction, a
//! marker's use, a piece of furniture's (shelves, containers, ladders), a
//! person's name and gender. The exits (the ways back outdoors) are boxed in orange and
//! can be clicked too; the one walking in lands the player at says so. The
//! properties panel tells what the building is (in a settlement: its name,
//! quality and faction), what it holds, its cell, and every settlement whose
//! cells use it, with the building's name there.

use eframe::{egui, egui_wgpu};
use glam::Vec3;

use super::viewport_tools::{
    aspect_ratio, handle_camera_input, paint_scene, size_in_pixels, EditorResources, ViewportProjection,
};
use crate::arena2::{
    building_type_name, faction_kind_name, guild_group_name, rmb_block_kind, social_group_name,
    Arena2Library, Climate, GameReferences, LocationBuilding, LocationDefinition, RmbInterior, Season,
    INDIVIDUAL_FACTION_KIND,
};
use crate::buildings::{
    arrival_exit, building_access, entrance_centers, furniture_use, interior_exits, interior_layout,
    interior_marker_name, interior_marker_use, landing_markers, people_leave_when_closed, shop_stock,
    FurnitureUse, InteriorExit, InteriorSummary,
};
use crate::camera::OrbitCamera;
use crate::exterior::ClimateTextures;
use crate::people::{person_details, Gender, PersonDetails, PersonName, PersonPlace};
use crate::picking::{pick_object, PickOptions};
use crate::render::{EntranceBox, EntranceKind, FrameRequest, ViewSettings, ViewportTarget};
use crate::scene::{Scene, SceneObject, SceneObjectKind, MARKER_TEXTURE_ARCHIVE};
use crate::unity_install::UnityText;

/// Which building's inside: an outdoor cell (RMB block) and the building's place in it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InteriorKey {
    pub block: String,
    pub record: usize,
}

impl InteriorKey {
    /// For example "TVRNAL01.RMB, building 4" (counting from 1).
    pub fn label(&self) -> String {
        format!("{}, building {}", self.block, self.record + 1)
    }
}

/// The building as it stands in one settlement: what the game makes of it there.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildingPlace {
    /// The settlement, by index in the library's location list.
    pub location_index: usize,
    pub location_name: String,
    pub region_index: usize,
    pub region_name: &'static str,
    /// The settlement's place in its region's list of locations.
    pub index_in_region: usize,
    /// The building's key there (see [`crate::people::building_key`]).
    pub building_key: i32,
    /// Its data there: name seed, faction and quality from the settlement's list.
    pub data: LocationBuilding,
    /// Its name there, if the game gives it one and the words are available.
    pub name: Option<String>,
}

/// A settlement whose cells use the building, and what the building is there.
#[derive(Debug, Clone, PartialEq)]
pub struct InteriorUse {
    pub location_index: usize,
    pub data: LocationBuilding,
    pub name: Option<String>,
    /// The building's key there.
    pub building_key: i32,
}

/// Something the user asked to open from the view's properties.
#[derive(Debug, Clone, PartialEq)]
pub enum InteriorOpen {
    /// The settlement at this index in the library's location list.
    Settlement(usize),
    /// The outdoor cell with this RMB block name.
    Cell(String),
}

/// State of the view.
pub struct InteriorView {
    pub key: InteriorKey,
    /// The settlement the building was opened from, if any.
    pub place: Option<BuildingPlace>,
    /// The building's type as its cell gives it.
    building_type: u8,
    interior: RmbInterior,
    summary: InteriorSummary,
    /// The climate its textures are drawn in (always in summer, as the game draws insides).
    climate: Climate,
    /// The climate of the settlement it was opened from.
    place_climate: Option<Climate>,
    scene: Scene,
    /// Whether its rooms and doors are stored by their floor and were raised to fit.
    raised: bool,
    /// The pieces of furniture the game makes something of, by object.
    furniture: Vec<(usize, FurnitureUse)>,
    /// Who each person is, by object.
    people: Vec<(usize, PersonDetails)>,
    /// The ways back outdoors.
    exits: Vec<InteriorExit>,
    /// For each exit, by how many of the building's doors walking in lands the player in front of it.
    arrivals: Vec<usize>,
    /// How many doors lead into the building from outside.
    entrance_count: usize,
    /// Whether the exits are boxed.
    show_exits: bool,
    camera: OrbitCamera,
    view_settings: ViewSettings,
    target: ViewportTarget,
    /// The selected object, or else the selected exit, and those under the pointer.
    selected: Option<usize>,
    selected_exit: Option<usize>,
    hovered: Option<usize>,
    hovered_exit: Option<usize>,
    last_pointer_position: Option<egui::Pos2>,
    /// The settlements using the building, worked out by the application.
    pub uses: Option<Vec<InteriorUse>>,
}

impl InteriorView {
    /// Loads the inside of `key`, drawn in `climate`; `place` is the
    /// settlement it is opened from, if any.
    /// Daggerfall Unity's word lists (`unity`) name its people.
    pub fn open(
        key: InteriorKey,
        place: Option<BuildingPlace>,
        climate: Climate,
        unity: Option<&UnityText>,
        library: &mut Arena2Library,
    ) -> Result<Self, String> {
        let block = library
            .rmb_block(&key.block)
            .map_err(|error| format!("{} could not be loaded: {error}", key.block))?;
        let building = block
            .buildings
            .get(key.record)
            .ok_or_else(|| format!("{} has no building {}", key.block, key.record + 1))?;
        let layout = interior_layout(&block, key.record, library);
        let (mut scene, raised) = (layout.scene, layout.raised);
        let building_type =
            place.as_ref().map_or(building.data.building_type, |place| place.data.building_type);
        let furniture: Vec<(usize, FurnitureUse)> = layout
            .props
            .iter()
            .filter_map(|&index| match scene.objects.get(index)?.kind {
                SceneObjectKind::Model { model_id, .. } => {
                    Some((index, furniture_use(model_id, building_type)?))
                }
                _ => None,
            })
            .collect();
        // People come last; each named by who they are.
        let references = library.references();
        let person_place = place.as_ref().map(|place| PersonPlace {
            region_index: place.region_index,
            index_in_region: place.index_in_region,
            building_key: place.building_key,
        });
        let first_person = scene.objects.len() - building.interior.people.len();
        let people: Vec<(usize, PersonDetails)> = building
            .interior
            .people
            .iter()
            .enumerate()
            .map(|(number, person)| {
                let details = person_details(
                    person.texture,
                    person.faction_id,
                    Gender::from_flags(person.flags),
                    person.record_offset,
                    person_place.ok_or(
                        "Made up where the person stands: open the building from a settlement built with its cell",
                    ),
                    unity,
                    &references,
                );
                (first_person + number, details)
            })
            .collect();
        for (index, details) in &people {
            if let Some(name) = details.name.text() {
                scene.objects[*index].name = name.to_string();
            }
        }
        let exits = interior_exits(&scene, library);
        let markers = landing_markers(&building.interior);
        let entrances = entrance_centers(&block, key.record, library);
        let mut arrivals = vec![0; exits.len()];
        for &door in &entrances {
            if let Some(exit) = arrival_exit(door, &markers, &exits) {
                arrivals[exit] += 1;
            }
        }
        let mut view = Self {
            building_type: building.data.building_type,
            summary: InteriorSummary::of(&building.interior),
            interior: building.interior.clone(),
            key,
            place_climate: place.as_ref().map(|_| climate),
            place,
            climate,
            scene,
            raised,
            furniture,
            people,
            exits,
            arrivals,
            entrance_count: entrances.len(),
            show_exits: true,
            camera: OrbitCamera::default(),
            view_settings: ViewSettings {
                show_grid: false,
                show_room_outline: false,
                ..ViewSettings::default()
            },
            target: ViewportTarget::default(),
            selected: None,
            selected_exit: None,
            hovered: None,
            hovered_exit: None,
            last_pointer_position: None,
            uses: None,
        };
        view.frame(library);
        Ok(view)
    }

    /// The building's type in the place it was opened from, else in its cell.
    fn building_type(&self) -> u8 {
        self.place.as_ref().map_or(self.building_type, |place| place.data.building_type)
    }

    /// Name and kind, for example "The Rusty Mug (Tavern, Daggerfall)".
    pub fn description(&self) -> String {
        let kind = building_type_name(self.building_type());
        match &self.place {
            Some(BuildingPlace { name: Some(name), location_name, .. }) => {
                format!("{name} ({kind}, {location_name})")
            }
            Some(BuildingPlace { location_name, .. }) => format!("{kind} in {location_name}"),
            None => format!("{kind}: {}", self.key.label()),
        }
    }

    /// The status bar text.
    pub fn status_text(&self) -> String {
        let hint = match (self.hovered.and_then(|index| self.scene.objects.get(index)), self.hovered_exit) {
            (Some(object), _) => format!("{} - click to select it", object.name),
            (None, Some(exit)) => format!("{} - click to select it", exit_name(exit)),
            (None, None) => "Click an object or an exit (orange) to see what it is".to_string(),
        };
        format!("Inside {}, read-only. {hint}", self.description())
    }

    /// Points the camera at the whole inside.
    fn frame(&mut self, library: &mut Arena2Library) {
        if let Some((minimum, maximum)) = self.scene.bounds(library) {
            self.camera.frame_bounds(minimum, maximum);
        }
    }

    /// Selects exit `index` and points the camera at it.
    fn select_exit(&mut self, index: usize) {
        let Some(exit) = self.exits.get(index) else { return };
        let around = Vec3::splat(EXIT_FRAME_HALF_SIZE);
        self.camera.frame_bounds(exit.center() - around, exit.center() + around);
        self.selected = None;
        self.selected_exit = Some(index);
    }

    /// Clears the selection.
    fn clear_selection(&mut self) {
        self.selected = None;
        self.selected_exit = None;
    }

    /// Frees the GPU images.
    pub fn release(&mut self, render_state: &egui_wgpu::RenderState) {
        self.target.release(render_state);
    }

    /// Adds the View menu entries.
    pub fn view_menu(&mut self, ui: &mut egui::Ui, library: &mut Arena2Library) {
        if ui.add(egui::Button::new("Frame interior").shortcut_text("Home")).clicked() {
            self.frame(library);
            ui.close_menu();
        }
        ui.separator();
        ui.checkbox(&mut self.show_exits, "Exits")
            .on_hover_text("Orange boxes round the exits: the ways back outdoors");
        super::room_editor::view_settings_menu(ui, &mut self.view_settings);
    }

    /// Handles the view's keyboard shortcuts: Home frames the inside, Escape clears the selection.
    pub fn handle_shortcuts(&mut self, context: &egui::Context, library: &mut Arena2Library) {
        if context.wants_keyboard_input() {
            return;
        }
        let frame = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Home);
        if context.input_mut(|input| input.consume_shortcut(&frame)) {
            self.frame(library);
        }
        if context.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.clear_selection();
        }
    }

    /// Draws the 3D view; clicking an object selects it.
    pub fn show_viewport(&mut self, ui: &mut egui::Ui, resources: &mut EditorResources<'_>) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        if handle_camera_input(&mut self.camera, ui, &response) {
            self.last_pointer_position = None;
        }
        let projection = ViewportProjection::new(&self.camera, rect);
        if !response.dragged() {
            let pointer = response.hover_pos();
            if pointer != self.last_pointer_position {
                self.last_pointer_position = pointer;
                (self.hovered, self.hovered_exit) = match pointer {
                    Some(pointer) => self.under_pointer(&projection, pointer, resources.library),
                    None => (None, None),
                };
            }
        }
        if response.clicked_by(egui::PointerButton::Primary) {
            self.selected = self.hovered;
            self.selected_exit = self.hovered_exit;
        }
        let exit_boxes: Vec<EntranceBox> = match self.show_exits {
            true => self
                .exits
                .iter()
                .enumerate()
                .map(|(index, exit)| EntranceBox {
                    corners: exit.corners(),
                    highlighted: Some(index) == self.hovered_exit || Some(index) == self.selected_exit,
                    kind: EntranceKind::Exit,
                })
                .collect(),
            false => Vec::new(),
        };
        let request = FrameRequest {
            scene: Some(&self.scene),
            camera: &self.camera,
            selected_object: self.selected,
            hovered_object: self.hovered,
            selected_objects: &[],
            hovered_objects: &[],
            highlighted_group: None,
            highlighted_groups: &[],
            selected_groups: &[],
            placement_preview: &[],
            highlighted_objects: &[],
            context_scene: None,
            water_planes: &[],
            room_outlines: &[],
            settings: &self.view_settings,
            grid_height: 0.0,
            snap_grid_spacing: None,
            size_in_pixels: size_in_pixels(ui, rect),
            sky: None,
            dungeon_textures: None,
            climate_textures: Some(ClimateTextures::new(self.climate, Season::Summer)),
            warned_objects: &[],
            entrance_boxes: &exit_boxes,
            lighting: match self.view_settings.game_lighting {
                true => crate::lighting::GameLighting::Interior,
                false => crate::lighting::GameLighting::Off,
            },
        };
        paint_scene(ui, rect, resources, &mut self.target, &request);

        let hovered_name =
            match (self.hovered.and_then(|index| self.scene.objects.get(index)), self.hovered_exit) {
                (Some(object), _) => Some(object.name.clone()),
                (None, Some(exit)) => Some(exit_name(exit)),
                (None, None) => None,
            };
        if let (Some(name), Some(pointer)) = (hovered_name, response.hover_pos()) {
            ui.painter_at(rect).text(
                pointer + egui::vec2(16.0, 16.0),
                egui::Align2::LEFT_TOP,
                name,
                egui::FontId::proportional(13.0),
                egui::Color32::WHITE,
            );
        }
    }

    /// The object or else the exit under the pointer, if any. An exit is
    /// part of a wall, so it wins over the wall it is drawn on.
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
            include_flats: settings.show_flats || settings.show_markers,
            include_lights: false,
            camera_right: self.camera.right(),
        };
        let object = pick_object(&self.scene, library, &ray, &options);
        let exit = self
            .exits
            .iter()
            .enumerate()
            .filter(|_| self.show_exits)
            .filter_map(|(index, exit)| exit.hit_distance(&ray).map(|distance| (index, distance)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match (object, exit) {
            (Some(hit), Some((_, distance))) if distance > hit.distance + EXIT_PICK_SLACK => {
                (Some(hit.object_index), None)
            }
            (_, Some((exit, _))) => (None, Some(exit)),
            (object, None) => (object.map(|hit| hit.object_index), None),
        }
    }

    /// Draws the properties: the building, what it holds, the selected
    /// object and where the building is used. Returns what the user asked to open.
    pub fn show_properties(
        &mut self,
        ui: &mut egui::Ui,
        locations: &[LocationDefinition],
        references: &GameReferences,
    ) -> Option<InteriorOpen> {
        let mut action = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if let Some(index) = self.selected.filter(|&index| index < self.scene.objects.len()) {
                let mut clear = false;
                ui.horizontal(|ui| {
                    ui.strong("Selected object");
                    clear = ui.small_button("Clear").on_hover_text("Clear the selection (Esc)").clicked();
                });
                object_rows(
                    ui,
                    "interior_object",
                    &self.scene.objects[index],
                    references,
                    ObjectPlace::Interior,
                    self.person(index),
                );
                if let Some(furniture) = self.furniture_of(index) {
                    egui::Grid::new("interior_furniture_use").num_columns(2).spacing([8.0, 6.0]).show(
                        ui,
                        |ui| {
                            ui.label("Furniture").on_hover_text(
                                "What Daggerfall Unity makes of it, by its model and the kind of building \
                             (DaggerfallInterior.AddFurnitureAction)",
                            );
                            ui.label(furniture.name());
                            ui.end_row();
                            ui.label("Use");
                            ui.add(egui::Label::new(furniture.description()).wrap());
                            ui.end_row();
                        },
                    );
                }
                self.exits_of_object(ui, index);
                if clear {
                    self.clear_selection();
                }
                ui.separator();
            } else if let Some(index) = self.selected_exit.filter(|&index| index < self.exits.len()) {
                self.selected_exit_rows(ui, index);
                ui.separator();
            }
            self.building_rows(ui, references, &mut action);
            ui.add_space(6.0);
            self.contents(ui, references);
            ui.add_space(6.0);
            self.uses_list(ui, locations, &mut action);
        });
        action
    }

    /// The building: name, type, quality, faction, settlement, cell and climate.
    fn building_rows(
        &mut self,
        ui: &mut egui::Ui,
        references: &GameReferences,
        action: &mut Option<InteriorOpen>,
    ) {
        egui::Grid::new("interior_properties").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            if let Some(place) = &self.place {
                ui.label("Name").on_hover_text(
                    "What Daggerfall Unity calls the building in this settlement, from its name seed, type \
                     and faction",
                );
                match &place.name {
                    Some(name) => ui.strong(name),
                    None => ui.label(egui::RichText::new(no_name_reason(place.data.building_type)).weak()),
                };
                ui.end_row();
            }
            ui.label("Type").on_hover_text("The kind of building");
            ui.label(building_type_name(self.building_type()));
            ui.end_row();

            if let Some(place) = &self.place {
                ui.label("Quality").on_hover_text("How good the building is (1 to 20): a shop's wares and prices");
                ui.label(format!("{} of 20", place.data.quality));
                ui.end_row();

                ui.label("Faction").on_hover_text("The faction the building belongs to (FACTION.TXT)");
                ui.label(faction_label(place.data.faction_id, references));
                ui.end_row();
                faction_groups_row(ui, place.data.faction_id, references);

                ui.label("Settlement").on_hover_text("The settlement the building was opened from. Click to open it.");
                let label = format!("{} · {}", place.location_name, place.region_name);
                if ui.link(label).clicked() {
                    *action = Some(InteriorOpen::Settlement(place.location_index));
                }
                ui.end_row();
            }

            let faction = self.place.as_ref().map_or(0, |place| place.data.faction_id);
            ui.label("Open").on_hover_text(
                "When the player can walk in without picking the lock, as Daggerfall Unity decides it \
                 (PlayerActivate.BuildingIsUnlocked). Any building is open to its owner, and a residence \
                 while a quest sends the player there.",
            );
            ui.add(egui::Label::new(building_access(self.building_type(), faction).describe()).wrap());
            ui.end_row();
            shop_stock_row(ui, self.building_type(), self.place.as_ref().map(|place| place.data.quality));
            if people_leave_when_closed(self.building_type()) && !self.interior.people.is_empty() {
                ui.label("People").on_hover_text(
                    "Daggerfall Unity takes the people out of a closed shop, and out of other buildings that \
                     keep hours while they are closed (not for members of the guild a guild hall or house \
                     belongs to)",
                );
                ui.label("Away while it is closed");
                ui.end_row();
            }
            if let Some(place) = &self.place {
                ui.label("Building key").on_hover_text(
                    "How quests and saved games refer to the building in this settlement \
                     (BuildingDirectory.MakeBuildingKey): its cell's column and row on the settlement's \
                     grid, and its place in the cell",
                );
                ui.label(building_key_label(place.building_key));
                ui.end_row();
            }

            ui.label("Cell").on_hover_text(
                "The outdoor cell (RMB block) the building belongs to, and its place in the cell. Click to open the cell.",
            );
            let cell = format!("{} ({})", self.key.label(), rmb_block_kind(&self.key.block));
            if ui.link(cell).clicked() {
                *action = Some(InteriorOpen::Cell(self.key.block.clone()));
            }
            ui.end_row();

            if self.raised {
                ui.label("Corrected").on_hover_text(
                    "This building stores its rooms and doors by their floor, as furniture and flats are stored, \
                     rather than by their middle as every other building does",
                );
                ui.add(
                    egui::Label::new(
                        "Rooms and doors raised 1.6 m (half a storey) to meet the furniture and the doors outside. \
                         Daggerfall Unity lays them out as stored, with the furniture floating above the floors.",
                    )
                    .wrap(),
                );
                ui.end_row();
            }

            ui.label("Climate").on_hover_text(
                "The climate decides some of the textures inside, as the game draws them where the building \
                 stands (always as in summer). Choose one to see the inside in it.",
            );
            let mut chosen = self.climate;
            egui::ComboBox::from_id_salt("interior_climate").selected_text(chosen.name).show_ui(ui, |ui| {
                for climate in Climate::ALL {
                    let label = match Some(climate) == self.place_climate {
                        true => format!("{} (its own)", climate.name),
                        false => climate.name.to_string(),
                    };
                    ui.selectable_value(&mut chosen, climate, label);
                }
            });
            self.climate = chosen;
            ui.end_row();

            ui.label("Lighting").on_hover_text(
                "Draws the inside with its lamps as Daggerfall Unity lights them: dim but for each lamp's \
                 light, its reach, brightness and colour set by its image (also View > Game lighting)",
            );
            ui.checkbox(&mut self.view_settings.game_lighting, "As in the game");
            ui.end_row();
        });
    }

    /// What the building holds, with its people and markers listed to select.
    fn contents(&mut self, ui: &mut egui::Ui, references: &GameReferences) {
        let summary = self.summary;
        let exit_count = self.exits.len();
        egui::Grid::new("interior_contents").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            let mut row = |label: &str, hint: &str, value: usize| {
                ui.label(label).on_hover_text(hint);
                ui.label(value.to_string());
                ui.end_row();
            };
            row("Models", "Walls, floors and furniture", summary.models);
            row("Flats", "Sprites: lights, clutter, decoration", summary.flats);
            row("People", "Shopkeepers, guild members, quest givers", summary.people);
            row(
                "Inside doors",
                "Doors between rooms, which open and close; none of them leads outdoors",
                summary.doors,
            );
            row(
                "Exits",
                "The ways back outdoors: faces of the walls drawn as doors, boxed in orange",
                exit_count,
            );
            row("Enter markers", "Where the player appears on walking in", summary.enter_markers);
            row(
                "Quest markers",
                "Where quests put the people and enemies they place here",
                summary.quest_markers,
            );
            row(
                "Quest item markers",
                "Where quests put the items they place here",
                summary.quest_item_markers,
            );
            row(
                "Treasure markers",
                "Piles of loot in taverns and some guilds' buildings",
                summary.treasure_markers,
            );
            row("Spawn points", "Points the game can place enemies at inside", summary.spawn_points);
        });

        self.exits_list(ui);
        self.furniture_list(ui);
        let people: Vec<usize> = self.people.iter().map(|(index, _)| *index).collect();
        egui::CollapsingHeader::new(format!("People ({})", people.len())).id_salt("interior_people").show(
            ui,
            |ui| {
                if people.is_empty() {
                    ui.label(egui::RichText::new("Nobody").weak());
                }
                for index in people {
                    let object = &self.scene.objects[index];
                    let faction = match object.kind {
                        SceneObjectKind::Flat { data, .. } => {
                            faction_label(data.faction_or_mobile_id, references)
                        }
                        _ => String::new(),
                    };
                    let gender = self.person(index).map_or("", |person| person.gender.name());
                    let label = format!("{} · {gender} · {faction}", object.name);
                    if ui
                        .selectable_label(self.selected == Some(index), label)
                        .on_hover_text("Click to select")
                        .clicked()
                    {
                        self.selected = Some(index);
                    }
                }
            },
        );
        let markers: Vec<usize> = (0..self.scene.objects.len())
            .filter(|&index| {
                matches!(self.scene.objects[index].kind, SceneObjectKind::Flat { texture, .. }
                    if texture.archive == MARKER_TEXTURE_ARCHIVE && texture.record != UNUSED_MARKER_RECORD)
            })
            .collect();
        egui::CollapsingHeader::new(format!("Markers ({})", markers.len())).id_salt("interior_markers").show(
            ui,
            |ui| {
                for index in markers {
                    let object = &self.scene.objects[index];
                    if ui.selectable_label(self.selected == Some(index), &object.name).clicked() {
                        self.selected = Some(index);
                    }
                }
            },
        );
    }

    /// The exits, listed to select, with whether to box them.
    fn exits_list(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(format!("Exits ({})", self.exits.len()))
            .id_salt("interior_exits")
            .default_open(true)
            .show(ui, |ui| {
                ui.checkbox(&mut self.show_exits, "Show exit boxes")
                    .on_hover_text("Orange boxes round the exits in the view (also View > Exits)");
                if self.exits.is_empty() {
                    ui.label(egui::RichText::new("None: the building cannot be left").weak());
                }
                let mut chosen = None;
                for (index, exit) in self.exits.iter().enumerate() {
                    let mut label =
                        format!("{} · {}", exit_name(index), self.scene.objects[exit.object].name);
                    let mut hint = String::from("Click to select it and point the camera at it");
                    if self.arrivals[index] > 0 {
                        label.push_str(" · arrival");
                        hint.insert_str(0, "Walking in lands the player in front of it.\n");
                    }
                    if ui
                        .selectable_label(self.selected_exit == Some(index), label)
                        .on_hover_text(hint)
                        .clicked()
                    {
                        chosen = Some(index);
                    }
                }
                if let Some(index) = chosen {
                    self.select_exit(index);
                }
            });
    }

    /// The selected exit: what it is part of, whether walking in lands there, where it is.
    fn selected_exit_rows(&mut self, ui: &mut egui::Ui, index: usize) {
        let (mut clear, mut frame, mut select_model) = (false, false, false);
        ui.horizontal(|ui| {
            ui.strong("Selected exit");
            clear = ui.small_button("Clear").on_hover_text("Clear the selection (Esc)").clicked();
            frame = ui.small_button("Frame").on_hover_text("Point the camera at it").clicked();
        });
        let exit = self.exits[index];
        egui::Grid::new("interior_exit").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Name");
            ui.label(exit_name(index));
            ui.end_row();
            ui.label("Part of")
                .on_hover_text("The model whose faces drawn as a door make the exit. Click to select it.");
            select_model = ui.link(&self.scene.objects[exit.object].name).clicked();
            ui.end_row();
            ui.label("Use");
            ui.add(
                egui::Label::new("Takes the player back outdoors, out of the building's door nearest to them")
                    .wrap(),
            );
            ui.end_row();
            if self.entrance_count > 0 {
                ui.label("Arrival").on_hover_text(
                    "Walking in by a door, Daggerfall Unity lands the player in front of the exit nearest to the \
                     enter marker nearest to that door",
                );
                ui.add(egui::Label::new(arrival_text(self.arrivals[index], self.entrance_count)).wrap());
                ui.end_row();
            }
            let center = exit.center();
            ui.label("Position").on_hover_text("Where the exit's middle is, in metres");
            ui.label(format!("{:.2}, {:.2}, {:.2}", center.x, center.y, center.z));
            ui.end_row();
        });
        if clear {
            self.clear_selection();
        } else if frame {
            self.select_exit(index);
        } else if select_model {
            self.selected = Some(exit.object);
            self.selected_exit = None;
        }
    }

    /// For a selected model that exits are part of, a row of them to select.
    fn exits_of_object(&mut self, ui: &mut egui::Ui, object: usize) {
        let exits: Vec<usize> =
            (0..self.exits.len()).filter(|&index| self.exits[index].object == object).collect();
        if exits.is_empty() {
            return;
        }
        let mut chosen = None;
        ui.horizontal_wrapped(|ui| {
            ui.label("Exits").on_hover_text(
                "Its faces drawn as a door, which lead back outdoors. Click one to select it.",
            );
            for &index in &exits {
                if ui.link(exit_name(index)).clicked() {
                    chosen = Some(index);
                }
            }
        });
        if let Some(index) = chosen {
            self.select_exit(index);
        }
    }

    /// Who object `index` is, if it is one of the building's people.
    fn person(&self, index: usize) -> Option<&PersonDetails> {
        self.people.iter().find(|(object, _)| *object == index).map(|(_, details)| details)
    }

    /// What the game makes of object `index`, if it is furniture it makes something of.
    fn furniture_of(&self, index: usize) -> Option<FurnitureUse> {
        self.furniture.iter().find(|(object, _)| *object == index).map(|(_, furniture)| *furniture)
    }

    /// The furniture the player can use, listed to select.
    fn furniture_list(&mut self, ui: &mut egui::Ui) {
        let usable: Vec<(usize, FurnitureUse)> = self
            .furniture
            .iter()
            .copied()
            .filter(|(_, furniture)| *furniture != FurnitureUse::EmptyShelf)
            .collect();
        egui::CollapsingHeader::new(format!("Furniture ({})", usable.len()))
            .id_salt("interior_furniture")
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Shelves, containers and ladders the player can use").weak());
                if usable.is_empty() {
                    ui.label(egui::RichText::new("None").weak());
                }
                for (index, furniture) in usable {
                    let label = format!("{} · {}", furniture.name(), self.scene.objects[index].name);
                    if ui
                        .selectable_label(self.selected == Some(index), label)
                        .on_hover_text(furniture.description())
                        .clicked()
                    {
                        self.selected = Some(index);
                        self.selected_exit = None;
                    }
                }
            });
    }

    /// The settlements whose cells use the building, with its name in each.
    fn uses_list(
        &self,
        ui: &mut egui::Ui,
        locations: &[LocationDefinition],
        action: &mut Option<InteriorOpen>,
    ) {
        let Some(uses) = &self.uses else { return };
        ui.strong(format!("Used in ({})", uses.len())).on_hover_text(
            "Every settlement built with this building's cell, and what the building is there. Click one to open it.",
        );
        if uses.is_empty() {
            ui.label(egui::RichText::new("No settlement uses its cell").weak());
        }
        for building_use in uses {
            let Some(location) = locations.get(building_use.location_index) else { continue };
            let name = building_use
                .name
                .clone()
                .unwrap_or_else(|| building_type_name(building_use.data.building_type));
            let label = format!("{name} · {} · {}", location.name, location.region_name);
            if ui
                .selectable_label(false, label)
                .on_hover_text(format!(
                    "Quality {} of 20\nBuilding key {}\nClick to open {}",
                    building_use.data.quality,
                    building_key_label(building_use.building_key),
                    location.name
                ))
                .clicked()
            {
                *action = Some(InteriorOpen::Settlement(building_use.location_index));
            }
        }
    }
}

/// Half the size of the box the camera frames round a selected exit, in metres.
const EXIT_FRAME_HALF_SIZE: f32 = 2.0;
/// How far behind the object under the pointer an exit may be and still be
/// picked first, in metres: its box stands a little out from its wall.
const EXIT_PICK_SLACK: f32 = 0.25;

/// A building key and what it is made of, for example "131331 (column 2, row 1, building 4)".
pub fn building_key_label(key: i32) -> String {
    let (column, row, record) = (key >> 16 & 0xff, key >> 8 & 0xff, key & 0xff);
    format!("{key} (column {column}, row {row}, building {})", record + 1)
}

/// What exit `index` is called, counting from 1.
fn exit_name(index: usize) -> String {
    format!("Exit {}", index + 1)
}

/// Whether walking in lands the player in front of an exit, which it does by `arrivals` of the building's `doors` doors.
fn arrival_text(arrivals: usize, doors: usize) -> String {
    match (arrivals, doors) {
        (0, _) => "No: walking in lands the player at another exit".to_string(),
        (_, 1) => "Yes: walking in lands the player in front of it".to_string(),
        (arrivals, doors) if arrivals == doors => {
            format!("Yes: walking in by any of the building's {doors} doors lands the player in front of it")
        }
        (arrivals, doors) => format!(
            "Yes, by {arrivals} of the building's {doors} doors: walking in by them lands the player in front of it"
        ),
    }
}

/// The "random flat" marker, which Daggerfall Unity has no use for, found in most
/// buildings; left out of the markers list.
const UNUSED_MARKER_RECORD: u8 = 20;

/// Why a building has no name: the game names only some kinds, and needs Daggerfall Unity's words.
fn no_name_reason(building_type: u8) -> &'static str {
    match crate::buildings::is_named_building(building_type) {
        true => "Needs Daggerfall Unity's folder (File > Set Daggerfall Unity folder…)",
        false => "None: the game does not name this kind of building",
    }
}

/// A faction's id, name and kind, for example "357 · Gortwog (Individual)"; "None" for 0.
pub fn faction_label(faction_id: u16, references: &GameReferences) -> String {
    match (faction_id, references.factions.get(&faction_id)) {
        (0, _) => "None".to_string(),
        (_, Some(faction)) => {
            format!("{faction_id} · {} ({})", faction.name, faction_kind_name(faction.kind))
        }
        (_, None) => format!("{faction_id} · not a faction of FACTION.TXT"),
    }
}

/// Where an object shown with [`object_rows`] is, which changes what its markers mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectPlace {
    /// Inside a building.
    Interior,
    /// Outdoors, in a settlement or outdoor cell.
    Outdoors,
}

/// Rows describing `object`: what it is, its image or model, a person's
/// faction, a marker's use and where it is.
pub fn object_rows(
    ui: &mut egui::Ui,
    id: &str,
    object: &SceneObject,
    references: &GameReferences,
    place: ObjectPlace,
    person: Option<&PersonDetails>,
) {
    egui::Grid::new(id).num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label("Name");
        ui.label(&object.name);
        ui.end_row();
        match object.kind {
            SceneObjectKind::Model { model_id, .. } => {
                ui.label("Model id").on_hover_text("Number of the 3D model in ARCH3D.BSA");
                ui.label(model_id.to_string());
                ui.end_row();
            }
            SceneObjectKind::Flat { texture, data } => {
                ui.label("Texture").on_hover_text("The image the flat shows: its TEXTURE.### archive and record");
                ui.label(texture.to_string());
                ui.end_row();
                if texture.archive == MARKER_TEXTURE_ARCHIVE {
                    let (name, use_) = match place {
                        ObjectPlace::Interior => (interior_marker_name(texture.record), interior_marker_use(texture.record)),
                        ObjectPlace::Outdoors => outdoor_marker(texture.record),
                    };
                    ui.label("Marker").on_hover_text("Markers are invisible in the game; they tell it where things go");
                    ui.label(name);
                    ui.end_row();
                    ui.label("Use");
                    ui.add(egui::Label::new(use_).wrap());
                    ui.end_row();
                } else if data.faction_or_mobile_id != 0 {
                    ui.label("Faction").on_hover_text(
                        "The faction a person belongs to. Daggerfall Unity makes a flat with a faction a person \
                         to talk to; one of the individual kind is that named person, whom quests use.",
                    );
                    ui.add(egui::Label::new(faction_label(data.faction_or_mobile_id, references)).wrap());
                    ui.end_row();
                    let individual = references
                        .factions
                        .get(&data.faction_or_mobile_id)
                        .is_some_and(|faction| faction.kind == INDIVIDUAL_FACTION_KIND);
                    if individual {
                        ui.label("Person");
                        ui.label("A named person, whom quests can use");
                        ui.end_row();
                    }
                    if let Some(person) = person {
                        person_rows(ui, person);
                    }
                }
            }
            SceneObjectKind::Light { radius, .. } => {
                ui.label("Radius");
                ui.label(format!("{radius:.1} m"));
                ui.end_row();
            }
        }
        let position = object.position;
        ui.label("Position").on_hover_text("Where the object is, in metres");
        ui.label(format!("{:.2}, {:.2}, {:.2}", position.x, position.y, position.z));
        ui.end_row();
    });
}

/// Who a person is: their name, gender, what their image shows, and whether they are a child.
pub(crate) fn person_rows(ui: &mut egui::Ui, person: &PersonDetails) {
    ui.label("Called").on_hover_text(
        "A named person keeps their faction's name; anyone else gets a name Daggerfall Unity makes up from \
         the region's name bank, seeded by where their record is, the building and the settlement",
    );
    match &person.name {
        PersonName::Named(name) => ui.strong(name),
        PersonName::MadeUp(name) => ui.label(name),
        PersonName::Unknown(why) => ui.add(egui::Label::new(egui::RichText::new(*why).weak()).wrap()),
    };
    ui.end_row();
    ui.label("Gender").on_hover_text(
        "Bit 32 of the person's flags makes a woman; outdoors in settlements, their image's record in \
         FLATS.CFG decides instead",
    );
    ui.label(person.gender.name());
    ui.end_row();
    ui.label("Race").on_hover_text(
        "Their faction's race, or else the people of the region's (StaticNPC.GetRaceFromFaction)",
    );
    ui.label(person.race.describe());
    ui.end_row();
    if let Some((social, guild)) = person.groups {
        ui.label("Social group").on_hover_text(
            "Their faction's social group, which decides how they talk and react to the player",
        );
        ui.label(social_group_name(social));
        ui.end_row();
        ui.label("Guild group").on_hover_text(
            "Their faction's guild group, which decides which guild's services and quests they give",
        );
        ui.add(egui::Label::new(guild_group_name(guild)).wrap());
        ui.end_row();
    }
    if let Some(caption) = &person.caption {
        ui.label("Shows").on_hover_text("What FLATS.CFG says the person's image shows");
        ui.label(caption);
        ui.end_row();
    }
    if person.child {
        ui.label("Child").on_hover_text("A child's image, or the children's faction (514)");
        ui.label("Yes");
        ui.end_row();
    }
}

/// A row of the social and guild groups of faction `faction_id`, if it is in `FACTION.TXT`.
pub fn faction_groups_row(ui: &mut egui::Ui, faction_id: u16, references: &GameReferences) {
    let Some(faction) = references.factions.get(&faction_id).filter(|_| faction_id != 0) else { return };
    ui.label("Groups").on_hover_text(
        "The faction's social group (how its people talk and react to the player) and guild group (which \
         guild's services and quests they give)",
    );
    let text =
        format!("{} · {}", social_group_name(faction.social_group), guild_group_name(faction.guild_group));
    ui.add(egui::Label::new(text).wrap());
    ui.end_row();
}

/// A row of what a shop of `building_type` sells, if it is a shop; `quality`
/// is the shop's in the settlement it stands in, if known.
pub fn shop_stock_row(ui: &mut egui::Ui, building_type: u8, quality: Option<u8>) {
    let Some(stock) = shop_stock(building_type) else { return };
    let chances: Vec<String> = stock.groups.iter().map(|(name, chance)| format!("{name} {chance}")).collect();
    let quality = match quality {
        Some(quality) => format!("this shop's quality, {quality} of 20"),
        None => "the shop's quality in its settlement".to_string(),
    };
    ui.label("Sells").on_hover_text(format!(
        "What Daggerfall Unity stocks the shelves with (DaggerfallLoot.StockShopShelf): items no rarer than \
         {quality}, each of a group stocked chance × 5 × (21 − rarity) / 100 times in 100.\nChances: {}",
        if chances.is_empty() { "none".to_string() } else { chances.join(", ") }
    ));
    ui.add(egui::Label::new(stock.describe()).wrap());
    ui.end_row();
}

/// What a marker outdoors is and what the game does with it.
fn outdoor_marker(record: u8) -> (&'static str, &'static str) {
    match record {
        10 => (
            "Start marker",
            "An arrival point: fast travel to the settlement puts the player at the start marker nearest to \
             where they come in",
        ),
        8 => ("Enter marker", "Daggerfall Unity has no use for an enter marker outdoors"),
        _ => ("Editor marker", "Daggerfall Unity has no use for this marker outdoors"),
    }
}
