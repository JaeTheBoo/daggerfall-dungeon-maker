//! The asset palette: a browsable list of models, flats, markers and lights
//! that can be placed into the open room.
//!
//! Clicking a tile "arms" that asset. While an asset is armed the viewport
//! shows a ghost preview under the mouse, and a left click places a copy.
//! Model tiles are captioned with the model's display name and can be
//! renamed by right-clicking them.
//!
//! Any asset can be marked as a favourite with the star in its tile's corner
//! (or its right-click menu); the **Favorites** tab lists them all, whatever
//! their kind. Favourites are saved with the user's names (see [`CustomNames`]).
//!
//! The **Prefabs** tab lists the user's prefabs (see [`crate::prefabs`]).
//! Clicking one arms all of its objects, to place them together; its
//! right-click menu renames it or deletes it from the list.

use std::collections::HashMap;

use eframe::egui;
use glam::Vec3;

use crate::app::viewport_tools::EditorResources;
use crate::arena2::{Arena2Library, TextureKey};
use crate::catalog::{flat_description, AssetCatalog, FlatAsset};
use crate::custom_models::CustomModelLibrary;
use crate::custom_names::{CustomNames, Favorite};
use crate::prefabs::{PrefabId, PrefabLibrary};
use crate::render::ThumbnailStatus;
use crate::scene::{
    default_model_object_name, FlatData, LightData, ModelData, SceneObject, SceneObjectKind,
    ADDED_OBJECTS_GROUP,
};

/// Maximum number of thumbnails created per frame, to keep scrolling smooth.
const THUMBNAILS_PER_FRAME: usize = 4;
/// Height reserved under each tile for its caption, in points.
const CAPTION_HEIGHT: f32 = 16.0;
/// Gap between tiles, in points.
const TILE_SPACING: f32 = 6.0;
/// Colour of a favourite's star.
const FAVORITE_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 200, 60);
/// Marks prefabs in the palette and the outliner.
pub const PREFAB_MARK: &str = "▣";
/// Colour of prefabs: their mark, and the box drawn around a placed one.
pub const PREFAB_COLOR: egui::Color32 = egui::Color32::from_rgb(90, 235, 120);
/// Radius given to newly placed lights, in world units (400 native units).
const DEFAULT_LIGHT_RADIUS: f32 = 10.0;
/// Height above the surface at which new lights are placed, in world units.
const LIGHT_PLACEMENT_HEIGHT: f32 = 1.5;

/// Something that can be placed in a room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceableAsset {
    Model {
        model_id: u32,
    },
    Flat {
        texture: TextureKey,
    },
    Light,
    /// One of the user's prefabs: all of its objects.
    Prefab {
        id: PrefabId,
    },
}

impl PlaceableAsset {
    /// The favourite that marks this asset; prefabs are the user's own, so they are not marked.
    pub fn favorite(self) -> Option<Favorite> {
        match self {
            Self::Model { model_id } => Some(Favorite::Model { model_id }),
            Self::Flat { texture } => {
                Some(Favorite::Flat { archive: texture.archive, record: texture.record })
            }
            Self::Light => Some(Favorite::Light),
            Self::Prefab { .. } => None,
        }
    }

    /// The asset a favourite marks.
    pub fn of_favorite(favorite: Favorite) -> Self {
        match favorite {
            Favorite::Model { model_id } => Self::Model { model_id },
            Favorite::Flat { archive, record } => Self::Flat { texture: TextureKey::new(archive, record) },
            Favorite::Light => Self::Light,
        }
    }

    /// Name given to objects created from this asset.
    ///
    /// Models get their default object name so they are listed under the
    /// model's display name, which follows the user's naming.
    pub fn name(&self) -> String {
        match self {
            Self::Model { model_id } => default_model_object_name(*model_id),
            Self::Flat { texture } => flat_description(*texture),
            Self::Light => "Light".to_string(),
            Self::Prefab { .. } => "Prefab".to_string(),
        }
    }

    /// Name shown for this asset in the palette: for models, the model's
    /// display name, and for prefabs, the prefab's name.
    pub fn label(
        &self,
        custom_names: &CustomNames,
        catalog: &AssetCatalog,
        prefabs: &PrefabLibrary,
    ) -> String {
        match self {
            Self::Model { model_id } => custom_names.model_label(*model_id, catalog),
            Self::Prefab { id } => {
                prefabs.get(*id).map_or("Deleted prefab", |prefab| prefab.label()).to_string()
            }
            _ => self.name(),
        }
    }

    /// Creates the scene objects for this asset resting on `surface_point`,
    /// turned by `yaw_degrees`: one, or a prefab's objects (numbered from 0,
    /// marked as placed prefab 0; placing them renumbers both).
    ///
    /// Models are lifted so the bottom of their bounding box touches the
    /// surface, flats so their lower edge does, and lights float above it. A
    /// prefab's base rests on the surface.
    pub fn create_objects(
        &self,
        library: &mut Arena2Library,
        prefabs: &PrefabLibrary,
        surface_point: Vec3,
        yaw_degrees: f32,
    ) -> Vec<SceneObject> {
        let (kind, position, rotation_degrees) = match *self {
            Self::Prefab { id } => {
                return prefabs
                    .get(id)
                    .map(|prefab| prefab.place(surface_point, yaw_degrees, 0, 0))
                    .unwrap_or_default()
            }
            Self::Model { model_id } => {
                let lift = library.model(model_id).map_or(0.0, |mesh| -mesh.bounds_min.y);
                (
                    SceneObjectKind::Model { model_id, data: ModelData::default() },
                    surface_point + Vec3::Y * lift,
                    Vec3::new(0.0, yaw_degrees, 0.0),
                )
            }
            Self::Flat { texture } => {
                let half_height = library.billboard_world_size(texture).map_or(0.5, |size| size.y * 0.5);
                (
                    SceneObjectKind::Flat { texture, data: FlatData::default() },
                    surface_point + Vec3::Y * half_height,
                    Vec3::ZERO,
                )
            }
            Self::Light => (
                SceneObjectKind::Light { radius: DEFAULT_LIGHT_RADIUS, data: LightData::default() },
                surface_point + Vec3::Y * LIGHT_PLACEMENT_HEIGHT,
                Vec3::ZERO,
            ),
        };
        vec![SceneObject {
            // The cell gives the object its id when it is placed.
            id: 0,
            next: None,
            name: self.name(),
            group_index: ADDED_OBJECTS_GROUP,
            kind,
            position,
            rotation_degrees,
            visible: true,
            scale: None,
            prefab: None,
            folder: None,
        }]
    }
}

/// An asset chosen for placement, with the rotation the user has dialled in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArmedAsset {
    pub asset: PlaceableAsset,
    /// Rotation around the vertical axis applied to placed models, in degrees.
    pub yaw_degrees: f32,
}

/// The palette's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaletteTab {
    Favorites,
    Prefabs,
    DungeonModels,
    ExteriorModels,
    AllModels,
    Flats,
    AllFlats,
    Markers,
    Lights,
    CustomModels,
}

impl PaletteTab {
    const ALL: [Self; 10] = [
        Self::Favorites,
        Self::Prefabs,
        Self::DungeonModels,
        Self::ExteriorModels,
        Self::AllModels,
        Self::Flats,
        Self::AllFlats,
        Self::Markers,
        Self::Lights,
        Self::CustomModels,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Favorites => "★ Favorites",
            Self::Prefabs => "▣ Prefabs",
            Self::DungeonModels => "Dungeon Models",
            Self::ExteriorModels => "Exterior Models",
            Self::AllModels => "All Models",
            Self::Flats => "Flats",
            Self::AllFlats => "All Flats",
            Self::Markers => "Markers",
            Self::Lights => "Lights",
            Self::CustomModels => "Custom Models",
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            Self::Favorites => "The assets you marked with the star in a tile's corner, of every kind",
            Self::Prefabs => {
                "Your prefabs: groups of objects to place together. Select several objects in a cell and \
                 right-click them to create one; right-click a prefab here to rename or delete it"
            }
            Self::DungeonModels => "Models used in Daggerfall's dungeon cells, most common first",
            Self::ExteriorModels => {
                "Models Daggerfall uses outdoors: buildings, walls, scenery and dungeon entrances, most common first"
            }
            Self::AllModels => "Every model in ARCH3D.BSA",
            Self::Flats => "Billboard sprites used in dungeon cells: torches, furniture, people, treasure",
            Self::AllFlats => {
                "Every image of Daggerfall's flat archives, also those no dungeon cell uses: people, \
                 furniture, plants, statues, crops and trees. Search by archive name, for example Plants"
            }
            Self::Markers => {
                "Editor markers: start and enter points, enemy spawns, quest and treasure markers"
            }
            Self::Lights => "Point lights",
            Self::CustomModels => "Your own models, imported with Import > Import Model…",
        }
    }
}

/// One tile in the palette grid.
struct PaletteEntry {
    asset: PlaceableAsset,
    /// Name shown under the tile.
    caption: String,
    /// Extra information shown in the tooltip.
    details: String,
}

impl PaletteEntry {
    /// Builds the tile for `asset`, with `usage` describing where the game uses it.
    fn new(
        asset: PlaceableAsset,
        usage: String,
        custom_names: &CustomNames,
        catalog: &AssetCatalog,
        prefabs: &PrefabLibrary,
    ) -> Self {
        let details = match asset {
            PlaceableAsset::Model { model_id } => match catalog.model_description(model_id) {
                Some(description) => format!("Model {model_id}\n{}\n{usage}", description.summary()),
                None => format!("Model {model_id}\n{usage}"),
            },
            _ => usage,
        };
        Self { asset, caption: asset.label(custom_names, catalog, prefabs), details }
    }
}

/// State of the asset palette panel.
pub struct AssetPalette {
    /// Whether the panel is shown.
    pub is_open: bool,
    /// Asset currently armed for placement.
    pub armed: Option<ArmedAsset>,
    tab: PaletteTab,
    search_text: String,
    /// Edge length of each tile, in points.
    tile_size: f32,
    /// egui textures for flat thumbnails; `None` marks textures that failed to load.
    flat_thumbnails: HashMap<TextureKey, Option<crate::app::kept_texture::KeptTexture>>,
    /// What the user asked to do with one of their imported models, for the application to do.
    model_request: Option<CustomModelRequest>,
}

/// Something the user asked to do with one of their imported models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomModelRequest {
    /// Choose an image for part `part` of the model (`None`: every part without a texture).
    ChangeTexture { model_id: u32, part: Option<usize> },
    /// Delete the model.
    Delete { model_id: u32 },
}

impl Default for AssetPalette {
    fn default() -> Self {
        Self {
            is_open: true,
            armed: None,
            tab: PaletteTab::DungeonModels,
            search_text: String::new(),
            tile_size: 84.0,
            flat_thumbnails: HashMap::new(),
            model_request: None,
        }
    }
}

impl AssetPalette {
    /// What the user asked to do with one of their imported models, once.
    pub fn take_model_request(&mut self) -> Option<CustomModelRequest> {
        self.model_request.take()
    }

    /// Stops placing the model `model_id`, which was deleted.
    pub fn forget_model(&mut self, model_id: u32) {
        if self.armed.is_some_and(|armed| armed.asset == PlaceableAsset::Model { model_id }) {
            self.armed = None;
        }
    }

    /// Arms `asset` for placement, or disarms it if it is already armed.
    fn toggle_armed(&mut self, asset: PlaceableAsset) {
        self.armed = match self.armed {
            Some(armed) if armed.asset == asset => None,
            _ => Some(ArmedAsset { asset, yaw_degrees: 0.0 }),
        };
    }

    /// Shows the Custom Models tab, for example once a model was imported.
    pub fn show_custom_models(&mut self) {
        self.tab = PaletteTab::CustomModels;
    }

    /// Shows the Prefabs tab, for example once a prefab was made.
    pub fn show_prefabs(&mut self) {
        self.tab = PaletteTab::Prefabs;
    }

    /// Rotates the armed asset by `degrees` around the vertical axis.
    pub fn rotate_armed(&mut self, degrees: f32) {
        if let Some(armed) = self.armed.as_mut() {
            armed.yaw_degrees = (armed.yaw_degrees + degrees).rem_euclid(360.0);
        }
    }

    /// Draws the palette as a panel along the bottom of the window.
    ///
    /// `editor_id` keeps the panel's id unique when several editors are open.
    pub fn show(
        &mut self,
        context: &egui::Context,
        editor_id: egui::Id,
        resources: &mut EditorResources<'_>,
    ) {
        if !self.is_open {
            return;
        }

        egui::TopBottomPanel::bottom(editor_id.with("asset_palette"))
            .resizable(true)
            .default_height(240.0)
            .min_height(120.0)
            .show(context, |ui| {
                self.show_header(ui, resources);
                ui.separator();

                let entries = self.visible_entries(
                    resources.catalog,
                    resources.custom_names,
                    resources.prefabs,
                    resources.custom_models,
                );
                if entries.is_empty() && self.tab == PaletteTab::CustomModels {
                    let text = match resources.custom_models.models().is_empty() {
                        true => {
                            "No custom models yet. Import a model of your own (an OBJ, glTF, GLB or .blend file) \
                             with Import > Import Model…"
                        }
                        false => "No custom model matches the search.",
                    };
                    ui.label(egui::RichText::new(text).weak());
                    return;
                }
                if entries.is_empty() && self.tab == PaletteTab::Prefabs {
                    let text = match resources.prefabs.prefabs().is_empty() {
                        true => {
                            "No prefabs yet. Select several objects in a cell (Ctrl+click them, or drag a box \
                             around them), right-click them and choose Create Prefab."
                        }
                        false => "No prefab matches the search.",
                    };
                    ui.label(egui::RichText::new(text).weak());
                    return;
                }
                let mut thumbnail_budget = THUMBNAILS_PER_FRAME;
                let mut has_pending_thumbnails = false;

                let tile_size = self.tile_size;
                let tile_stride = tile_size + TILE_SPACING;
                let columns = ((ui.available_width() + TILE_SPACING) / tile_stride).floor().max(1.0) as usize;
                let row_count = entries.len().div_ceil(columns);

                egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
                    ui,
                    tile_size + CAPTION_HEIGHT,
                    row_count,
                    |ui, row_range| {
                        ui.spacing_mut().item_spacing = egui::vec2(TILE_SPACING, TILE_SPACING);
                        for row in row_range {
                            ui.horizontal(|ui| {
                                for entry in entries.iter().skip(row * columns).take(columns) {
                                    let thumbnail = self.thumbnail_for(
                                        ui.ctx(),
                                        entry.asset,
                                        resources,
                                        &mut thumbnail_budget,
                                    );
                                    has_pending_thumbnails |= thumbnail == TileImage::Pending;
                                    let favorite = entry.asset.favorite();
                                    let is_favorite =
                                        favorite.map(|favorite| resources.custom_names.is_favorite(favorite));
                                    let (response, star_clicked) =
                                        self.draw_tile(ui, entry, thumbnail, is_favorite);
                                    if let (true, Some(favorite)) = (star_clicked, favorite) {
                                        resources.custom_names.toggle_favorite(favorite);
                                    } else if response.clicked() {
                                        self.toggle_armed(entry.asset);
                                    }
                                    response.context_menu(|ui| {
                                        if let PlaceableAsset::Prefab { id } = entry.asset {
                                            if show_prefab_menu(ui, id, resources.prefabs) {
                                                resources.prefabs.remove(id);
                                                if self.armed.is_some_and(|armed| armed.asset == entry.asset) {
                                                    self.armed = None;
                                                }
                                                ui.close_menu();
                                            }
                                            return;
                                        }
                                        let Some(favorite) = favorite else { return };
                                        let text = match is_favorite {
                                            Some(true) => "☆ Remove from favorites",
                                            _ => "★ Add to favorites",
                                        };
                                        if ui.button(text).clicked() {
                                            resources.custom_names.toggle_favorite(favorite);
                                            ui.close_menu();
                                        }
                                        if let PlaceableAsset::Model { model_id } = entry.asset {
                                            ui.separator();
                                            show_model_name_menu(ui, model_id, resources.custom_names);
                                            if let Some(model) = resources.custom_models.get(model_id) {
                                                ui.separator();
                                                if let Some(part) = show_texture_menu(ui, model) {
                                                    self.model_request =
                                                        Some(CustomModelRequest::ChangeTexture { model_id, part });
                                                    ui.close_menu();
                                                }
                                                ui.separator();
                                                let hover = "Deletes the model from Custom Models, after asking. \
                                                             Copies placed in your project's cells and prefabs \
                                                             are removed too; the file it was imported from \
                                                             is not touched.";
                                                if ui.button("Delete model…").on_hover_text(hover).clicked() {
                                                    self.model_request = Some(CustomModelRequest::Delete { model_id });
                                                    ui.close_menu();
                                                }
                                            }
                                        }
                                    });
                                }
                            });
                        }
                    },
                );

                if has_pending_thumbnails {
                    ui.ctx().request_repaint();
                }
            });
    }

    /// Draws the tab bar, search box, tile size slider and placement hint.
    fn show_header(&mut self, ui: &mut egui::Ui, resources: &EditorResources<'_>) {
        ui.horizontal(|ui| {
            ui.strong("Assets");
            ui.separator();
            for tab in PaletteTab::ALL {
                ui.selectable_value(&mut self.tab, tab, tab.label()).on_hover_text(tab.tooltip());
            }
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.search_text)
                    .hint_text("Search: id, name, door, Torture…")
                    .desired_width(180.0),
            );
            ui.add(egui::Slider::new(&mut self.tile_size, 56.0..=140.0).show_value(false).text("Size"));
        });

        // The placement hint gets its own line so it is never cut off by the panel width.
        let hint_color = egui::Color32::from_rgb(110, 230, 140);
        match self.armed {
            Some(armed) => {
                ui.horizontal_wrapped(|ui| {
                    let asset_label =
                        armed.asset.label(resources.custom_names, resources.catalog, resources.prefabs);
                    ui.colored_label(hint_color, format!("Placing {asset_label} ({:.0}°)", armed.yaw_degrees));
                    ui.label(
                        egui::RichText::new(
                            "Click in the cell to place · Shift+click to keep placing · Q/E or R to turn · Alt: don't snap · Esc to cancel",
                        )
                        .weak(),
                    );
                    if ui.small_button("Cancel").clicked() {
                        self.armed = None;
                    }
                });
            }
            None => {
                let hint = match self.tab {
                    PaletteTab::Prefabs => {
                        "Click a prefab to start placing it in the cell · right-click it to rename or delete it"
                    }
                    _ => {
                        "Click an asset to start placing it in the cell · ☆ in a tile's corner marks it as a favorite · right-click a model to name it"
                    }
                };
                ui.label(egui::RichText::new(hint).weak());
            }
        }
    }

    /// Returns the tiles of the current tab that match the search text.
    fn visible_entries(
        &self,
        catalog: &AssetCatalog,
        custom_names: &CustomNames,
        prefabs: &PrefabLibrary,
        custom_models: &CustomModelLibrary,
    ) -> Vec<PaletteEntry> {
        let search = self.search_text.trim().to_lowercase();
        let matches = |text: &str| search.is_empty() || text.to_lowercase().contains(&search);
        // Models match on their id, the user's name for them, or their description.
        let model_matches = |model_id: u32| {
            matches(&model_id.to_string())
                || custom_names.model_name_matches(model_id, &search)
                || catalog.model_description(model_id).is_some_and(|description| description.matches(&search))
        };
        let entry = |asset, usage| PaletteEntry::new(asset, usage, custom_names, catalog, prefabs);
        let model_usage = |model_id: u32| match catalog.room_model(model_id) {
            Some(model) => format!("Used in {} cells", model.room_count),
            None => "Not used in any dungeon cell".to_string(),
        };

        match self.tab {
            PaletteTab::Favorites => custom_names
                .favorites()
                .iter()
                .map(|&favorite| PlaceableAsset::of_favorite(favorite))
                .filter_map(|asset| match asset {
                    PlaceableAsset::Prefab { .. } => None,
                    PlaceableAsset::Model { model_id } => {
                        model_matches(model_id).then(|| entry(asset, model_usage(model_id)))
                    }
                    PlaceableAsset::Flat { texture } => {
                        let flat = catalog
                            .flats
                            .iter()
                            .chain(&catalog.all_flats)
                            .chain(&catalog.markers)
                            .find(|flat| flat.texture == texture);
                        let usage = flat.map_or_else(|| texture.to_string(), FlatAsset::usage);
                        let found = match flat {
                            Some(flat) => flat.matches(&search),
                            None => matches(&flat_description(texture)),
                        };
                        found.then(|| entry(asset, usage))
                    }
                    PlaceableAsset::Light => matches("light").then(|| {
                        entry(
                            asset,
                            format!("Point light, radius {DEFAULT_LIGHT_RADIUS} m (editable after placing)"),
                        )
                    }),
                })
                .collect(),
            PaletteTab::Prefabs => prefabs
                .prefabs()
                .iter()
                .filter(|prefab| matches(prefab.label()))
                .map(|prefab| {
                    let usage = format!("Prefab of {} objects", prefab.objects.len());
                    entry(PlaceableAsset::Prefab { id: prefab.id }, usage)
                })
                .collect(),
            PaletteTab::DungeonModels => catalog
                .room_models
                .iter()
                .filter(|model| model_matches(model.model_id))
                .map(|model| {
                    let usage = format!(
                        "Used in {} cells, {} placements in total",
                        model.room_count, model.placement_count
                    );
                    entry(PlaceableAsset::Model { model_id: model.model_id }, usage)
                })
                .collect(),
            PaletteTab::ExteriorModels => catalog
                .exterior_models
                .iter()
                .filter(|model| model_matches(model.model_id))
                .map(|model| {
                    let usage = format!(
                        "Used outdoors in {} blocks, {} placements in total",
                        model.room_count, model.placement_count
                    );
                    entry(PlaceableAsset::Model { model_id: model.model_id }, usage)
                })
                .collect(),
            PaletteTab::CustomModels => custom_models
                .models()
                .iter()
                .filter(|model| model_matches(model.id) || matches(&model.name))
                .map(|model| {
                    let size = model.mesh.size();
                    let usage = format!(
                        "Your model, imported from {}\n{:.1} × {:.1} × {:.1} m",
                        model.source, size.x, size.y, size.z
                    );
                    entry(PlaceableAsset::Model { model_id: model.id }, usage)
                })
                .collect(),
            PaletteTab::AllModels => catalog
                .all_model_ids
                .iter()
                .filter(|&&model_id| model_matches(model_id))
                .map(|&model_id| entry(PlaceableAsset::Model { model_id }, model_usage(model_id)))
                .collect(),
            PaletteTab::Flats | PaletteTab::AllFlats | PaletteTab::Markers => {
                let flats = match self.tab {
                    PaletteTab::Flats => &catalog.flats,
                    PaletteTab::AllFlats => &catalog.all_flats,
                    _ => &catalog.markers,
                };
                flats
                    .iter()
                    .filter(|flat| flat.matches(&search))
                    .map(|flat| entry(PlaceableAsset::Flat { texture: flat.texture }, flat.usage()))
                    .collect()
            }
            PaletteTab::Lights => vec![entry(
                PlaceableAsset::Light,
                format!("Point light, radius {DEFAULT_LIGHT_RADIUS} m (editable after placing)"),
            )],
        }
    }

    /// Fetches (and if the budget allows, creates) the image shown on a tile.
    fn thumbnail_for(
        &mut self,
        context: &egui::Context,
        asset: PlaceableAsset,
        resources: &mut EditorResources<'_>,
        budget: &mut usize,
    ) -> TileImage {
        let library = &mut *resources.library;
        let asset = match asset {
            PlaceableAsset::Prefab { id } => {
                let model = resources.prefabs.get(id).and_then(|prefab| {
                    prefab.largest_model(|model_id| {
                        library.model(model_id).map_or(0.0, |mesh| mesh.bounding_radius())
                    })
                });
                match model {
                    Some(model_id) => PlaceableAsset::Model { model_id },
                    None => return TileImage::PrefabIcon,
                }
            }
            asset => asset,
        };
        match asset {
            PlaceableAsset::Prefab { .. } => TileImage::PrefabIcon,
            PlaceableAsset::Model { model_id } => {
                let (Some(render_state), Some(renderer)) =
                    (resources.render_state, resources.renderer.as_deref_mut())
                else {
                    return TileImage::None;
                };
                // Only thumbnails that still need rendering count against this frame's budget.
                let needs_render = !renderer.has_cached_thumbnail(model_id);
                let allow_render = *budget > 0;
                if needs_render && allow_render {
                    *budget -= 1;
                }
                match renderer.model_thumbnail(render_state, library, model_id, allow_render) {
                    ThumbnailStatus::Ready(texture_id) => {
                        TileImage::Texture { texture_id, aspect_ratio: 1.0 }
                    }
                    ThumbnailStatus::Pending => TileImage::Pending,
                    ThumbnailStatus::Unavailable => TileImage::None,
                }
            }
            PlaceableAsset::Flat { texture } => {
                let cached_handle = match self.flat_thumbnails.entry(texture) {
                    std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        if *budget == 0 {
                            return TileImage::Pending;
                        }
                        *budget -= 1;
                        entry.insert(library.texture_image(texture).map(|image| {
                            let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                [image.width as usize, image.height as usize],
                                &image.pixels,
                            );
                            crate::app::kept_texture::KeptTexture::new(context.load_texture(
                                texture.to_string(),
                                color_image,
                                egui::TextureOptions::NEAREST,
                            ))
                        }))
                    }
                };
                match cached_handle.as_ref() {
                    Some(handle) => {
                        let [width, height] = handle.size();
                        TileImage::Texture {
                            texture_id: handle.id(),
                            aspect_ratio: width as f32 / height.max(1) as f32,
                        }
                    }
                    None => TileImage::None,
                }
            }
            PlaceableAsset::Light => TileImage::LightIcon,
        }
    }

    /// Draws one tile, with the favourite star in its top-right corner (always
    /// shown for a favourite, otherwise while the pointer is over the tile).
    /// `is_favorite` is `None` for assets that cannot be favourites (prefabs,
    /// which show a prefab mark there instead). Returns its response and
    /// whether the star was clicked.
    fn draw_tile(
        &self,
        ui: &mut egui::Ui,
        entry: &PaletteEntry,
        image: TileImage,
        is_favorite: Option<bool>,
    ) -> (egui::Response, bool) {
        let tile_size = self.tile_size;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(tile_size, tile_size + CAPTION_HEIGHT), egui::Sense::click());
        let image_rect = egui::Rect::from_min_size(rect.min, egui::vec2(tile_size, tile_size));
        let painter = ui.painter_at(rect);

        let is_armed = self.armed.is_some_and(|armed| armed.asset == entry.asset);
        let background = if is_armed {
            egui::Color32::from_rgb(40, 80, 50)
        } else if response.hovered() {
            egui::Color32::from_gray(45)
        } else {
            egui::Color32::from_gray(28)
        };
        painter.rect_filled(image_rect, 4.0, background);

        match image {
            TileImage::Texture { texture_id, aspect_ratio } => {
                let inner = image_rect.shrink(4.0);
                let size = if aspect_ratio >= 1.0 {
                    egui::vec2(inner.width(), inner.width() / aspect_ratio)
                } else {
                    egui::vec2(inner.height() * aspect_ratio, inner.height())
                };
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                painter.image(
                    texture_id,
                    egui::Rect::from_center_size(inner.center(), size),
                    uv,
                    egui::Color32::WHITE,
                );
            }
            TileImage::Pending => {
                painter.text(
                    image_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "…",
                    egui::FontId::proportional(18.0),
                    egui::Color32::GRAY,
                );
            }
            TileImage::LightIcon => draw_light_icon(&painter, image_rect),
            TileImage::PrefabIcon => {
                painter.text(
                    image_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    PREFAB_MARK,
                    egui::FontId::proportional(tile_size * 0.4),
                    PREFAB_COLOR,
                );
            }
            TileImage::None => {
                painter.text(
                    image_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "?",
                    egui::FontId::proportional(18.0),
                    egui::Color32::GRAY,
                );
            }
        }

        if is_armed {
            painter.rect_stroke(
                image_rect,
                4.0,
                egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(110, 230, 140)),
            );
        }

        let star_rect = egui::Rect::from_min_size(
            egui::pos2(image_rect.right() - 20.0, image_rect.top() + 2.0),
            egui::vec2(18.0, 18.0),
        );
        let pointer = ui.ctx().pointer_interact_pos();
        let can_favorite = is_favorite.is_some();
        let is_favorite = is_favorite == Some(true);
        let over_star =
            can_favorite && response.hovered() && pointer.is_some_and(|pointer| star_rect.contains(pointer));
        let star_clicked =
            can_favorite && response.clicked() && pointer.is_some_and(|pointer| star_rect.contains(pointer));
        if !can_favorite {
            painter.text(
                star_rect.center(),
                egui::Align2::CENTER_CENTER,
                PREFAB_MARK,
                egui::FontId::proportional(14.0),
                PREFAB_COLOR,
            );
        } else if is_favorite || response.hovered() {
            let (star, color) = match (is_favorite, over_star) {
                (true, _) => ("★", FAVORITE_COLOR),
                (false, true) => ("★", egui::Color32::from_gray(200)),
                (false, false) => ("☆", egui::Color32::from_gray(150)),
            };
            painter.text(
                star_rect.center(),
                egui::Align2::CENTER_CENTER,
                star,
                egui::FontId::proportional(15.0),
                color,
            );
        }

        painter.text(
            egui::pos2(rect.center().x, image_rect.bottom() + 2.0),
            egui::Align2::CENTER_TOP,
            truncate_caption(&entry.caption, tile_size),
            egui::FontId::proportional(11.0),
            ui.visuals().text_color(),
        );

        let rename_hint = match entry.asset {
            PlaceableAsset::Model { .. } => " · right-click to name it",
            _ => "",
        };
        let favorite_hint = match (can_favorite, is_favorite) {
            (false, _) => "right-click to rename or delete it",
            (true, true) => "★ to remove from favorites",
            (true, false) => "☆ to add to favorites",
        };
        let response = match over_star {
            true => response.on_hover_text(favorite_hint),
            false => response.on_hover_text(format!(
                "{}\n{}\nClick to place · {favorite_hint}{rename_hint}",
                entry.caption, entry.details
            )),
        };
        (response, star_clicked)
    }
}

/// Contents of a prefab tile's right-click menu: its name, to edit, and a
/// button deleting it from the list. Returns whether that was clicked.
fn show_prefab_menu(ui: &mut egui::Ui, id: PrefabId, prefabs: &mut PrefabLibrary) -> bool {
    let Some(prefab) = prefabs.get(id) else { return false };
    ui.strong(format!("Prefab of {} objects", prefab.objects.len()));
    let mut name = prefab.name.clone();
    let field = egui::TextEdit::singleline(&mut name).hint_text("Name of the prefab");
    if ui.add(field).on_hover_text("Rename the prefab (copies already placed keep their name)").changed() {
        prefabs.rename(id, &name);
    }
    ui.separator();
    ui.button("Delete prefab")
        .on_hover_text(
            "Removes the prefab from this list. Copies already placed in cells stay as they are, still \
             grouped as prefabs.",
        )
        .clicked()
}

/// The entries of an imported model's right-click menu that change its
/// texture: one for a model of one part, else one per part and one for every
/// part without a texture. Returns the part chosen (`None` for every part
/// without one), once one is.
fn show_texture_menu(ui: &mut egui::Ui, model: &crate::custom_models::CustomModel) -> Option<Option<usize>> {
    let hover = "Choose an image (PNG, JPG, BMP or TGA) to draw the model with, as its texture coordinates \
                 lay it out, or its MTL material file (whose materials and textures it then takes). Use it when \
                 the texture or the MTL file was not found on import.";
    if model.parts.len() <= 1 {
        let text = match model.parts.first().is_some_and(|part| part.textured) {
            true => "Change texture…",
            false => "Choose texture… (it has none)",
        };
        return ui.button(text).on_hover_text(hover).clicked().then_some(Some(0));
    }
    let mut chosen = None;
    ui.menu_button("Change texture", |ui| {
        if ui.button("Every part without one…").on_hover_text(hover).clicked() {
            chosen = Some(None);
        }
        ui.separator();
        for (index, part) in model.parts.iter().enumerate() {
            let material = match part.material.is_empty() {
                true => format!("Part {}", index + 1),
                false => part.material.clone(),
            };
            let state = if part.textured { "" } else { " (no texture)" };
            if ui.button(format!("{material}{state}…")).on_hover_text(hover).clicked() {
                chosen = Some(Some(index));
            }
        }
    });
    chosen
}

/// Contents of a model tile's right-click menu: a field for the user's name for the model.
fn show_model_name_menu(ui: &mut egui::Ui, model_id: u32, custom_names: &mut CustomNames) {
    ui.strong(format!("Model {model_id}"));
    let mut name = custom_names.model_name_for_editing(model_id);
    let field = egui::TextEdit::singleline(&mut name).hint_text("Your name for this model (editor only)");
    if ui.add(field).changed() {
        custom_names.set_model_name(model_id, &name);
    }
}

/// What is drawn inside a tile.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TileImage {
    Texture {
        texture_id: egui::TextureId,
        aspect_ratio: f32,
    },
    Pending,
    LightIcon,
    /// A prefab without models to show.
    PrefabIcon,
    None,
}

/// Draws a simple light bulb style icon.
fn draw_light_icon(painter: &egui::Painter, rect: egui::Rect) {
    let center = rect.center();
    let radius = rect.width() * 0.16;
    let color = egui::Color32::from_rgb(255, 225, 110);
    painter.circle_filled(center, radius, color);
    for ray in 0..8 {
        let angle = ray as f32 / 8.0 * std::f32::consts::TAU;
        let direction = egui::vec2(angle.cos(), angle.sin());
        painter.line_segment(
            [center + direction * radius * 1.4, center + direction * radius * 2.2],
            egui::Stroke::new(2.0_f32, color),
        );
    }
}

/// Shortens a caption with an ellipsis so it fits under a tile.
fn truncate_caption(caption: &str, tile_size: f32) -> String {
    let max_characters = ((tile_size / 6.0) as usize).max(4);
    if caption.chars().count() <= max_characters {
        caption.to_string()
    } else {
        let shortened: String = caption.chars().take(max_characters - 1).collect();
        format!("{shortened}…")
    }
}
