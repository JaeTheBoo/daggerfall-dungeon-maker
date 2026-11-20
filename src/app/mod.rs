//! The egui application: windows, menus, dialogs and shortcuts.
//!
//! The main window shows the project panel and one workspace at a time: a
//! single room in a [`RoomEditor`] or a whole dungeon in a [`DungeonView`].
//! Double-clicking a room in the dungeon view opens it in its own window
//! running a full room editor; edits made there are copied back into the
//! dungeon view.
//!
//! Rooms and dungeons from Daggerfall open read-only. Trying to change one
//! offers to copy it into the [`Project`], where it can be edited, renamed and
//! saved in a `.ddm` project file.

mod arena2_prompt;
mod dungeon_browser;
mod dungeon_view;
mod entrance_picker;
mod export_window;
mod exterior_view;
mod interior_view;
mod kept_texture;
mod playtest_ui;
mod project_panel;
mod room_browser;
mod room_editor;
mod viewport_tools;
mod world_map;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::{egui, egui_wgpu};
use glam::Vec3;

use crate::arena2::{Arena2Library, Climate, DungeonDefinition, RDB_BLOCK_SIDE};
use crate::blender::BlendConversion;
use crate::buildings::{settlement_buildings, BuildingNamer};
use crate::catalog::AssetCatalog;
use crate::connectors::OpeningsLeftOpen;
use crate::custom_models::CustomModelLibrary;
use crate::custom_names::CustomNames;
use crate::dungeon_textures::{DungeonIdentity, TextureMode, TextureSetting};
use crate::prefabs::PrefabLibrary;
use crate::project::{
    CellCategory, DungeonLocation, ItemId, Project, ProjectDungeon, ProjectDungeonRoom, ProjectEntry,
    ProjectRoom, PROJECT_FILE_EXTENSION,
};
use crate::render::{RoomOutline, SceneRenderer};
use crate::scene::Scene;
use crate::unity_install::UnityText;
use arena2_prompt::Arena2Prompt;
use dungeon_browser::DungeonBrowser;
use dungeon_view::{
    DungeonDetails, DungeonIds, DungeonPropertiesAction, DungeonView, LocationWindowsRequest,
};
use export_window::{ExportRequest, ExportWindow};
use exterior_view::{ExteriorDetails, ExteriorOpen, ExteriorSubject, ExteriorView};
use interior_view::{BuildingPlace, InteriorKey, InteriorOpen, InteriorUse, InteriorView};
use project_panel::{OpenedItem, PanelAction, ProjectPanel};
use room_browser::RoomBrowser;
use room_editor::{ContextCell, ContextDoors, DungeonContext, RoomEditor, UsedInEntry};
use viewport_tools::EditorResources;

/// Storage key under which the chosen ARENA2 folder is remembered between runs.
const ARENA2_FOLDER_STORAGE_KEY: &str = "arena2_folder";
/// Storage key under which the user's room and model names are saved.
const CUSTOM_NAMES_STORAGE_KEY: &str = "custom_names";
/// The user's prefabs, saved as JSON text (see [`PrefabLibrary::to_text`]).
const PREFABS_STORAGE_KEY: &str = "prefabs";
/// Folder of the application's data the user's imported models are kept in.
const CUSTOM_MODELS_FOLDER: &str = "custom_models";
/// Storage key of the openings Daggerfall's dungeons leave open, learned from the game data.
const OPENINGS_STORAGE_KEY: &str = "openings_left_open";
/// Storage key of the export window's settings (folders, Unity editor, platforms).
const EXPORT_SETTINGS_STORAGE_KEY: &str = "export_settings";
/// Storage key of what the project panel shows.
const PANEL_VIEW_STORAGE_KEY: &str = "project_panel_view";
/// Storage key of Daggerfall Unity's install folder (File > Set Daggerfall Unity folder…).
const UNITY_FOLDER_STORAGE_KEY: &str = "daggerfall_unity_folder";
/// Size of the window showing a building's inside.
const INTERIOR_WINDOW_SIZE: [f32; 2] = [1150.0, 740.0];
/// Storage key under which the last project file is remembered, to reopen it on the next run.
const PROJECT_FILE_STORAGE_KEY: &str = "project_file";
/// Storage key of which textures dungeons are drawn with.
const DUNGEON_TEXTURES_STORAGE_KEY: &str = "dungeon_textures";
/// Storage key of the Blender program the user chose (Import > Locate Blender…).
const BLENDER_STORAGE_KEY: &str = "blender_program";
/// How long the pointer must rest on something before its explanation appears.
const TOOLTIP_DELAY_SECONDS: f32 = 0.5;
/// Name shown in window titles.
const APPLICATION_NAME: &str = "Daggerfall Dungeon Maker";
/// Name of a project that has not been saved yet.
const UNTITLED_PROJECT_NAME: &str = "Untitled project";
/// Initial size of a room editor window, in points.
const ROOM_WINDOW_SIZE: [f32; 2] = [1280.0, 820.0];

/// Optional settings supplied on the command line.
#[derive(Debug, Clone, Default)]
pub struct StartupOptions {
    /// ARENA2 folder to load instead of the remembered one.
    pub arena2_folder: Option<PathBuf>,
    /// Project file to open instead of the one open last time.
    pub project_file: Option<PathBuf>,
    /// Room to open once the game data has loaded, for example `N0000004.RDB`
    /// (or an outdoor cell, for example `TVRNAL01.RMB`).
    pub room_name: Option<String>,
    /// Dungeon to open once the game data has loaded, for example `Privateer's Hold`.
    pub dungeon_name: Option<String>,
    /// Settlement to open once the game data has loaded, for example `Daggerfall`.
    pub settlement_name: Option<String>,
    /// Building inside to open once the game data has loaded: an outdoor
    /// cell and the building's place in it (from 1), for example `TVRNAL01.RMB:2`.
    pub interior: Option<String>,
}

/// What the main window is showing.
enum Workspace {
    /// Nothing is open yet.
    Empty,
    /// A single room.
    Room(Box<RoomEditor>),
    /// A whole dungeon.
    Dungeon(Box<DungeonView>),
    /// A settlement or an outdoor cell from Daggerfall, read-only.
    Exterior(Box<ExteriorView>),
    /// The inside of one of Daggerfall's buildings, read-only.
    Interior(Box<InteriorView>),
}

/// A room editor running in its own operating system window.
struct RoomWindow {
    viewport_id: egui::ViewportId,
    editor: RoomEditor,
    /// Index of the room in the main window's dungeon that this window edits.
    dungeon_room_index: Option<usize>,
    /// Cleared when the user closes the window.
    is_open: bool,
    show_controls_window: bool,
}

/// What the user tried to change that comes from Daggerfall, so what a copy is made of.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CopySubject {
    /// A Daggerfall room open on its own: copy the room.
    Room,
    /// A property of a Daggerfall dungeon: copy the dungeon.
    Dungeon,
    /// A room in a Daggerfall dungeon: copy the dungeon and the room.
    DungeonAndRoom,
    /// A Daggerfall room in a project dungeon: copy the room for that dungeon.
    RoomInDungeon,
}

/// The "You cannot edit … from Daggerfall" question, shown after the user tried to change one.
#[derive(Debug, Clone, Copy)]
struct CopyPrompt {
    subject: CopySubject,
    /// Window the attempt was made in: `None` for the main window, otherwise a room window.
    viewport: Option<egui::ViewportId>,
}

/// The answer to the copy prompt.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CopyChoice {
    CreateCopy,
    Cancel,
}

/// Something that would discard unsaved project changes, waiting for the user to decide.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DiscardingAction {
    NewProject,
    OpenProject,
    Exit,
}

/// The openings Daggerfall's dungeons leave open, as saved with the settings,
/// with the game data they were learned from.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredOpenings {
    folder: String,
    version: u32,
    openings: OpeningsLeftOpen,
}

/// A new project dungeon and its map pixel, which its location id is picked from.
type NewDungeonPlace = (ItemId, Option<crate::arena2::MapPixel>);

/// The dungeon maker application.
pub struct DungeonMakerApp {
    /// Loaded game data; `None` until the user has chosen an ARENA2 folder.
    library: Option<Arena2Library>,
    /// Handles to the GPU device shared with egui; `None` only if wgpu failed to initialise.
    render_state: Option<egui_wgpu::RenderState>,
    /// GPU renderer shared by every window; `None` only if wgpu failed to initialise.
    renderer: Option<SceneRenderer>,
    /// Placeable assets, built from the game data when it is loaded.
    catalog: AssetCatalog,
    /// For each Daggerfall cell, the indices of the Daggerfall dungeons that use it.
    cell_usage: HashMap<String, Vec<usize>>,
    /// For each Daggerfall outdoor cell (RMB block), the indices of the locations that use it.
    outdoor_usage: std::sync::Arc<HashMap<String, Vec<usize>>>,
    /// The index of the dungeon under each location, by region and location id.
    dungeon_under: HashMap<(usize, u16), usize>,
    /// The user's own names for rooms and models, saved with the settings.
    custom_names: CustomNames,
    /// The user's prefabs, saved with the settings.
    prefabs: PrefabLibrary,
    /// The models the user imported, kept in the application's data folder.
    custom_models: CustomModelLibrary,
    /// The Blender program the user chose to convert `.blend` files with,
    /// saved with the settings; `None` to look for Blender where it is
    /// usually installed.
    blender_program: Option<PathBuf>,
    /// A `.blend` file Blender is converting to import, in the background.
    blend_conversion: Option<BlendConversion>,
    /// The user's own rooms and dungeons.
    project: Project,
    /// Where the project was last saved or opened from; `None` until it is saved.
    project_path: Option<PathBuf>,
    /// The project as last saved or opened, to tell what has unsaved changes.
    /// Saving one cell or dungeon (Ctrl+S) saves it into this copy, which is
    /// then written, so other unsaved changes stay unsaved.
    saved_project: Project,
    workspace: Workspace,
    /// What the workspace shows; `None` when it is empty.
    opened_item: Option<OpenedItem>,
    room_windows: Vec<RoomWindow>,
    /// Counter used to give every room window a unique id.
    next_room_window_number: u64,
    arena2_prompt: Arena2Prompt,
    room_browser: RoomBrowser,
    dungeon_browser: DungeonBrowser,
    project_panel: ProjectPanel,
    export_window: ExportWindow,
    /// Testing in Daggerfall Unity: the Test button's questions, the test
    /// being readied and the game running it.
    playtest: playtest_ui::PlaytestState,
    /// The learned openings, as saved with the settings.
    stored_openings: Option<StoredOpenings>,
    /// Learning the openings in the background: the folder and where the result arrives.
    openings_job: Option<(PathBuf, std::sync::mpsc::Receiver<OpeningsLeftOpen>)>,
    copy_prompt: Option<CopyPrompt>,
    /// Room or dungeon the user asked to delete, waiting for confirmation.
    delete_prompt: Option<ItemId>,
    /// Action waiting for the user to decide what to do with unsaved changes.
    unsaved_changes_prompt: Option<DiscardingAction>,
    /// Set once the user agreed to close the program, so the close is not stopped again.
    exit_confirmed: bool,
    status_message: String,
    error_message: Option<String>,
    show_controls_window: bool,
    /// Title last sent to the main window, to avoid resending it every frame.
    window_title: String,
    /// Which textures dungeons are drawn with (View > Dungeon textures), saved with the settings.
    texture_setting: TextureSetting,
    /// The texture mode in Daggerfall Unity's own settings, read at startup.
    game_texture_mode: Option<TextureMode>,
    /// The location ids an export gives the project's new dungeons, with what
    /// they were worked out from (each new dungeon and its map pixel).
    new_location_ids: Option<(Vec<NewDungeonPlace>, HashMap<ItemId, u16>)>,
    /// Daggerfall Unity's install folder, saved with the settings: its word
    /// lists name the buildings of settlements.
    unity_folder: Option<PathBuf>,
    /// Daggerfall Unity's word lists, read from `unity_folder`.
    unity_text: Option<std::sync::Arc<UnityText>>,
    /// A building's inside, shown in a window of its own.
    interior_window: Option<Box<InteriorView>>,
    /// The settlements using each building, worked out when it is first opened.
    interior_uses: HashMap<InteriorKey, Vec<InteriorUse>>,
}

impl DungeonMakerApp {
    /// Creates the application.
    ///
    /// The ARENA2 folder given on the command line is used if present,
    /// otherwise the folder remembered from the last run. If neither is
    /// valid the user is asked to choose one. The project given on the command
    /// line, or else the one open last time, is reopened.
    pub fn new(creation_context: &eframe::CreationContext<'_>, startup_options: StartupOptions) -> Self {
        // Property explanations appear after the pointer has rested on them for half a second.
        creation_context.egui_ctx.style_mut(|style| style.interaction.tooltip_delay = TOOLTIP_DELAY_SECONDS);
        let render_state = creation_context.wgpu_render_state.clone();
        let renderer = render_state.as_ref().map(SceneRenderer::new);
        let stored_string = |key: &str| creation_context.storage.and_then(|storage| storage.get_string(key));
        let saved_folder = startup_options
            .arena2_folder
            .clone()
            .or_else(|| stored_string(ARENA2_FOLDER_STORAGE_KEY).map(PathBuf::from));
        let project_file = startup_options.project_file.clone().or_else(|| {
            stored_string(PROJECT_FILE_STORAGE_KEY).map(PathBuf::from).filter(|path| path.is_file())
        });
        let custom_names = creation_context
            .storage
            .and_then(|storage| eframe::get_value(storage, CUSTOM_NAMES_STORAGE_KEY))
            .unwrap_or_default();
        let prefabs = match stored_string(PREFABS_STORAGE_KEY).map(|text| PrefabLibrary::from_text(&text)) {
            Some(Ok(prefabs)) => prefabs,
            Some(Err(error)) => {
                log::warn!("could not read the saved prefabs: {error}");
                PrefabLibrary::default()
            }
            None => PrefabLibrary::default(),
        };
        let (custom_models, model_problems) = match eframe::storage_dir(APPLICATION_NAME) {
            Some(folder) => CustomModelLibrary::open(folder.join(CUSTOM_MODELS_FOLDER)),
            None => (CustomModelLibrary::default(), Vec::new()),
        };

        let mut project_panel = ProjectPanel::default();
        if let Some(view) =
            creation_context.storage.and_then(|storage| eframe::get_value(storage, PANEL_VIEW_STORAGE_KEY))
        {
            project_panel.view = view;
        }

        let mut export_window = ExportWindow::default();
        if let Some(settings) = creation_context
            .storage
            .and_then(|storage| eframe::get_value(storage, EXPORT_SETTINGS_STORAGE_KEY))
        {
            export_window.settings = settings;
        }

        let project = Project::default();
        let saved_project = project.clone();
        let mut app = Self {
            library: None,
            render_state,
            renderer,
            catalog: AssetCatalog::default(),
            cell_usage: HashMap::new(),
            outdoor_usage: std::sync::Arc::default(),
            dungeon_under: HashMap::new(),
            custom_names,
            prefabs,
            custom_models,
            blender_program: stored_string(BLENDER_STORAGE_KEY)
                .filter(|path| !path.is_empty())
                .map(PathBuf::from),
            blend_conversion: None,
            project,
            project_path: None,
            saved_project,
            workspace: Workspace::Empty,
            opened_item: None,
            room_windows: Vec::new(),
            next_room_window_number: 0,
            arena2_prompt: Arena2Prompt::new(true, saved_folder.as_deref()),
            room_browser: RoomBrowser::default(),
            dungeon_browser: DungeonBrowser::default(),
            project_panel,
            export_window,
            playtest: Default::default(),
            stored_openings: creation_context
                .storage
                .and_then(|storage| eframe::get_value(storage, OPENINGS_STORAGE_KEY)),
            openings_job: None,
            copy_prompt: None,
            delete_prompt: None,
            unsaved_changes_prompt: None,
            exit_confirmed: false,
            status_message: "Choose your Daggerfall ARENA2 folder to begin.".to_string(),
            error_message: None,
            show_controls_window: false,
            window_title: String::new(),
            texture_setting: creation_context
                .storage
                .and_then(|storage| eframe::get_value(storage, DUNGEON_TEXTURES_STORAGE_KEY))
                .unwrap_or_default(),
            game_texture_mode: crate::dungeon_textures::game_texture_mode(),
            new_location_ids: None,
            unity_folder: stored_string(UNITY_FOLDER_STORAGE_KEY)
                .filter(|path| !path.is_empty())
                .map(PathBuf::from),
            unity_text: None,
            interior_window: None,
            interior_uses: HashMap::new(),
        };
        app.load_unity_text();
        if !model_problems.is_empty() {
            app.error_message = Some(model_problems.join("\n"));
        }

        if app.renderer.is_none() {
            app.error_message =
                Some("The GPU renderer could not be initialised, so the 3D view is unavailable.".into());
        }
        if let Some(folder) = saved_folder.and_then(|folder| Arena2Library::resolve_folder(&folder)) {
            app.load_library(&folder);
        }
        if let Some(path) = project_file {
            app.open_project_file(&path);
        }
        if app.library.is_some() {
            if let Some(room_name) = startup_options.room_name {
                let room_name = room_name.to_ascii_uppercase();
                match room_name.ends_with(".RMB") {
                    true => app.open_outdoor_cell(&room_name),
                    false => app.open_room(&room_name),
                }
            }
            if let Some(dungeon_name) = startup_options.dungeon_name {
                app.open_dungeon_named(&dungeon_name);
            }
            if let Some(interior) = startup_options.interior {
                match interior
                    .rsplit_once(':')
                    .and_then(|(block, number)| Some((block, number.parse::<usize>().ok()?)))
                {
                    Some((block, number)) if number > 0 => app.open_interior(&InteriorKey {
                        block: block.to_ascii_uppercase(),
                        record: number - 1,
                    }),
                    _ => {
                        app.error_message = Some(format!(
                            "'{interior}' is not a building: use a cell and a number, as TVRNAL01.RMB:2"
                        ))
                    }
                }
            }
            if let Some(settlement_name) = startup_options.settlement_name {
                app.open_settlement_named(&settlement_name);
            }
        }
        app
    }

    /// Loads the game data in `folder`, replacing any previously loaded data.
    fn load_library(&mut self, folder: &Path) {
        match Arena2Library::open(folder) {
            Ok(mut library) => {
                self.close_everything();
                let room_names = library.rdb_block_names();
                self.room_browser.set_room_names(room_names.clone());
                let dungeons = library.dungeons();
                self.cell_usage.clear();
                for (index, dungeon) in dungeons.iter().enumerate() {
                    for block in &dungeon.blocks {
                        let users = self.cell_usage.entry(block.block_name.clone()).or_default();
                        if users.last() != Some(&index) {
                            users.push(index);
                        }
                    }
                }
                self.dungeon_under = dungeons
                    .iter()
                    .enumerate()
                    .filter_map(|(index, dungeon)| {
                        Some(((dungeon.region_index, dungeon.classic?.location_id), index))
                    })
                    .collect();
                self.project_panel.set_daggerfall_contents(room_names, dungeons);
                self.list_outdoor_cells(&mut library);
                // The user's own models show as if they were Daggerfall's.
                self.custom_models.register(&mut library);
                self.catalog = AssetCatalog::build(&mut library);
                self.start_learning_openings(&library);
                self.status_message = format!("Loaded game data from {}", folder.display());
                self.library = Some(library);
                // Cached GPU resources belong to the old data, so start afresh.
                if let (Some(renderer), Some(render_state)) =
                    (self.renderer.as_mut(), self.render_state.as_ref())
                {
                    renderer.clear_cache(render_state);
                }
                self.arena2_prompt.is_open = false;
            }
            Err(error) => self.arena2_prompt.open(Some(error.to_string())),
        }
    }

    /// Lists Daggerfall's settlements and outdoor cells in the project panel,
    /// noting the locations that use each cell.
    fn list_outdoor_cells(&mut self, library: &mut Arena2Library) {
        let locations = library.locations();
        let mut usage: HashMap<String, Vec<usize>> = HashMap::new();
        let mut found_names: HashMap<&str, Option<String>> = HashMap::new();
        for (index, location) in locations.iter().enumerate() {
            for name in &location.exterior.block_names {
                let found = found_names.entry(name).or_insert_with(|| library.rmb_block_found_name(name));
                if let Some(found) = found {
                    let users = usage.entry(found.clone()).or_default();
                    if users.last() != Some(&index) {
                        users.push(index);
                    }
                }
            }
        }
        self.outdoor_usage = std::sync::Arc::new(usage);
        let block_names = library.rmb_block_names();
        // Every building with an inside, by its cell and place in it.
        let mut interiors = Vec::new();
        for name in &block_names {
            let Ok(block) = library.rmb_block(name) else { continue };
            for (record, building) in block.buildings.iter().enumerate() {
                if building.has_interior() {
                    interiors
                        .push((InteriorKey { block: name.clone(), record }, building.data.building_type));
                }
            }
        }
        self.interior_uses.clear();
        self.project_panel.set_outdoor_contents(locations, block_names, self.outdoor_usage.clone());
        self.project_panel.set_interiors(interiors);
    }

    /// Reads Daggerfall Unity's word lists, which name buildings, from its
    /// install folder: the one saved, or else one found where it is usually unzipped.
    fn load_unity_text(&mut self) {
        let mods = self.export_window.settings.mods_folder.trim();
        let hints: Vec<PathBuf> = self
            .unity_folder
            .iter()
            .cloned()
            .chain((!mods.is_empty()).then(|| PathBuf::from(mods)))
            .collect();
        let Some(folder) = crate::unity_install::find_install_folder(&hints) else { return };
        match UnityText::load(&folder) {
            Ok(text) => {
                self.unity_text = Some(std::sync::Arc::new(text));
                self.unity_folder = Some(folder);
            }
            Err(error) => log::warn!("could not read Daggerfall Unity's text: {error}"),
        }
    }

    /// Asks for Daggerfall Unity's install folder and reads its word lists from it.
    fn choose_unity_folder(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Daggerfall Unity's folder");
        if let Some(folder) = &self.unity_folder {
            dialog = dialog.set_directory(folder);
        }
        let Some(chosen) = dialog.pick_folder() else { return };
        let Some(folder) = crate::unity_install::resolve_install_folder(&chosen) else {
            self.error_message = Some(format!(
                "{} is not Daggerfall Unity's folder: it has no DaggerfallUnity_Data/StreamingAssets/Text folder \
                 with Daggerfall Unity's word lists.",
                chosen.display()
            ));
            return;
        };
        match UnityText::load(&folder) {
            Ok(text) => {
                self.unity_text = Some(std::sync::Arc::new(text));
                self.status_message =
                    format!("Building names now come from Daggerfall Unity in {}", folder.display());
                self.unity_folder = Some(folder);
                self.interior_uses.clear();
                // A settlement open shows its buildings' names from now on.
                if let Some(OpenedItem::DaggerfallSettlement(index)) = self.opened_item.clone() {
                    self.open_settlement(index);
                }
            }
            Err(error) => self.error_message = Some(error),
        }
    }

    /// The settlements whose cells use the building `key`, with what the
    /// building is in each (its data and name), worked out once and kept.
    fn interior_uses_of(&mut self, key: &InteriorKey) -> Vec<InteriorUse> {
        if let Some(uses) = self.interior_uses.get(key) {
            return uses.clone();
        }
        let Some(library) = self.library.as_mut() else { return Vec::new() };
        let locations = library.locations();
        let users = self.outdoor_usage.get(&key.block).cloned().unwrap_or_default();
        // Each cell read once, though settlements share many.
        let mut blocks: HashMap<String, Option<crate::arena2::RmbBlock>> = HashMap::new();
        for &location_index in &users {
            for name in &locations[location_index].exterior.block_names {
                if !blocks.contains_key(name) {
                    let block = library.rmb_block(name).ok();
                    blocks.insert(name.clone(), block);
                }
            }
        }
        let references = library.references();
        let namer = BuildingNamer { unity: self.unity_text.as_deref(), references: &references };
        let mut uses = Vec::new();
        for &location_index in &users {
            let location = &locations[location_index];
            let cells: Vec<Option<&crate::arena2::RmbBlock>> = location
                .exterior
                .block_names
                .iter()
                .map(|name| blocks.get(name).and_then(Option::as_ref))
                .collect();
            let buildings = settlement_buildings(location, &cells);
            let width = location.exterior.width.max(1);
            for (cell, block) in cells.iter().enumerate() {
                let is_this = block.is_some_and(|block| block.name.eq_ignore_ascii_case(&key.block));
                if let Some(data) = buildings[cell].get(key.record).filter(|_| is_this) {
                    let name = namer.name(data, &location.name, location.region_index);
                    let building_key = crate::people::building_key(cell % width, cell / width, key.record);
                    uses.push(InteriorUse { location_index, data: *data, name, building_key });
                }
            }
        }
        self.interior_uses.insert(key.clone(), uses.clone());
        uses
    }

    /// Opens the inside of the building `key` in the main window, read-only.
    fn open_interior(&mut self, key: &InteriorKey) {
        let uses = self.interior_uses_of(key);
        let Some(library) = self.library.as_mut() else { return };
        match InteriorView::open(key.clone(), None, Climate::DEFAULT, self.unity_text.as_deref(), library) {
            Ok(mut view) => {
                view.uses = Some(uses);
                self.set_workspace(
                    Workspace::Interior(Box::new(view)),
                    Some(OpenedItem::DaggerfallInterior(key.clone())),
                );
            }
            Err(error) => self.error_message = Some(error),
        }
    }

    /// Opens the inside of the building `key` in a window of its own, as it
    /// stands in `place` (if it was opened from a settlement), in `climate`.
    fn open_interior_window(&mut self, key: InteriorKey, place: Option<BuildingPlace>, climate: Climate) {
        let uses = self.interior_uses_of(&key);
        let Some(library) = self.library.as_mut() else { return };
        match InteriorView::open(key, place, climate, self.unity_text.as_deref(), library) {
            Ok(mut view) => {
                view.uses = Some(uses);
                self.close_interior_window();
                self.interior_window = Some(Box::new(view));
            }
            Err(error) => self.error_message = Some(error),
        }
    }

    /// Closes the window showing a building's inside, releasing its GPU images.
    fn close_interior_window(&mut self) {
        if let (Some(mut view), Some(render_state)) =
            (self.interior_window.take(), self.render_state.as_ref())
        {
            view.release(render_state);
        }
    }

    /// Draws the window showing a building's inside, if one is open.
    fn show_interior_window(&mut self, context: &egui::Context) {
        let mut close = false;
        let mut action = None;
        {
            let (Some(view), Some(library)) = (self.interior_window.as_mut(), self.library.as_mut()) else {
                return;
            };
            let locations = library.locations();
            let references = library.references();
            let mut resources = EditorResources {
                library,
                render_state: self.render_state.as_ref(),
                renderer: self.renderer.as_mut(),
                catalog: &self.catalog,
                custom_names: &mut self.custom_names,
                prefabs: &mut self.prefabs,
                custom_models: &self.custom_models,
                texture_setting: &mut self.texture_setting,
                game_texture_mode: self.game_texture_mode,
                unity: self.unity_text.as_deref(),
            };
            let builder = egui::ViewportBuilder::default()
                .with_title(format!("Inside {} - {APPLICATION_NAME}", view.description()))
                .with_inner_size(INTERIOR_WINDOW_SIZE);
            context.show_viewport_immediate(
                egui::ViewportId::from_hash_of("interior_window"),
                builder,
                |window_context, _class| {
                    view.handle_shortcuts(window_context, resources.library);
                    egui::TopBottomPanel::bottom("interior_window_status")
                        .show(window_context, |ui| ui.label(view.status_text()));
                    egui::SidePanel::right("interior_window_properties")
                        .resizable(true)
                        .default_width(300.0)
                        .show(window_context, |ui| {
                            ui.heading(view.description());
                            ui.separator();
                            action = view.show_properties(ui, &locations, &references);
                        });
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(window_context, |ui| view.show_viewport(ui, &mut resources));
                    if window_context.input(|input| input.viewport().close_requested()) {
                        close = true;
                    }
                },
            );
        }
        if close {
            self.close_interior_window();
        }
        match action {
            Some(InteriorOpen::Settlement(index)) => self.open_settlement(index),
            Some(InteriorOpen::Cell(name)) => self.open_outdoor_cell(&name),
            None => {}
        }
    }

    /// Gives the catalog the openings Daggerfall's dungeons leave open: the
    /// saved ones if they were learned from this game data, otherwise learned
    /// afresh on a background thread (it takes a while).
    fn start_learning_openings(&mut self, library: &Arena2Library) {
        let folder = library.root().display().to_string();
        if let Some(stored) = self
            .stored_openings
            .as_ref()
            .filter(|stored| stored.folder == folder && stored.version == OpeningsLeftOpen::VERSION)
        {
            self.catalog.openings_left_open = Some(std::sync::Arc::new(stored.openings.clone()));
            self.openings_job = None;
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let root = library.root().to_path_buf();
        std::thread::spawn(move || match Arena2Library::open(&root) {
            Ok(mut library) => {
                let _ = sender.send(OpeningsLeftOpen::learn(&mut library));
            }
            Err(error) => log::warn!("could not learn the openings dungeons leave open: {error}"),
        });
        self.openings_job = Some((library.root().to_path_buf(), receiver));
    }

    /// Takes in the learned openings once the background thread has them.
    fn poll_openings_job(&mut self, context: &egui::Context) {
        let Some((folder, receiver)) = &self.openings_job else { return };
        match receiver.try_recv() {
            Ok(openings) => {
                self.stored_openings = Some(StoredOpenings {
                    folder: folder.display().to_string(),
                    version: OpeningsLeftOpen::VERSION,
                    openings: openings.clone(),
                });
                self.catalog.openings_left_open = Some(std::sync::Arc::new(openings));
                self.openings_job = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                context.request_repaint_after(std::time::Duration::from_millis(500));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.openings_job = None,
        }
    }

    // -----------------------------------------------------------------------
    // Opening rooms and dungeons
    // -----------------------------------------------------------------------

    /// Opens `item` in the main window.
    fn open_item(&mut self, item: &OpenedItem) {
        match item {
            OpenedItem::DaggerfallRoom(room_name) => self.open_room(room_name),
            OpenedItem::DaggerfallDungeon(dungeon_index) => self.open_dungeon(*dungeon_index),
            OpenedItem::DaggerfallSettlement(location_index) => self.open_settlement(*location_index),
            OpenedItem::DaggerfallOutdoorCell(block_name) => self.open_outdoor_cell(block_name),
            OpenedItem::DaggerfallInterior(key) => self.open_interior(key),
            OpenedItem::Project(id) => self.open_project_item(*id),
        }
    }

    /// Opens the Daggerfall room called `room_name`, read-only, in the main window.
    fn open_room(&mut self, room_name: &str) {
        let Some(library) = self.library.as_mut() else { return };
        match library.rdb_block(room_name) {
            Ok(block) => {
                let scene = Scene::from_rdb_block(&block);
                let title = format!("{room_name} ({})", room_browser::category_description(room_name));
                let mut editor = RoomEditor::new(scene, title, egui::Id::new("main_room_editor"), library);
                editor.set_read_only(true);
                self.status_message = editor.status_message().to_string();
                self.set_workspace(
                    Workspace::Room(Box::new(editor)),
                    Some(OpenedItem::DaggerfallRoom(room_name.to_string())),
                );
            }
            Err(error) => self.error_message = Some(format!("Could not open {room_name}: {error}")),
        }
    }

    /// Opens the Daggerfall dungeon at `dungeon_index` in the library's dungeon list, read-only.
    fn open_dungeon(&mut self, dungeon_index: usize) {
        let Some(library) = self.library.as_mut() else { return };
        let dungeons = library.dungeons();
        let Some(definition) = dungeons.get(dungeon_index) else { return };

        let layout = ProjectDungeonRoom::layout_of(definition);
        let (view, problems) = DungeonView::load(
            &definition.name,
            definition.region_name,
            definition.dungeon_type,
            &layout,
            &self.project,
            library,
        );
        self.show_dungeon(view, problems, OpenedItem::DaggerfallDungeon(dungeon_index));
    }

    /// Opens the Daggerfall location at `location_index` in the library's location list, read-only.
    fn open_settlement(&mut self, location_index: usize) {
        let Some(library) = self.library.as_mut() else { return };
        let locations = library.locations();
        let Some(location) = locations.get(location_index) else { return };
        let climate = library
            .world_map()
            .and_then(|world| world.location_climate_at(location.map_pixel))
            .map_or(Climate::DEFAULT, Climate::from_value);
        let references = library.references();
        let namer = BuildingNamer { unity: self.unity_text.as_deref(), references: &references };
        let (view, problems) = ExteriorView::settlement(location_index, location, climate, &namer, library);
        self.show_exterior(view, problems, OpenedItem::DaggerfallSettlement(location_index));
    }

    /// Opens the Daggerfall location called `name` (case-insensitive): the
    /// largest of that name, as several share some names.
    fn open_settlement_named(&mut self, name: &str) {
        let Some(library) = self.library.as_mut() else { return };
        let locations = library.locations();
        let size = |index: usize| locations[index].exterior.block_names.len();
        let found = (0..locations.len())
            .filter(|&index| locations[index].name.eq_ignore_ascii_case(name))
            .max_by_key(|&index| size(index));
        match found {
            Some(index) => self.open_settlement(index),
            None => self.error_message = Some(format!("There is no location called '{name}'.")),
        }
    }

    /// Opens the Daggerfall outdoor cell `block_name` (an RMB block), read-only.
    fn open_outdoor_cell(&mut self, block_name: &str) {
        let Some(library) = self.library.as_mut() else { return };
        let (view, problems) = ExteriorView::cell(block_name, library);
        self.show_exterior(view, problems, OpenedItem::DaggerfallOutdoorCell(block_name.to_string()));
    }

    /// Shows a loaded settlement or outdoor cell in the main window, reporting cells that could not be loaded.
    fn show_exterior(&mut self, view: ExteriorView, problems: Vec<String>, item: OpenedItem) {
        if !problems.is_empty() {
            self.error_message = Some(format!("Some cells could not be loaded:\n{}", problems.join("\n")));
        }
        self.set_workspace(Workspace::Exterior(Box::new(view)), Some(item));
    }

    /// Opens the first Daggerfall dungeon whose name matches `dungeon_name` (case-insensitive).
    fn open_dungeon_named(&mut self, dungeon_name: &str) {
        let Some(library) = self.library.as_mut() else { return };
        let dungeon_index =
            library.dungeons().iter().position(|dungeon| dungeon.name.eq_ignore_ascii_case(dungeon_name));
        match dungeon_index {
            Some(index) => self.open_dungeon(index),
            None => self.error_message = Some(format!("There is no dungeon called '{dungeon_name}'.")),
        }
    }

    /// Opens the project room or dungeon `id` in the main window.
    fn open_project_item(&mut self, id: ItemId) {
        if self.library.is_none() {
            self.error_message =
                Some("Choose your ARENA2 folder first: cells are drawn from the game data.".into());
            return;
        }
        // Store the open item's changes first, in case it is the one being reopened.
        self.set_workspace(Workspace::Empty, None);
        self.fill_in_copied_location(id);
        let entry = self.project.find(id).cloned();
        let Some(library) = self.library.as_mut() else { return };
        match entry {
            Some(ProjectEntry::Room(room)) => {
                let scene = Scene {
                    block_name: room.based_on.clone().unwrap_or_default(),
                    objects: room.objects.clone(),
                };
                let mut editor =
                    RoomEditor::new(scene, room.name.clone(), egui::Id::new("main_room_editor"), library);
                editor.set_project_room_name(Some(room.name.clone()));
                editor.set_cell_category(Some(room.category));
                self.status_message = editor.status_message().to_string();
                self.set_workspace(Workspace::Room(Box::new(editor)), Some(OpenedItem::Project(id)));
            }
            Some(ProjectEntry::Dungeon(dungeon)) => {
                let (view, problems) = DungeonView::load(
                    &dungeon.name,
                    &dungeon.region_name,
                    dungeon.dungeon_type,
                    &dungeon.rooms,
                    &self.project,
                    library,
                );
                self.show_dungeon(view, problems, OpenedItem::Project(id));
            }
            _ => {}
        }
    }

    /// Gives project dungeon `id` the location type and entrance of the
    /// Daggerfall dungeon it was copied from, if it has no location yet
    /// (dungeons copied before dungeons had locations).
    fn fill_in_copied_location(&mut self, id: ItemId) {
        let Some(library) = self.library.as_mut() else { return };
        let Some(dungeon) = self.project.dungeon_mut(id) else { return };
        if dungeon.location != DungeonLocation::default() {
            return;
        }
        let Some(original) = dungeon.based_on.as_deref() else { return };
        let dungeons = library.dungeons();
        let matching = |definition: &&DungeonDefinition| definition.name == original;
        let definition = dungeons
            .iter()
            .filter(matching)
            .find(|definition| definition.region_name == dungeon.region_name)
            .or_else(|| dungeons.iter().find(matching));
        if let Some(definition) = definition {
            dungeon.location = DungeonLocation::copied_from(definition);
        }
    }

    /// Shows a loaded dungeon in the main window, reporting rooms that could not be loaded.
    fn show_dungeon(&mut self, view: DungeonView, problems: Vec<String>, item: OpenedItem) {
        if !problems.is_empty() {
            self.error_message = Some(format!("Some cells could not be loaded:\n{}", problems.join("\n")));
        }
        self.status_message = view.status_text(&self.custom_names);
        self.set_workspace(Workspace::Dungeon(Box::new(view)), Some(item));
    }

    /// Opens room `room_index` of the open dungeon in its own room editor window.
    ///
    /// Only one room window is open at a time: any other room window is closed
    /// first. Its edits are not lost, because every frame copies them back into
    /// the dungeon view. If the room is already open, its window is brought to the front.
    fn open_room_window(&mut self, context: &egui::Context, room_index: usize) {
        if let Some(window) =
            self.room_windows.iter().find(|window| window.dungeon_room_index == Some(room_index))
        {
            context.send_viewport_cmd_to(window.viewport_id, egui::ViewportCommand::Focus);
            return;
        }
        self.close_room_windows();

        let (Workspace::Dungeon(view), Some(library)) = (&self.workspace, self.library.as_mut()) else {
            return;
        };
        let Some(room) = view.room(room_index) else { return };

        let window_number = self.next_room_window_number;
        self.next_room_window_number += 1;
        let title = format!("{} in {}", room.block_name, view.name);
        let editor_id = egui::Id::new(("room_window_editor", window_number));
        let mut editor = RoomEditor::new(room.scene.clone(), title, editor_id, library);
        // Daggerfall's own rooms stay read-only, even in a project dungeon.
        editor.set_read_only(room.project_room.is_none());
        editor.set_cell_category(
            room.project_room.and_then(|room_id| self.project.room(room_id)).map(|room| room.category),
        );
        editor.set_dungeon_type(view.dungeon_type);
        // The dungeon it was opened from is selected under "Used in", so its
        // other cells show faded around this one.
        if let Some(dungeon) = self.opened_item.clone() {
            editor.select_used_in(Some(dungeon));
        }
        self.status_message = format!("Opened {} in a new window", room.block_name);
        self.room_windows.push(RoomWindow {
            viewport_id: egui::ViewportId::from_hash_of(("room_window", window_number)),
            editor,
            dungeon_room_index: Some(room_index),
            is_open: true,
            show_controls_window: false,
        });
    }

    /// Replaces the main window's workspace, which now shows `item`.
    ///
    /// Changes to the project item that was open are stored in the project
    /// first. Room windows belong to the old workspace, so they are closed.
    fn set_workspace(&mut self, workspace: Workspace, item: Option<OpenedItem>) {
        self.store_workspace_changes();
        if let Some(render_state) = self.render_state.as_ref() {
            match &mut self.workspace {
                Workspace::Room(editor) => editor.release(render_state),
                Workspace::Dungeon(view) => view.release(render_state),
                Workspace::Exterior(view) => view.release(render_state),
                Workspace::Interior(view) => view.release(render_state),
                Workspace::Empty => {}
            }
        }
        self.close_room_windows();
        self.workspace = workspace;
        self.opened_item = item;
        self.copy_prompt = None;
    }

    /// Closes the workspace and every room window.
    fn close_everything(&mut self) {
        self.set_workspace(Workspace::Empty, None);
        self.close_interior_window();
    }

    /// Closes every room window, releasing their GPU images.
    ///
    /// A window's OS window disappears on the next frame, because it is no longer shown.
    fn close_room_windows(&mut self) {
        if let Some(render_state) = self.render_state.as_ref() {
            for window in &mut self.room_windows {
                window.editor.release(render_state);
            }
        }
        self.room_windows.clear();
    }

    // -----------------------------------------------------------------------
    // Project items
    // -----------------------------------------------------------------------

    /// Copies the open project room's or dungeon's edits into the project.
    fn store_workspace_changes(&mut self) {
        let Some(OpenedItem::Project(id)) = self.opened_item else { return };
        match &self.workspace {
            Workspace::Room(editor) => {
                if let Some(room) = self.project.room_mut(id) {
                    if room.objects != editor.scene.objects {
                        room.objects = editor.scene.objects.clone();
                    }
                    if let Some(category) = editor.cell_category() {
                        room.category = category;
                    }
                }
            }
            Workspace::Dungeon(view) => {
                if self.project.dungeon(id).is_some() {
                    view.store_edited_rooms(&mut self.project);
                }
            }
            Workspace::Exterior(_) | Workspace::Interior(_) | Workspace::Empty => {}
        }
    }

    /// The dungeons that use a cell: those using the project cell `project_room`,
    /// or, for a Daggerfall cell, the Daggerfall dungeons and project dungeons
    /// that use the original `block_name`. Project dungeons come first.
    fn used_in(&mut self, block_name: &str, project_room: Option<ItemId>) -> Vec<UsedInEntry> {
        let mut entries: Vec<UsedInEntry> = self
            .project
            .dungeons()
            .into_iter()
            .filter(|dungeon| {
                dungeon.rooms.iter().any(|placement| match project_room {
                    Some(room_id) => placement.room == Some(room_id),
                    None => placement.room.is_none() && placement.block_name == block_name,
                })
            })
            .map(|dungeon| UsedInEntry {
                label: format!("{} · your dungeon", dungeon.name),
                item: OpenedItem::Project(dungeon.id),
            })
            .collect();
        if project_room.is_none() {
            if let (Some(library), Some(users)) = (self.library.as_mut(), self.cell_usage.get(block_name)) {
                let dungeons = library.dungeons();
                entries.extend(users.iter().filter_map(|&index| {
                    let dungeon = dungeons.get(index)?;
                    Some(UsedInEntry {
                        label: format!("{} · {}", dungeon.name, dungeon.region_name),
                        item: OpenedItem::DaggerfallDungeon(index),
                    })
                }));
            }
        }
        entries
    }

    /// The cells of `dungeon` around this cell, positioned relative to it, for
    /// showing faded around the cell. The cell is found as in [`Self::used_in`];
    /// `layout_index` picks its place when the cell appears more than once and
    /// `dungeon` is the dungeon open in the main window.
    fn dungeon_context(
        &mut self,
        dungeon: &OpenedItem,
        block_name: &str,
        project_room: Option<ItemId>,
        layout_index: Option<usize>,
    ) -> Option<DungeonContext> {
        let library = self.library.as_mut()?;
        let layout = match dungeon {
            OpenedItem::DaggerfallDungeon(index) => {
                ProjectDungeonRoom::layout_of(library.dungeons().get(*index)?)
            }
            OpenedItem::Project(id) => self.project.dungeon(*id)?.rooms.clone(),
            _ => return None,
        };
        let is_this_cell = |placement: &ProjectDungeonRoom| match project_room {
            Some(room_id) => placement.room == Some(room_id),
            None => placement.room.is_none() && placement.block_name == block_name,
        };
        let preferred = layout_index.filter(|&index| layout.get(index).is_some_and(is_this_cell));
        let here_index = preferred.or_else(|| layout.iter().position(is_this_cell))?;
        let here = &layout[here_index];

        let mut objects = Vec::new();
        let mut outlines = vec![RoomOutline::cell(Vec3::ZERO)];
        let mut cells = Vec::new();
        let model_tags = library.model_tags();
        let mut doors = ContextDoors {
            origin: crate::export::cell_origin(here),
            is_start: here.is_starting_block,
            ..ContextDoors::default()
        };
        for (index, placement) in layout.iter().enumerate() {
            if index == here_index {
                continue;
            }
            let offset = Vec3::new(
                (placement.grid_x - here.grid_x) as f32 * RDB_BLOCK_SIDE,
                0.0,
                (placement.grid_z - here.grid_z) as f32 * RDB_BLOCK_SIDE,
            );
            let Ok((scene, _)) = dungeon_view::load_room_scene(placement, &self.project, library) else {
                continue;
            };
            // Its doors, in the game's order, for the doors the game switches off.
            let origin = crate::export::cell_origin(placement);
            let cell_doors = crate::doors::action_doors_in_order(&scene.objects, &model_tags)
                .into_iter()
                .map(|door| origin + scene.objects[door].position);
            match index < here_index {
                true => doors.before.extend(cell_doors),
                false => doors.after.extend(cell_doors),
            }
            if placement.is_starting_block {
                doors.exit = crate::doors::exit_door_point(&scene.objects, library);
            }
            let cell_index = cells.len();
            objects.extend(scene.objects.into_iter().map(|mut object| {
                object.position += offset;
                object.group_index = cell_index;
                object
            }));
            outlines.push(RoomOutline::cell(offset));
            let label = match placement.room.and_then(|room_id| self.project.room(room_id)) {
                Some(room) => format!("{} (your cell)", room.name),
                None => placement.block_name.clone(),
            };
            cells.push(ContextCell { label, layout_index: index });
        }
        Some(DungeonContext {
            dungeon: dungeon.clone(),
            scene: Scene { block_name: String::new(), objects },
            outlines,
            cells,
            doors,
        })
    }

    /// What Daggerfall Unity picks `dungeon`'s textures from, as an export
    /// gives it; `None` for rooms and for project dungeons not yet placed.
    fn dungeon_identity(&mut self, dungeon: &OpenedItem) -> Option<DungeonIdentity> {
        let library = self.library.as_mut()?;
        let world = library.world_map();
        match dungeon {
            OpenedItem::DaggerfallDungeon(index) => {
                crate::export::daggerfall_dungeon_identity(library.dungeons().get(*index)?, world.as_deref())
            }
            OpenedItem::Project(id) => {
                // New dungeons' location ids depend on every new dungeon's place; work them out again only then.
                let key: Vec<NewDungeonPlace> = self
                    .project
                    .dungeons()
                    .into_iter()
                    .filter(|dungeon| dungeon.location.replaces.is_none())
                    .map(|dungeon| (dungeon.id, dungeon.location.map_pixel))
                    .collect();
                if self.new_location_ids.as_ref().is_none_or(|(cached, _)| *cached != key) {
                    let ids = crate::export::new_location_ids(&self.project, &library.location_ids());
                    self.new_location_ids = Some((key, ids));
                }
                let ids = &self.new_location_ids.as_ref()?.1;
                let dungeon = self.project.dungeon(*id)?;
                crate::export::dungeon_identity(dungeon, &library.dungeons(), world.as_deref(), ids)
            }
            _ => None,
        }
    }

    /// Where `dungeon` stands, which the names of the people in it are made
    /// from: a Daggerfall dungeon's place, or the one a replacement takes;
    /// a new dungeon's is not known until the game loads it.
    fn dungeon_person_place(
        &mut self,
        dungeon: &OpenedItem,
    ) -> Result<crate::people::PersonPlace, &'static str> {
        let library = self.library.as_mut().ok_or("No game data")?;
        let definitions = library.dungeons();
        let definition = match dungeon {
            OpenedItem::DaggerfallDungeon(index) => definitions.get(*index),
            OpenedItem::Project(id) => {
                let project_dungeon = self.project.dungeon(*id).ok_or("No such dungeon")?;
                let replaced = project_dungeon.location.replaces.as_ref().ok_or(
                    "Made up where the person stands: a new dungeon's place in its region is given when \
                     Daggerfall Unity loads it",
                )?;
                definitions.iter().find(|definition| replaced.is(definition))
            }
            _ => None,
        };
        let definition = definition.ok_or(room_editor::NO_DUNGEON_FOR_NAMES)?;
        let classic = definition.classic.ok_or(room_editor::NO_DUNGEON_FOR_NAMES)?;
        Ok(crate::people::PersonPlace {
            region_index: definition.region_index,
            index_in_region: classic.location_index,
            building_key: 0,
        })
    }

    /// The rooms of `dungeon`, in layout order.
    fn dungeon_layout(&mut self, dungeon: &OpenedItem) -> Option<Vec<ProjectDungeonRoom>> {
        match dungeon {
            OpenedItem::DaggerfallDungeon(index) => {
                Some(ProjectDungeonRoom::layout_of(self.library.as_mut()?.dungeons().get(*index)?))
            }
            OpenedItem::Project(id) => Some(self.project.dungeon(*id)?.rooms.clone()),
            _ => None,
        }
    }

    /// Opens the cell at `layout_index` in `dungeon`, double-clicked among the
    /// faded cells around another one, keeping `dungeon` around it.
    ///
    /// From a cell window of the dungeon open in the main window, the cell opens
    /// in a cell window the same way; otherwise it opens in the main window.
    fn open_context_cell(
        &mut self,
        context: &egui::Context,
        dungeon: OpenedItem,
        layout_index: usize,
        from_window: bool,
    ) {
        if from_window && self.opened_item.as_ref() == Some(&dungeon) {
            if let Workspace::Dungeon(view) = &self.workspace {
                let room_index = (0..)
                    .map_while(|index| view.room(index).map(|_| index))
                    .find(|&index| view.layout_index(index) == Some(layout_index));
                if let Some(room_index) = room_index {
                    self.open_room_window(context, room_index);
                    return;
                }
            }
        }
        let Some(placement) =
            self.dungeon_layout(&dungeon).and_then(|layout| layout.get(layout_index).cloned())
        else {
            return;
        };
        match placement.room {
            Some(room_id) => self.open_project_item(room_id),
            None => self.open_room(&placement.block_name),
        }
        if let Workspace::Room(editor) = &mut self.workspace {
            editor.set_home_placement(dungeon.clone(), layout_index);
            editor.select_used_in(Some(dungeon));
        }
    }

    /// The cell open in the main window: its block name and project room.
    fn main_cell(&self) -> Option<(String, Option<ItemId>)> {
        match self.opened_item.clone()? {
            OpenedItem::DaggerfallRoom(block_name) => Some((block_name, None)),
            OpenedItem::Project(id) if matches!(self.workspace, Workspace::Room(_)) => {
                let block_name =
                    self.project.room(id).and_then(|room| room.based_on.clone()).unwrap_or_default();
                Some((block_name, Some(id)))
            }
            _ => None,
        }
    }

    /// "Used in" list for the cell open in the main window.
    fn main_used_in(&mut self) -> Vec<UsedInEntry> {
        match self.opened_item.clone() {
            Some(OpenedItem::DaggerfallRoom(block_name)) => self.used_in(&block_name, None),
            Some(OpenedItem::Project(id)) if matches!(self.workspace, Workspace::Room(_)) => {
                let block_name =
                    self.project.room(id).and_then(|room| room.based_on.clone()).unwrap_or_default();
                self.used_in(&block_name, Some(id))
            }
            _ => Vec::new(),
        }
    }

    /// "Used in" lists for each room window, in window order.
    fn room_window_used_in(&mut self) -> Vec<Vec<UsedInEntry>> {
        let cells: Vec<Option<(String, Option<ItemId>)>> = self
            .room_windows
            .iter()
            .map(|window| {
                let Workspace::Dungeon(view) = &self.workspace else { return None };
                let room = window.dungeon_room_index.and_then(|index| view.room(index))?;
                Some((room.block_name.clone(), room.project_room))
            })
            .collect();
        cells
            .into_iter()
            .map(|cell| match cell {
                Some((block_name, project_room)) => self.used_in(&block_name, project_room),
                None => Vec::new(),
            })
            .collect()
    }

    /// Shows the project's current names in the open workspace and room windows.
    fn show_current_names(&mut self) {
        let Some(OpenedItem::Project(id)) = self.opened_item else { return };
        let Some(name) = self.project.find(id).map(|entry| entry.name().to_string()) else { return };
        match &mut self.workspace {
            Workspace::Room(editor) if editor.title() != name => {
                editor.set_title(name.clone());
                editor.set_project_room_name(Some(name));
            }
            Workspace::Dungeon(view) => {
                view.name = name;
                // The type can change in the properties panel; room windows preview enemies for it.
                view.dungeon_type = self.project.dungeon(id).and_then(|dungeon| dungeon.dungeon_type);
                for window in &mut self.room_windows {
                    window.editor.set_dungeon_type(view.dungeon_type);
                    let Some(room) = window.dungeon_room_index.and_then(|index| view.room(index)) else {
                        continue;
                    };
                    let room_name = room
                        .project_room
                        .and_then(|room_id| self.project.room(room_id))
                        .map(|room| room.name.clone());
                    let title =
                        format!("{} in {}", room_name.as_deref().unwrap_or(&room.block_name), view.name);
                    if window.editor.title() != title {
                        window.editor.set_title(title);
                        window.editor.set_project_room_name(room_name);
                    }
                }
            }
            _ => {}
        }
    }

    /// Name a copy of the Daggerfall room `block_name` is based on: the user's
    /// name for the room if it has one, otherwise the block name without `.RDB`.
    fn room_copy_base_name(&self, block_name: &str) -> String {
        match self.custom_names.room_name(block_name) {
            Some(custom_name) => custom_name.to_string(),
            None => block_name.trim_end_matches(".RDB").to_string(),
        }
    }

    /// Adds a copy of the Daggerfall room or dungeon `item` to the project and returns its id.
    ///
    /// A room's objects come from `room_scene` if given (the open room),
    /// otherwise from the game data. The copy goes in `folder`, or the top level.
    fn add_copy_of_daggerfall_item(
        &mut self,
        item: &OpenedItem,
        room_scene: Option<&Scene>,
        folder: Option<ItemId>,
    ) -> Option<ItemId> {
        let library = self.library.as_mut()?;
        let entry = match item {
            OpenedItem::DaggerfallRoom(room_name) => {
                let scene = match room_scene {
                    Some(scene) => scene.clone(),
                    None => match library.rdb_block(room_name) {
                        Ok(block) => Scene::from_rdb_block(&block),
                        Err(error) => {
                            self.error_message = Some(format!("Could not copy {room_name}: {error}"));
                            return None;
                        }
                    },
                };
                let base_name = self.room_copy_base_name(room_name);
                ProjectEntry::Room(ProjectRoom {
                    id: self.project.new_id(),
                    name: self.project.copy_name(&base_name),
                    based_on: Some(room_name.clone()),
                    category: CellCategory::of_copy(Some(room_name)),
                    objects: scene.objects,
                })
            }
            OpenedItem::DaggerfallDungeon(dungeon_index) => {
                let dungeons = library.dungeons();
                let definition = dungeons.get(*dungeon_index)?;
                ProjectEntry::Dungeon(ProjectDungeon {
                    id: self.project.new_id(),
                    name: self.project.copy_name(&definition.name),
                    region_name: definition.region_name.to_string(),
                    dungeon_type: definition.dungeon_type,
                    based_on: Some(definition.name.clone()),
                    rooms: ProjectDungeonRoom::layout_of(definition),
                    location: DungeonLocation::copied_from(definition),
                })
            }
            _ => return None,
        };
        let id = entry.id();
        self.status_message = format!("Created {} in the project", entry.name());
        self.project.add(entry, folder);
        Some(id)
    }

    /// Turns the open Daggerfall room or dungeon into a copy in the project.
    ///
    /// The open editor and room windows keep their camera and selection; only
    /// what they edit changes. A copied dungeon's rooms are still Daggerfall's
    /// own, so they stay read-only until they are copied too.
    fn convert_open_item_to_copy(&mut self) {
        let Some(item) = self.opened_item.clone() else { return };
        let open_room_scene = match &self.workspace {
            Workspace::Room(editor) => Some(editor.scene.clone()),
            _ => None,
        };
        let Some(id) = self.add_copy_of_daggerfall_item(&item, open_room_scene.as_ref(), None) else {
            return;
        };
        self.opened_item = Some(OpenedItem::Project(id));
        let message = format!("{}. You can now edit it.", self.status_message);
        let category = self.project.room(id).map(|room| room.category);
        match &mut self.workspace {
            Workspace::Room(editor) => {
                editor.set_read_only(false);
                editor.set_cell_category(category);
                editor.set_status_message(message);
            }
            Workspace::Dungeon(_) | Workspace::Exterior(_) | Workspace::Interior(_) | Workspace::Empty => {}
        }
        // Titles and names follow the copy from the next frame on.
        self.show_current_names();
    }

    /// Copies the Daggerfall room edited in the room window `viewport` for the
    /// open dungeon, copying the dungeon first if it is Daggerfall's own too.
    /// The room window then edits the copy.
    fn copy_room_in_open_dungeon(&mut self, viewport: egui::ViewportId) {
        if matches!(self.opened_item, Some(OpenedItem::DaggerfallDungeon(_))) {
            self.convert_open_item_to_copy();
        }
        let Some(OpenedItem::Project(dungeon_id)) = self.opened_item else { return };
        let Some(window) = self.room_windows.iter().find(|window| window.viewport_id == viewport) else {
            return;
        };
        let Some(room_index) = window.dungeon_room_index else { return };
        let objects = window.editor.scene.objects.clone();
        let Workspace::Dungeon(view) = &self.workspace else { return };
        let (Some(layout_index), Some(room)) = (view.layout_index(room_index), view.room(room_index)) else {
            return;
        };
        let block_name = room.block_name.clone();

        let room_id = self.project.new_id();
        let name = self.project.copy_name(&self.room_copy_base_name(&block_name));
        let dungeon_name =
            self.project.dungeon(dungeon_id).map(|dungeon| dungeon.name.clone()).unwrap_or_default();
        self.project.add(
            ProjectEntry::Room(ProjectRoom {
                id: room_id,
                name: name.clone(),
                category: CellCategory::of_copy(Some(&block_name)),
                based_on: Some(block_name),
                objects,
            }),
            None,
        );
        if let Some(placement) =
            self.project.dungeon_mut(dungeon_id).and_then(|dungeon| dungeon.rooms.get_mut(layout_index))
        {
            placement.room = Some(room_id);
        }
        if let Workspace::Dungeon(view) = &mut self.workspace {
            view.set_project_room(room_index, room_id);
        }
        if let Some(window) = self.room_windows.iter_mut().find(|window| window.viewport_id == viewport) {
            window.editor.set_read_only(false);
            window.editor.set_cell_category(self.project.room(room_id).map(|room| room.category));
            window
                .editor
                .set_status_message(format!("Created {name} for {dungeon_name}. You can now edit it."));
        }
        self.status_message = format!("Created {name} for {dungeon_name}");
        self.show_current_names();
    }

    /// Carries out what the user asked for in the project panel.
    fn handle_panel_action(&mut self, action: PanelAction) {
        match action {
            PanelAction::Open(item) => self.open_item(&item),
            PanelAction::Created(id) => {
                if let Some(name) = self.project.find(id).map(|entry| entry.name().to_string()) {
                    self.open_item(&OpenedItem::Project(id));
                    self.status_message = format!("Created {name} in the project");
                }
            }
            PanelAction::CopyFromDaggerfall { item, folder, open } => {
                if open && self.opened_item.as_ref() == Some(&item) {
                    self.convert_open_item_to_copy();
                } else if let Some(id) = self.add_copy_of_daggerfall_item(&item, None, folder) {
                    if open {
                        self.open_project_item(id);
                    }
                }
            }
            PanelAction::Duplicate(id) => {
                self.store_workspace_changes();
                if let Some(copy_id) = self.project.duplicate(id) {
                    let name =
                        self.project.find(copy_id).map(|entry| entry.name().to_string()).unwrap_or_default();
                    self.status_message = format!("Created {name}");
                }
            }
            PanelAction::Delete(id) => self.delete_prompt = Some(id),
        }
    }

    /// Deletes the project room or dungeon `id`, closing it first if it is open.
    ///
    /// Dungeons that used a deleted room use the Daggerfall room again; one
    /// that is open is closed first so it does not keep showing the room.
    fn delete_project_item(&mut self, id: ItemId) {
        let open_dungeon_uses_room = match self.opened_item {
            Some(OpenedItem::Project(open_id)) => self
                .project
                .dungeon(open_id)
                .is_some_and(|dungeon| dungeon.rooms.iter().any(|placement| placement.room == Some(id))),
            _ => false,
        };
        if self.opened_item == Some(OpenedItem::Project(id)) || open_dungeon_uses_room {
            self.set_workspace(Workspace::Empty, None);
        }
        self.project.stop_using_room(id);
        if let Some(entry) = self.project.remove(id) {
            self.status_message = format!("Deleted {}", entry.name());
        }
    }

    // -----------------------------------------------------------------------
    // Project files
    // -----------------------------------------------------------------------

    /// Name of the project, from its file name.
    fn project_title(&self) -> String {
        self.project_path
            .as_deref()
            .and_then(Path::file_stem)
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| UNTITLED_PROJECT_NAME.to_string())
    }

    /// Whether the project changed since it was last saved or opened.
    fn has_unsaved_changes(&mut self) -> bool {
        self.store_workspace_changes();
        !self.project.same_contents(&self.saved_project)
    }

    /// The project room or dungeon open in the workspace, if one is.
    fn opened_project_item(&self) -> Option<ItemId> {
        match self.opened_item {
            Some(OpenedItem::Project(id)) if self.project.find(id).is_some() => Some(id),
            _ => None,
        }
    }

    /// Whether the project room or dungeon `id` (with, for a dungeon, its
    /// cells) has changes that are not saved. Edits still in the workspace
    /// count, as they are stored in the project every frame.
    fn item_has_unsaved_changes(&self, id: ItemId) -> bool {
        self.project.item_differs_from(&self.saved_project, id)
    }

    /// Saves the open cell or dungeon (Ctrl+S): only its changes, and for a
    /// dungeon its cells', are written to the project file. Without an open
    /// project item, or before the project has a file, the whole project is saved.
    fn save_open_item(&mut self) {
        self.store_workspace_changes();
        let (Some(id), Some(path)) = (self.opened_project_item(), self.project_path.clone()) else {
            self.save_project();
            return;
        };
        let name = self.project.find(id).map(|entry| entry.name().to_string()).unwrap_or_default();
        if !self.item_has_unsaved_changes(id) {
            self.status_message = format!("{name} has no unsaved changes");
            return;
        }
        let saved = self.saved_project.with_item_from(&self.project, id);
        match saved.save(&path) {
            Ok(()) => {
                self.saved_project = saved;
                let others = if self.has_unsaved_changes() {
                    "; other changes in the project are still unsaved (File > Save project saves them)"
                } else {
                    ""
                };
                self.status_message = format!("Saved {name} to {}{others}", path.display());
            }
            Err(error) => self.error_message = Some(error),
        }
    }

    /// Runs `action`, first asking what to do with unsaved changes if there are any.
    fn request(&mut self, action: DiscardingAction) {
        if self.has_unsaved_changes() {
            self.unsaved_changes_prompt = Some(action);
        } else {
            self.perform(action);
        }
    }

    /// Runs `action` without asking about unsaved changes.
    fn perform(&mut self, action: DiscardingAction) {
        match action {
            DiscardingAction::NewProject => {
                self.set_workspace(Workspace::Empty, None);
                self.replace_project(Project::default(), None);
                self.status_message = "Started a new project".to_string();
            }
            DiscardingAction::OpenProject => {
                let dialog = rfd::FileDialog::new()
                    .set_title("Open project")
                    .add_filter("Daggerfall Dungeon Maker project", &[PROJECT_FILE_EXTENSION]);
                if let Some(path) = dialog.pick_file() {
                    self.open_project_file(&path);
                }
            }
            DiscardingAction::Exit => {
                self.exit_confirmed = true;
            }
        }
    }

    /// Opens the project saved at `path`, replacing the open one.
    fn open_project_file(&mut self, path: &Path) {
        match Project::load(path) {
            Ok(project) => {
                self.set_workspace(Workspace::Empty, None);
                self.replace_project(project, Some(path.to_path_buf()));
                self.status_message = format!("Opened project {}", path.display());
            }
            Err(error) => self.error_message = Some(error),
        }
    }

    /// Makes `project` the open project, saved at `path`.
    fn replace_project(&mut self, project: Project, path: Option<PathBuf>) {
        self.saved_project = project.clone();
        self.project = project;
        self.project_path = path;
    }

    /// Saves the project to its file, asking for one if it has never been saved.
    /// Returns `false` if it was not saved.
    fn save_project(&mut self) -> bool {
        match self.project_path.clone() {
            Some(path) => self.save_project_to(&path),
            None => self.save_project_as(),
        }
    }

    /// Asks for a file and saves the project to it. Returns `false` if it was not saved.
    fn save_project_as(&mut self) -> bool {
        let dialog = rfd::FileDialog::new()
            .set_title("Save project")
            .add_filter("Daggerfall Dungeon Maker project", &[PROJECT_FILE_EXTENSION])
            .set_file_name(format!("{}.{PROJECT_FILE_EXTENSION}", self.project_title()));
        let Some(mut path) = dialog.save_file() else { return false };
        if path.extension().is_none_or(|extension| !extension.eq_ignore_ascii_case(PROJECT_FILE_EXTENSION)) {
            path.as_mut_os_string().push(format!(".{PROJECT_FILE_EXTENSION}"));
        }
        self.save_project_to(&path)
    }

    /// Saves the project to `path`. Returns `false` (and reports the error) if it could not be written.
    fn save_project_to(&mut self, path: &Path) -> bool {
        self.store_workspace_changes();
        match self.project.save(path) {
            Ok(()) => {
                self.saved_project = self.project.clone();
                self.project_path = Some(path.to_path_buf());
                self.status_message = format!("Saved project {}", path.display());
                true
            }
            Err(error) => {
                self.error_message = Some(error);
                false
            }
        }
    }

    // -----------------------------------------------------------------------
    // Menus, shortcuts and windows
    // -----------------------------------------------------------------------

    /// Handles keyboard shortcuts of the main window.
    fn handle_shortcuts(&mut self, context: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};

        let shortcut = |modifiers, key| {
            context.input_mut(|input| input.consume_shortcut(&KeyboardShortcut::new(modifiers, key)))
        };
        if shortcut(Modifiers::COMMAND | Modifiers::SHIFT, Key::S) {
            self.save_project_as();
        }
        if shortcut(Modifiers::COMMAND, Key::S) {
            self.save_open_item();
        }
        let has_library = self.library.is_some();
        if shortcut(Modifiers::COMMAND | Modifiers::SHIFT, Key::O) && self.can_open_dungeons() {
            self.dungeon_browser.is_open = true;
        }
        if shortcut(Modifiers::COMMAND, Key::O) && has_library {
            self.room_browser.is_open = true;
        }

        let Some(library) = self.library.as_mut() else { return };
        match &mut self.workspace {
            Workspace::Room(editor) => editor.handle_shortcuts(context, library),
            Workspace::Dungeon(view) => view.handle_shortcuts(context, library),
            Workspace::Exterior(view) => view.handle_shortcuts(context, library),
            Workspace::Interior(view) => view.handle_shortcuts(context, library),
            Workspace::Empty => {}
        }
    }

    /// Whether the loaded game data includes dungeon layouts.
    fn can_open_dungeons(&self) -> bool {
        self.library.as_ref().is_some_and(Arena2Library::has_dungeons)
    }

    /// Draws the main window's menu bar.
    fn show_menu_bar(&mut self, context: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(context, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| self.file_menu(ui));
                ui.menu_button("Import", |ui| self.import_menu(ui));
                let library = self.library.as_mut();
                if let (Workspace::Room(editor), Some(_)) = (&mut self.workspace, library.as_ref()) {
                    ui.menu_button("Edit", |ui| editor.edit_menu(ui));
                }
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.project_panel.is_open, "Project panel");
                    match (&mut self.workspace, library) {
                        (Workspace::Room(editor), Some(library)) => {
                            ui.separator();
                            editor.view_menu(ui, library);
                        }
                        (Workspace::Dungeon(view), Some(library)) => {
                            ui.separator();
                            view.view_menu(ui, library);
                        }
                        (Workspace::Exterior(view), Some(library)) => {
                            ui.separator();
                            view.view_menu(ui, library);
                        }
                        (Workspace::Interior(view), Some(library)) => {
                            ui.separator();
                            view.view_menu(ui, library);
                        }
                        _ => {}
                    }
                    if matches!(self.workspace, Workspace::Room(_) | Workspace::Dungeon(_)) {
                        ui.separator();
                        viewport_tools::dungeon_texture_menu(
                            ui,
                            &mut self.texture_setting,
                            self.game_texture_mode,
                        );
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("Viewport controls").clicked() {
                        self.show_controls_window = true;
                        ui.close_menu();
                    }
                });
                if let Workspace::Room(editor) = &mut self.workspace {
                    ui.separator();
                    editor.toolbar(ui);
                }
            });
        });
    }

    /// Adds the Import menu entries.
    fn import_menu(&mut self, ui: &mut egui::Ui) {
        let hover = "Imports a 3D model of your own (a Wavefront OBJ file with its MTL materials and their \
                     textures, a glTF file: .gltf or .glb, or a Blender file: .blend, which Blender converts; \
                     one unit is a metre) to place in cells like Daggerfall's models. It is listed under Custom \
                     Models in the asset palette, and a .dfmod export takes it into the game.";
        let converting = self.blend_conversion.is_some();
        if ui.add_enabled(!converting, egui::Button::new("Import Model…")).on_hover_text(hover).clicked() {
            ui.close_menu();
            self.import_model();
        }
        let blender = match self.blender() {
            Some(program) => format!("Now using {}.", program.display()),
            None => "Blender was not found on this computer.".to_string(),
        };
        let hover = format!(
            "Chooses the Blender program (blender.exe) that converts .blend files when they are imported. \
             Blender is free from blender.org; it is found on its own where its installer or Steam puts it. \
             {blender}"
        );
        if ui.button("Locate Blender…").on_hover_text(hover).clicked() {
            ui.close_menu();
            self.locate_blender();
        }
    }

    /// The Blender program `.blend` files are converted with: the one the
    /// user chose, or else one installed in the usual places.
    fn blender(&self) -> Option<PathBuf> {
        self.blender_program.clone().filter(|program| program.is_file()).or_else(crate::blender::find_blender)
    }

    /// Asks where Blender is, and keeps the answer. Returns whether one was chosen.
    fn locate_blender(&mut self) -> bool {
        let mut dialog = rfd::FileDialog::new().set_title("Where is Blender?");
        if cfg!(windows) {
            dialog = dialog.add_filter("Blender (blender.exe)", &["exe"]);
        }
        if let Some(folder) = self.blender().as_deref().and_then(Path::parent) {
            dialog = dialog.set_directory(folder);
        }
        let Some(program) = dialog.pick_file() else { return false };
        let program = crate::blender::blender_program(&program);
        self.status_message = format!("Blender at {} converts .blend files", program.display());
        self.blender_program = Some(program);
        true
    }

    /// Asks for a model file (OBJ, glTF, GLB or .blend) and imports it as one of the user's models.
    fn import_model(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import a model")
            .add_filter("3D model (OBJ, glTF, GLB, Blender)", &crate::custom_models::MODEL_EXTENSIONS)
            .add_filter("Wavefront OBJ model", &["obj"])
            .add_filter("glTF model", &["gltf", "glb"])
            .add_filter("Blender file", &["blend"])
            .pick_file()
        else {
            return;
        };
        if crate::blender::is_blend_file(&path) {
            self.start_blend_conversion(&path);
            return;
        }
        let result = self.custom_models.import(&path);
        self.finish_import(result);
    }

    /// Starts Blender converting the `.blend` file at `path`, to import it once
    /// it is done (see [`Self::poll_blend_conversion`]). Without Blender, offers
    /// to locate it first.
    fn start_blend_conversion(&mut self, path: &Path) {
        let blender = match self.blender() {
            Some(blender) => blender,
            None => {
                let answer = rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Info)
                    .set_title("Blender is needed")
                    .set_description(
                        "A .blend file is imported by Blender, which converts it to glTF without opening a \
                         window. Blender was not found where its installer or Steam puts it.\n\nShow where \
                         Blender (blender.exe) is? Blender is free from blender.org.",
                    )
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show();
                if answer != rfd::MessageDialogResult::Yes || !self.locate_blender() {
                    return;
                }
                match self.blender() {
                    Some(blender) => blender,
                    None => return,
                }
            }
        };
        let Some(work_folder) = self.custom_models.conversions_folder() else {
            self.error_message = Some("This system has no folder to keep imported models in".to_string());
            return;
        };
        match BlendConversion::start(&blender, path, &work_folder) {
            Ok(conversion) => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                self.status_message = format!("Blender is converting {name} to import it…");
                self.blend_conversion = Some(conversion);
            }
            Err(error) => self.error_message = Some(format!("Could not import the model: {error}")),
        }
    }

    /// Imports the `.blend` file Blender was converting, once it is done.
    fn poll_blend_conversion(&mut self, context: &egui::Context) {
        let Some(conversion) = &self.blend_conversion else { return };
        let Some(result) = conversion.poll() else {
            context.request_repaint_after(std::time::Duration::from_millis(200));
            return;
        };
        let source = conversion.source().to_path_buf();
        let result = result.and_then(|converted| self.custom_models.import_converted(&converted, &source));
        // Dropping the conversion removes the converted file, which the import has copied.
        self.blend_conversion = None;
        self.finish_import(result);
    }

    /// Shows the newly imported model everywhere, or tells what went wrong.
    /// If the model names textures (or a material file) that could not be
    /// read, offers to choose its texture image now.
    fn finish_import(&mut self, result: Result<crate::custom_models::ImportOutcome, String>) {
        let crate::custom_models::ImportOutcome { model, missing, missing_material_file, colour_only } =
            match result {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.error_message = Some(format!("Could not import the model: {error}"));
                    self.status_message = "The model was not imported".to_string();
                    return;
                }
            };
        self.refresh_custom_model(&model);
        // Listed under its file's name, which the user can change like any model's.
        self.custom_names.set_model_name(model.id, &model.name);
        if let Workspace::Room(editor) = &mut self.workspace {
            editor.show_custom_models();
        }
        for window in &mut self.room_windows {
            window.editor.show_custom_models();
        }
        let size = model.mesh.size();
        self.status_message = format!(
            "Imported {} as model {} ({:.1} × {:.1} × {:.1} m); it is under Custom Models in the asset palette",
            model.name, model.id, size.x, size.y, size.z
        );
        // A missing MTL file is offered first: it names the textures, and tells the materials apart.
        if let Some(material_file) = missing_material_file {
            self.offer_material_file(&model, &material_file);
        } else if !missing.is_empty() {
            self.offer_texture(&model, &missing_reason(&missing));
        } else if !colour_only.is_empty() {
            self.offer_texture(&model, &colour_only_reason(&colour_only));
        }
    }

    /// Asks whether to find the MTL file `material_file` the imported
    /// `model` names, which was not found beside it, and uses the one chosen.
    fn offer_material_file(&mut self, model: &crate::custom_models::CustomModel, material_file: &str) {
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("The model's material file was not found")
            .set_description(format!(
                "{} needs its material file {material_file}, which holds its materials and names their \
                 textures, but it is not beside the model, so the model is drawn in plain colour. (An OBJ file \
                 holds no image itself.)\n\nDo you want to find the MTL file now? You can also add it later: \
                 right-click the model under Custom Models and choose Choose texture…, which takes an MTL file \
                 as well as an image.",
                model.name
            ))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        let chosen = match answer {
            rfd::MessageDialogResult::Yes => self.pick_material_file(model.id, material_file),
            _ => None,
        };
        match chosen {
            Some(path) => self.apply_material_file(model.id, &path),
            None => {
                self.status_message = format!(
                    "Imported {} without its material file {material_file}; to add it (or a texture), \
                     right-click it under Custom Models and choose Choose texture…",
                    model.name
                )
            }
        }
    }

    /// Asks for an MTL file for the imported model `model_id`, starting
    /// beside the file it was imported from.
    fn pick_material_file(&self, model_id: u32, wanted: &str) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new()
            .set_title(format!("Find the material file {wanted}"))
            .add_filter("MTL material file", &["mtl"]);
        let source = self.custom_models.get(model_id).map(|model| PathBuf::from(&model.source));
        if let Some(folder) = source.as_deref().and_then(Path::parent).filter(|folder| folder.is_dir()) {
            dialog = dialog.set_directory(folder);
        }
        dialog.pick_file()
    }

    /// Gives the imported model `model_id` the materials of the MTL file at
    /// `path` (reading the model again with it), and offers to choose a
    /// texture for any it names that are still missing.
    fn apply_material_file(&mut self, model_id: u32, path: &Path) {
        let file = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        match self.custom_models.set_materials(model_id, path) {
            Ok(outcome) => {
                self.refresh_custom_model(&outcome.model);
                let parts = outcome.model.parts.len();
                let textured = outcome.model.parts.iter().filter(|part| part.textured).count();
                self.status_message = format!(
                    "{} now uses the materials of {file}: {textured} of its {parts} parts have a texture",
                    outcome.model.name
                );
                if !outcome.missing.is_empty() {
                    self.offer_texture(&outcome.model, &missing_reason(&outcome.missing));
                } else if !outcome.colour_only.is_empty() {
                    let reason = format!("{file} {}", colour_only_reason(&outcome.colour_only));
                    self.offer_texture(&outcome.model, &reason);
                }
            }
            Err(error) => self.error_message = Some(format!("Could not use {file}: {error}")),
        }
    }

    /// Says why the imported `model` has no texture (`reason`, which follows
    /// its name), and offers to choose its texture image.
    fn offer_texture(&mut self, model: &crate::custom_models::CustomModel, reason: &str) {
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("The model has no texture")
            .set_description(format!(
                "{}: {reason}.\n\nWithout a texture it is drawn in plain colour. (A GLB file, or a .blend file \
                 with its images packed, carries its textures inside.)\n\nChoose the texture image to draw it \
                 with now? You can also do it later: right-click the model under Custom Models and choose \
                 Change texture…",
                model.name
            ))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if answer == rfd::MessageDialogResult::Yes {
            self.change_custom_texture(model.id, None);
        } else {
            self.status_message = format!(
                "{} has no texture ({reason}); right-click it under Custom Models to choose one",
                model.name
            );
        }
    }

    /// Does what the user asked with one of their imported models.
    fn handle_custom_model_request(&mut self, request: room_editor::CustomModelRequest) {
        match request {
            room_editor::CustomModelRequest::ChangeTexture { model_id, part } => {
                self.change_custom_texture(model_id, part)
            }
            room_editor::CustomModelRequest::Delete { model_id } => self.delete_custom_model(model_id),
        }
    }

    /// Deletes the imported model `model_id`, after asking, and every copy of
    /// it placed in the project's cells and prefabs (which could not be drawn
    /// or exported without it).
    fn delete_custom_model(&mut self, model_id: u32) {
        let Some(model) = self.custom_models.get(model_id).cloned() else { return };
        let name = self.custom_names.model_name(model_id).unwrap_or(&model.name).to_string();
        // The open cell's or dungeon's latest changes count too.
        self.store_workspace_changes();
        let rooms: Vec<String> = self
            .project
            .rooms_using_model(model_id)
            .into_iter()
            .map(|(room, count)| match count {
                1 => room.to_string(),
                _ => format!("{room} ({count} copies)"),
            })
            .collect();
        let prefabs: Vec<String> = self
            .prefabs
            .prefabs_using_model(model_id)
            .into_iter()
            .map(|prefab| format!("the prefab {prefab}"))
            .collect();
        let mut description = format!(
            "Delete the model {name} (model {model_id}) from Custom Models? Its saved files are removed from the \
             editor's data folder, which cannot be undone. The file it was imported from is not touched:\n{}",
            model.source
        );
        let places: Vec<String> = rooms.iter().chain(&prefabs).cloned().collect();
        if !places.is_empty() {
            description.push_str(&format!(
                "\n\nIt is placed in your project: {}. Those copies are removed too (save the project to keep \
                 that); the open cell or dungeon is opened again without them.",
                places.join(", ")
            ));
        }
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title(format!("Delete {name}?"))
            .set_description(description)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if answer != rfd::MessageDialogResult::Yes {
            return;
        }
        if let Err(error) = self.custom_models.delete(model_id) {
            self.error_message = Some(format!("Could not delete {name}: {error}"));
            return;
        }

        // The project's copies: close what shows them, remove them, and show it again.
        let reopen = (!rooms.is_empty()).then(|| self.opened_item.clone()).flatten();
        if !rooms.is_empty() {
            self.set_workspace(Workspace::Empty, None);
        }
        let removed = self.project.remove_model(model_id) + self.prefabs.remove_model(model_id);
        if let Some(item) = reopen {
            self.open_item(&item);
        }

        let textures: Vec<crate::arena2::TextureKey> = model.textures.iter().map(|(key, _)| *key).collect();
        if let Some(library) = self.library.as_mut() {
            library.remove_custom_model(model_id, &textures);
        }
        if let (Some(renderer), Some(render_state)) = (self.renderer.as_mut(), self.render_state.as_ref()) {
            renderer.forget_model(render_state, model_id, &textures);
        }
        self.custom_names.set_model_name(model_id, "");
        let favorite = crate::custom_names::Favorite::Model { model_id };
        if self.custom_names.is_favorite(favorite) {
            self.custom_names.toggle_favorite(favorite);
        }
        if let Workspace::Room(editor) = &mut self.workspace {
            editor.forget_custom_model(model_id);
        }
        for window in &mut self.room_windows {
            window.editor.forget_custom_model(model_id);
        }
        self.status_message = match removed {
            0 => format!("Deleted the model {name}"),
            1 => format!("Deleted the model {name} and its copy in your project"),
            count => format!("Deleted the model {name} and its {count} copies in your project"),
        };
    }

    /// Asks for an image and draws part `part` of the imported model
    /// `model_id` with it (for `None`, every part without a texture).
    fn change_custom_texture(&mut self, model_id: u32, part: Option<usize>) {
        let name = self
            .custom_models
            .get(model_id)
            .map_or_else(|| format!("model {model_id}"), |model| model.name.clone());
        let mut dialog = rfd::FileDialog::new()
            .set_title(format!("Choose the texture of {name}, or its MTL material file"))
            .add_filter("Image or MTL material file", &["png", "jpg", "jpeg", "bmp", "tga", "mtl"])
            .add_filter("Image (PNG, JPG, BMP, TGA)", &["png", "jpg", "jpeg", "bmp", "tga"])
            .add_filter("MTL material file", &["mtl"]);
        let source = self.custom_models.get(model_id).map(|model| PathBuf::from(&model.source));
        if let Some(folder) = source.as_deref().and_then(Path::parent).filter(|folder| folder.is_dir()) {
            dialog = dialog.set_directory(folder);
        }
        let Some(image) = dialog.pick_file() else { return };
        // An MTL file gives the model its materials, with their own textures.
        if image.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("mtl")) {
            self.apply_material_file(model_id, &image);
            return;
        }
        match self.custom_models.set_texture(model_id, part, &image) {
            Ok(model) => {
                self.refresh_custom_model(&model);
                let file = image.file_name().unwrap_or_default().to_string_lossy();
                self.status_message = format!("{} is now drawn with {file}", model.name);
            }
            Err(error) => {
                self.error_message = Some(format!("Could not change the texture of {name}: {error}"))
            }
        }
    }

    /// Shows the imported `model` as it is now: in the game data's models,
    /// and drawn afresh where it was drawn before.
    fn refresh_custom_model(&mut self, model: &crate::custom_models::CustomModel) {
        if let Some(library) = self.library.as_mut() {
            library.add_custom_model(model.mesh.clone(), model.textures.clone(), model.files.clone());
        }
        if let (Some(renderer), Some(render_state)) = (self.renderer.as_mut(), self.render_state.as_ref()) {
            let textures: Vec<crate::arena2::TextureKey> =
                model.textures.iter().map(|(key, _)| *key).collect();
            renderer.forget_model(render_state, model.id, &textures);
        }
    }

    /// Adds the File menu entries.
    fn file_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("New project").clicked() {
            self.request(DiscardingAction::NewProject);
            ui.close_menu();
        }
        if ui.button("Open project…").clicked() {
            self.request(DiscardingAction::OpenProject);
            ui.close_menu();
        }
        // The open cell or dungeon alone, then the whole project.
        let item_kind = match self.opened_project_item().and_then(|id| self.project.find(id)) {
            Some(ProjectEntry::Dungeon(_)) => Some("dungeon"),
            Some(ProjectEntry::Room(_)) => Some("cell"),
            _ => None,
        };
        let item_button = egui::Button::new(format!("Save {}", item_kind.unwrap_or("cell or dungeon")))
            .shortcut_text("Ctrl+S");
        let item_response = ui
            .add_enabled(item_kind.is_some(), item_button)
            .on_hover_text(
                "Saves only the open cell or dungeon (a dungeon with its cells) to the project file",
            )
            .on_disabled_hover_text("Open one of your project's cells or dungeons to save it on its own");
        if item_response.clicked() {
            self.save_open_item();
            ui.close_menu();
        }
        let project_response = ui.button("Save project").on_hover_text(
            "Saves every change in the project: all cells and dungeons, folders and mod details",
        );
        if project_response.clicked() {
            self.save_project();
            ui.close_menu();
        }
        let save_as = egui::Button::new("Save project as…").shortcut_text("Ctrl+Shift+S");
        if ui
            .add(save_as)
            .on_hover_text("Saves the project with all its changes as a new project file, which stays open")
            .clicked()
        {
            self.save_project_as();
            ui.close_menu();
        }
        let export_response =
            ui.add_enabled(self.can_open_dungeons(), egui::Button::new("Export to Daggerfall Unity…"));
        if export_response.clicked() {
            self.export_window.open();
            ui.close_menu();
        }
        export_response.on_disabled_hover_text("Needs the game data, including MAPS.BSA");
        ui.separator();
        let has_library = self.library.is_some();
        if ui.add_enabled(has_library, egui::Button::new("Open cell…").shortcut_text("Ctrl+O")).clicked() {
            self.room_browser.is_open = true;
            ui.close_menu();
        }
        let dungeon_button = egui::Button::new("Open dungeon…").shortcut_text("Ctrl+Shift+O");
        let dungeon_response = ui.add_enabled(self.can_open_dungeons(), dungeon_button);
        if dungeon_response.clicked() {
            self.dungeon_browser.is_open = true;
            ui.close_menu();
        }
        if has_library && !self.can_open_dungeons() {
            dungeon_response.on_disabled_hover_text("MAPS.BSA was not found in the ARENA2 folder.");
        }
        let has_workspace = !matches!(self.workspace, Workspace::Empty);
        if ui.add_enabled(has_workspace, egui::Button::new("Close")).clicked() {
            self.set_workspace(Workspace::Empty, None);
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Set ARENA2 folder…").clicked() {
            self.arena2_prompt.open(None);
            ui.close_menu();
        }
        let unity_hint = match &self.unity_folder {
            Some(folder) if self.unity_text.is_some() => {
                format!("Building names come from Daggerfall Unity in {}", folder.display())
            }
            _ => "Choose where Daggerfall Unity is installed: its word lists name the buildings of settlements \
                  (shops, taverns, banks)"
                .to_string(),
        };
        if ui.button("Set Daggerfall Unity folder…").on_hover_text(unity_hint).clicked() {
            ui.close_menu();
            self.choose_unity_folder();
        }
        ui.separator();
        if ui.button("Exit").clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Draws the main window's status bar.
    fn show_status_bar(&mut self, context: &egui::Context) {
        let status = match &self.workspace {
            Workspace::Room(editor) => editor.status_message().to_string(),
            Workspace::Dungeon(view) => view.status_text(&self.custom_names),
            Workspace::Exterior(view) => match self.library.as_ref() {
                Some(library) => view.status_text(&self.custom_names, &library_locations(library)),
                None => self.status_message.clone(),
            },
            Workspace::Interior(view) => view.status_text(),
            Workspace::Empty => self.status_message.clone(),
        };
        egui::TopBottomPanel::bottom("status_bar").show(context, |ui| {
            ui.horizontal(|ui| {
                ui.label(status);
                self.show_test_button(ui);
            });
        });
    }

    /// Draws the project panel and carries out what the user asks for in it.
    fn show_project_panel(&mut self, context: &egui::Context) {
        let title = self.project_title();
        let action = self.project_panel.show(
            context,
            &mut self.project,
            &title,
            &self.custom_names,
            self.opened_item.as_ref(),
        );
        if let Some(action) = action {
            self.handle_panel_action(action);
        }
    }

    /// Draws the main window's workspace.
    fn show_workspace(&mut self, context: &egui::Context) {
        // The dungeon shown (or selected around the cell), whose textures it is drawn with.
        let shown_dungeon = match &self.workspace {
            Workspace::Dungeon(_) => self.opened_item.clone(),
            Workspace::Room(editor) => editor.context_dungeon().cloned(),
            _ => None,
        };
        let person_place = match &shown_dungeon {
            Some(dungeon) => self.dungeon_person_place(dungeon),
            None => Err(room_editor::NO_DUNGEON_FOR_NAMES),
        };
        let identity = shown_dungeon.and_then(|dungeon| self.dungeon_identity(&dungeon));
        let mut main_used_in = self.main_used_in();
        let mut open_request = None;
        let mut context_request = None;
        let mut cell_open_request = None;
        let Some(library) = self.library.as_mut() else {
            show_centered_hint(context, "Choose your ARENA2 folder to begin.");
            return;
        };
        let (world, daggerfall_dungeons) = match &self.workspace {
            Workspace::Dungeon(_) => (library.world_map(), Some(library.dungeons())),
            _ => (None, None),
        };
        let mut resources = EditorResources {
            library,
            render_state: self.render_state.as_ref(),
            renderer: self.renderer.as_mut(),
            catalog: &self.catalog,
            custom_names: &mut self.custom_names,
            prefabs: &mut self.prefabs,
            custom_models: &self.custom_models,
            texture_setting: &mut self.texture_setting,
            game_texture_mode: self.game_texture_mode,
            unity: self.unity_text.as_deref(),
        };

        let mut room_to_open = None;
        let mut properties_action = None;
        let mut added_room = None;
        let mut deleted_room = None;
        let mut moved_room = None;
        let mut merged_rooms: Option<Vec<usize>> = None;
        let mut exterior_action = None;
        let mut interior_action = None;
        let mut custom_model_request = None;
        match &mut self.workspace {
            Workspace::Room(editor) => {
                editor.set_used_in(std::mem::take(&mut main_used_in));
                editor.set_dungeon_identity(identity);
                editor.set_person_place(person_place);
                editor.show(context, &mut resources);
                open_request = editor.take_open_request();
                context_request =
                    editor.take_context_request().map(|dungeon| {
                        let home = dungeon.as_ref().and_then(|dungeon| editor.home_layout_index(dungeon));
                        (dungeon, home)
                    });
                cell_open_request = editor.take_cell_open_request();
                custom_model_request = editor.take_custom_model_request();
                if editor.take_blocked_edit() && self.copy_prompt.is_none() {
                    self.copy_prompt = Some(CopyPrompt { subject: CopySubject::Room, viewport: None });
                }
            }
            Workspace::Dungeon(view) => {
                view.set_identity(identity);
                let open_dungeon = match self.opened_item {
                    Some(OpenedItem::Project(id)) => self.project.dungeon(id),
                    _ => None,
                };
                let daggerfall_dungeons = daggerfall_dungeons.as_deref().unwrap_or_default();
                let location = match (open_dungeon, &self.opened_item) {
                    (Some(dungeon), _) => dungeon.location.clone(),
                    (None, Some(OpenedItem::DaggerfallDungeon(index))) => daggerfall_dungeons
                        .get(*index)
                        .map(|definition| DungeonLocation {
                            map_pixel: definition.map_pixel,
                            ..DungeonLocation::copied_from(definition)
                        })
                        .unwrap_or_default(),
                    _ => DungeonLocation::default(),
                };
                let other_dungeons = self.project.placed_dungeons(open_dungeon.map(|dungeon| dungeon.id));
                // The song the game plays there unless another is chosen: a Daggerfall
                // dungeon's own, or that of the dungeon a project dungeon replaces.
                let game_definition = match (open_dungeon, &self.opened_item) {
                    (Some(dungeon), _) => dungeon.location.replaces.as_ref().and_then(|replaced| {
                        daggerfall_dungeons.iter().find(|definition| replaced.is(definition))
                    }),
                    (None, Some(OpenedItem::DaggerfallDungeon(index))) => daggerfall_dungeons.get(*index),
                    _ => None,
                };
                let game_song = game_definition.and_then(|definition| {
                    let classic = definition.classic?;
                    Some(crate::music::song_for_seed(classic.dungeon_music_seed, definition.region_index))
                });
                // What the game knows the location by: Daggerfall's (or the replaced
                // dungeon's) own ids, or those an export gives a new dungeon.
                let ids = match game_definition.and_then(|definition| definition.classic) {
                    Some(classic) => Some(DungeonIds {
                        map_id: classic.map_id,
                        location_id: classic.location_id,
                        dungeon_location_id: classic.dungeon_location_id,
                        index_in_region: Some(classic.location_index),
                    }),
                    None => identity.map(|identity| DungeonIds {
                        map_id: identity.map_id,
                        location_id: identity.dungeon_location_id.wrapping_sub(1),
                        dungeon_location_id: identity.dungeon_location_id,
                        index_in_region: None,
                    }),
                };
                let details = DungeonDetails {
                    read_only: open_dungeon.is_none(),
                    based_on: open_dungeon.and_then(|dungeon| dungeon.based_on.as_deref()),
                    project: &self.project,
                    custom_names: resources.custom_names,
                    location: location.clone(),
                    world: world.as_deref(),
                    other_dungeons: &other_dungeons,
                    daggerfall_dungeons,
                    game_song,
                    ids,
                };
                properties_action = view.show_properties(context, &details);
                let windows_request = LocationWindowsRequest {
                    read_only: open_dungeon.is_none(),
                    location: &location,
                    world: world.as_deref(),
                    other_dungeons: &other_dungeons,
                    daggerfall_dungeons,
                };
                if let Some(action) = view.show_location_windows(context, &windows_request, &mut resources) {
                    properties_action = Some(action);
                }
                let project = &self.project;
                // The skybox it shows in Daggerfall Unity, drawn behind it here too.
                let skybox = (location.skybox && !location.skybox_file.trim().is_empty())
                    .then(|| PathBuf::from(location.skybox_file.trim()));
                egui::CentralPanel::default().frame(egui::Frame::none()).show(context, |ui| {
                    room_to_open = view.show(ui, &mut resources, project, skybox.as_deref());
                });
                if let Some(rooms) = view.take_merge_request() {
                    merged_rooms = Some(rooms);
                }
                if let Some(room_index) = view.take_make_start_request() {
                    properties_action = Some(DungeonPropertiesAction::MakeStart(room_index));
                }
                if let Some(placement) = view.take_add_room_request() {
                    added_room = Some(placement);
                }
                if let Some(room_index) = view.take_delete_room_request() {
                    deleted_room = Some(room_index);
                }
                if let Some(request) = view.take_move_room_request() {
                    moved_room = Some(request);
                }
            }
            Workspace::Interior(view) => {
                let locations = resources.library.locations();
                let references = resources.library.references();
                egui::SidePanel::right("interior_properties").resizable(true).default_width(300.0).show(
                    context,
                    |ui| {
                        ui.heading("Properties");
                        ui.separator();
                        interior_action = view.show_properties(ui, &locations, &references);
                    },
                );
                egui::CentralPanel::default()
                    .frame(egui::Frame::none())
                    .show(context, |ui| view.show_viewport(ui, &mut resources));
            }
            Workspace::Exterior(view) => {
                let locations = resources.library.locations();
                let (dungeon, used_in) = match &view.subject {
                    ExteriorSubject::Settlement(index) => {
                        let dungeon = locations.get(*index).and_then(|location| {
                            self.dungeon_under.get(&(location.region_index, location.location_id)).copied()
                        });
                        (dungeon, Vec::new())
                    }
                    ExteriorSubject::Cell(name) => (None, self.outdoor_usage.get(name).cloned().unwrap_or_default()),
                };
                let dungeons = resources.library.dungeons();
                let references = resources.library.references();
                // The dungeons under the locations built with the cell shown.
                let cell_dungeons: Vec<(usize, &str)> = used_in
                    .iter()
                    .filter_map(|&location| {
                        let location = locations.get(location)?;
                        let index = *self.dungeon_under.get(&(location.region_index, location.location_id))?;
                        Some((index, dungeons.get(index)?.name.as_str()))
                    })
                    .collect();
                let details = ExteriorDetails {
                    locations: &locations,
                    dungeon: dungeon
                        .and_then(|index| Some((index, dungeons.get(index)?.name.as_str()))),
                    used_in: &used_in,
                    cell_dungeons: &cell_dungeons,
                    references: &references,
                };
                exterior_action =
                    view.show_properties(context, &details, resources.custom_names, resources.library);
                egui::CentralPanel::default().frame(egui::Frame::none()).show(context, |ui| {
                    if let Some(action) = view.show(ui, &mut resources) {
                        exterior_action = Some(action);
                    }
                });
            }
            Workspace::Empty => show_centered_hint(
                context,
                "Click a dungeon or double-click a cell in the project panel, or use File > Open cell… / Open dungeon…",
            ),
        }
        if let Some(request) = custom_model_request {
            self.handle_custom_model_request(request);
        }
        match exterior_action {
            Some(ExteriorOpen::Cell(name)) => self.open_outdoor_cell(&name),
            Some(ExteriorOpen::Settlement(index)) => self.open_settlement(index),
            Some(ExteriorOpen::Dungeon(index)) => self.open_dungeon(index),
            Some(ExteriorOpen::DungeonUnder(location_index)) => {
                let locations = self.library.as_mut().map(|library| library.locations());
                let dungeon = locations.as_ref().and_then(|locations| {
                    let location = locations.get(location_index)?;
                    self.dungeon_under.get(&(location.region_index, location.location_id)).copied()
                });
                match dungeon {
                    Some(index) => self.open_dungeon(index),
                    None => {
                        self.status_message = "This location's dungeon is not in the dungeon list".to_string()
                    }
                }
            }
            Some(ExteriorOpen::Interior { key, place, climate }) => {
                self.open_interior_window(key, place, climate)
            }
            None => {}
        }
        match interior_action {
            Some(InteriorOpen::Settlement(index)) => self.open_settlement(index),
            Some(InteriorOpen::Cell(name)) => self.open_outdoor_cell(&name),
            None => {}
        }
        if let Some(room_index) = room_to_open {
            self.open_room_window(context, room_index);
        }
        if let Some(action) = properties_action {
            self.handle_dungeon_properties_action(context, action);
        }
        if let Some(placement) = added_room {
            self.add_room_to_open_dungeon(placement);
        }
        if let Some(room_index) = deleted_room {
            self.delete_room_from_open_dungeon(room_index);
        }
        if let Some((room_index, from, to)) = moved_room {
            self.move_room_in_open_dungeon(room_index, from, to);
        }
        if let Some(rooms) = merged_rooms {
            self.merge_rooms_in_open_dungeon(&rooms);
        }
        // Load the surrounding cells of a dungeon newly selected under "Used in".
        if let Some((dungeon, home)) = context_request {
            let context = match (dungeon, self.main_cell()) {
                (Some(dungeon), Some((block_name, project_room))) => {
                    self.dungeon_context(&dungeon, &block_name, project_room, home)
                }
                _ => None,
            };
            if let Workspace::Room(editor) = &mut self.workspace {
                editor.set_dungeon_context(context);
            }
        }
        if let Some((dungeon, layout_index)) = cell_open_request {
            self.open_context_cell(context, dungeon, layout_index, false);
        }
        if let Some(item) = open_request {
            self.open_item(&item);
        }
    }

    /// Carries out what the user asked for in the dungeon properties panel.
    fn handle_dungeon_properties_action(&mut self, context: &egui::Context, action: DungeonPropertiesAction) {
        let open_project_dungeon = match self.opened_item {
            Some(OpenedItem::Project(id)) => self.project.dungeon_mut(id),
            _ => None,
        };
        match (action, open_project_dungeon) {
            (DungeonPropertiesAction::Rename(name), Some(dungeon)) => dungeon.name = name,
            (DungeonPropertiesAction::SetType(dungeon_type), Some(dungeon)) => {
                dungeon.dungeon_type = Some(dungeon_type);
                self.status_message = format!("{} is now a {}", dungeon.name, dungeon_type.name());
            }
            (DungeonPropertiesAction::OpenRoom(room_index), _) => self.open_room_window(context, room_index),
            (DungeonPropertiesAction::PlaySong(song), _) => {
                let data = match self.library.as_ref().map(|library| library.midi_song(song.file)) {
                    Some(Ok(data)) => data,
                    Some(Err(error)) => {
                        self.error_message = Some(format!("Could not play {}: {error}", song.label()));
                        return;
                    }
                    None => return,
                };
                if let Workspace::Dungeon(view) = &mut self.workspace {
                    if let Err(error) = view.play_song(song, &data) {
                        self.error_message = Some(error);
                    }
                }
                // Show Stop straight away.
                context.request_repaint();
            }
            (DungeonPropertiesAction::MakeStart(room_index), Some(_)) => self.make_start_room(room_index),
            (DungeonPropertiesAction::SetLocation(location), Some(dungeon)) => {
                // The dungeon belongs to the region of its spot on the map, or to
                // that of the dungeon it replaces.
                let region = match &location.replaces {
                    Some(replaced) => Some(replaced.region_index),
                    None => location
                        .map_pixel
                        .zip(self.library.as_mut().and_then(|library| library.world_map()))
                        .and_then(|(pixel, world)| world.region_at(pixel)),
                };
                if location.map_pixel != dungeon.location.map_pixel
                    || location.replaces != dungeon.location.replaces
                {
                    match (&location.replaces, location.map_pixel) {
                        (Some(replaced), _) => {
                            self.status_message =
                                format!("{} now replaces {}", dungeon.name, replaced.label());
                        }
                        (None, Some(pixel)) => {
                            self.status_message = format!("{} is now at {pixel}", dungeon.name)
                        }
                        (None, None) => {}
                    }
                }
                if let Some(region_index) = region {
                    dungeon.region_name = crate::arena2::region_name(region_index).to_string();
                    if let Workspace::Dungeon(view) = &mut self.workspace {
                        view.region_name = dungeon.region_name.clone();
                    }
                }
                dungeon.location = location;
            }
            (DungeonPropertiesAction::MakeStart(_) | DungeonPropertiesAction::SetLocation(_), None)
                if self.copy_prompt.is_none() =>
            {
                self.copy_prompt = Some(CopyPrompt { subject: CopySubject::Dungeon, viewport: None });
            }
            (DungeonPropertiesAction::EditBlocked, _) if self.copy_prompt.is_none() => {
                self.copy_prompt = Some(CopyPrompt { subject: CopySubject::Dungeon, viewport: None });
            }
            _ => {}
        }
    }

    /// Adds the cell `placement` (dropped into the dungeon view) to the open
    /// dungeon. A Daggerfall dungeon cannot be changed, so the user is asked to
    /// copy it first.
    fn add_room_to_open_dungeon(&mut self, placement: ProjectDungeonRoom) {
        let Some(OpenedItem::Project(id)) = self.opened_item else {
            if self.copy_prompt.is_none() {
                self.copy_prompt = Some(CopyPrompt { subject: CopySubject::Dungeon, viewport: None });
            }
            return;
        };
        let Some(dungeon) = self.project.dungeon_mut(id) else { return };
        dungeon.rooms.push(placement.clone());
        let layout_index = dungeon.rooms.len() - 1;
        let (Workspace::Dungeon(view), Some(library)) = (&mut self.workspace, self.library.as_mut()) else {
            return;
        };
        match view.add_room(&placement, layout_index, &self.project, library) {
            Ok(()) => {
                self.status_message =
                    format!("Added {} at {}, {}", placement.block_name, placement.grid_x, placement.grid_z);
            }
            Err(error) => {
                self.error_message = Some(format!("Could not add {}: {error}", placement.block_name))
            }
        }
    }

    /// Moves room `room_index` (dragged in the dungeon view from grid spot
    /// `from`) to grid spot `spot` in the open dungeon. A Daggerfall dungeon
    /// cannot be changed, so the cell goes back and the user is asked to copy
    /// the dungeon first.
    fn move_room_in_open_dungeon(&mut self, room_index: usize, from: (i32, i32), spot: (i32, i32)) {
        let project_dungeon = match self.opened_item {
            Some(OpenedItem::Project(id)) => Some(id),
            _ => None,
        };
        let Workspace::Dungeon(view) = &mut self.workspace else { return };
        let Some(layout_index) = view.layout_index(room_index) else { return };
        let placement = project_dungeon
            .and_then(|id| self.project.dungeon_mut(id))
            .and_then(|dungeon| dungeon.rooms.get_mut(layout_index));
        let Some(placement) = placement else {
            // Not a project dungeon: put the cell back where it was.
            view.set_room_spot(room_index, from);
            if self.copy_prompt.is_none() {
                self.copy_prompt = Some(CopyPrompt { subject: CopySubject::Dungeon, viewport: None });
            }
            return;
        };
        placement.grid_x = spot.0;
        placement.grid_z = spot.1;
        self.status_message = format!("Moved {} to {}, {}", placement.block_name, spot.0, spot.1);
    }

    /// Merges the rooms `room_indices` of the open dungeon (chosen in the
    /// dungeon view) into one new project cell, which takes their place in the
    /// dungeon (see [`crate::merge`]). The cells merged are not changed;
    /// project cells among them stay in the project. A Daggerfall dungeon
    /// cannot be changed, so the user is asked to copy it first.
    fn merge_rooms_in_open_dungeon(&mut self, room_indices: &[usize]) {
        let Some(OpenedItem::Project(dungeon_id)) = self.opened_item else {
            if self.copy_prompt.is_none() {
                self.copy_prompt = Some(CopyPrompt { subject: CopySubject::Dungeon, viewport: None });
            }
            return;
        };
        // Edits to the dungeon's cells are kept before they are merged.
        self.store_workspace_changes();
        let Workspace::Dungeon(view) = &self.workspace else { return };
        let mut picked = Vec::new();
        for &room_index in room_indices {
            let (Some(room), Some(layout_index)) = (view.room(room_index), view.layout_index(room_index))
            else {
                continue;
            };
            let spot = (
                (room.origin.x / crate::arena2::RDB_BLOCK_SIDE).round() as i32,
                (room.origin.z / crate::arena2::RDB_BLOCK_SIDE).round() as i32,
            );
            picked.push((
                layout_index,
                room.scene.objects.clone(),
                spot,
                room.is_starting_block,
                room.category,
            ));
        }
        let sources: Vec<crate::merge::MergeSource<'_>> = picked
            .iter()
            .map(|(_, objects, spot, is_start, _)| crate::merge::MergeSource {
                objects,
                spot: *spot,
                is_start: *is_start,
            })
            .collect();
        let merged = match crate::merge::merge_cells(&sources) {
            Ok(merged) => merged,
            Err(error) => {
                self.status_message = format!("Could not merge the cells: {error}");
                return;
            }
        };
        let anchor_layout_index = picked[merged.anchor].0;
        let mut removed: Vec<usize> = picked
            .iter()
            .map(|(layout_index, ..)| *layout_index)
            .filter(|&index| index != anchor_layout_index)
            .collect();
        removed.sort_unstable();

        // The new cell, named like the dungeon.
        let dungeon_name =
            self.project.dungeon(dungeon_id).map(|dungeon| dungeon.name.clone()).unwrap_or_default();
        let base_name = format!("{dungeon_name} merged cell");
        let name = std::iter::once(base_name.clone())
            .chain((2..).map(|number| format!("{base_name} {number}")))
            .find(|candidate| !self.project.has_item_named(candidate))
            .expect("an unused name always exists");
        let room_id = self.project.new_id();
        let based_on = None;
        self.project.add(
            ProjectEntry::Room(ProjectRoom {
                id: room_id,
                name: name.clone(),
                based_on,
                // The cell whose place it takes decides its kind.
                category: picked[merged.anchor].4,
                objects: merged.objects,
            }),
            None,
        );
        let Some(dungeon) = self.project.dungeon_mut(dungeon_id) else { return };
        if let Some(placement) = dungeon.rooms.get_mut(anchor_layout_index) {
            placement.room = Some(room_id);
            placement.grid_x = merged.spot.0;
            placement.grid_z = merged.spot.1;
            placement.is_starting_block = merged.is_start;
        }
        for &index in removed.iter().rev() {
            if index < dungeon.rooms.len() {
                dungeon.rooms.remove(index);
            }
        }

        // The cells' windows close, and the view shows the dungeon again from the project.
        self.close_room_windows();
        self.reload_open_dungeon();
        let mut message = format!("Merged {} cells into {name}", picked.len());
        for warning in merged.warnings {
            message.push_str(&format!("; {warning}"));
        }
        self.status_message = message;
        self.show_current_names();
    }

    /// Loads the open project dungeon into the view again from the project,
    /// keeping the camera and view options, after its layout changed.
    fn reload_open_dungeon(&mut self) {
        let Some(OpenedItem::Project(id)) = self.opened_item else { return };
        let (Some(dungeon), Some(library)) = (self.project.dungeon(id), self.library.as_mut()) else {
            return;
        };
        let (mut view, problems) = DungeonView::load(
            &dungeon.name,
            &dungeon.region_name,
            dungeon.dungeon_type,
            &dungeon.rooms,
            &self.project,
            library,
        );
        if let Workspace::Dungeon(old) = &mut self.workspace {
            view.keep_view_of(old);
            if let Some(render_state) = self.render_state.as_ref() {
                old.release(render_state);
            }
        }
        self.workspace = Workspace::Dungeon(Box::new(view));
        if !problems.is_empty() {
            self.error_message = Some(format!("Some cells could not be loaded:\n{}", problems.join("\n")));
        }
    }

    /// Takes room `room_index` (pressed Delete on in the dungeon view) out of
    /// the open dungeon. A project cell it used stays in the project. A
    /// Daggerfall dungeon cannot be changed, so the user is asked to copy it first.
    fn delete_room_from_open_dungeon(&mut self, room_index: usize) {
        let Some(OpenedItem::Project(id)) = self.opened_item else {
            if self.copy_prompt.is_none() {
                self.copy_prompt = Some(CopyPrompt { subject: CopySubject::Dungeon, viewport: None });
            }
            return;
        };
        // Edits to the dungeon's cells are kept before the layout changes under them.
        self.store_workspace_changes();
        let Workspace::Dungeon(view) = &mut self.workspace else { return };
        let Some(layout_index) = view.layout_index(room_index) else { return };
        let Some(dungeon) = self.project.dungeon_mut(id) else { return };
        if layout_index >= dungeon.rooms.len() {
            return;
        }
        let placement = dungeon.rooms.remove(layout_index);
        view.remove_room(room_index);

        // The deleted cell's window closes; the windows of later cells follow them up one place.
        if let Some(render_state) = self.render_state.as_ref() {
            for window in
                self.room_windows.iter_mut().filter(|window| window.dungeon_room_index == Some(room_index))
            {
                window.editor.release(render_state);
            }
        }
        self.room_windows.retain(|window| window.dungeon_room_index != Some(room_index));
        for window in &mut self.room_windows {
            if let Some(index) = window.dungeon_room_index.as_mut().filter(|index| **index > room_index) {
                *index -= 1;
            }
        }

        let name = match placement.room.and_then(|room_id| self.project.room(room_id)) {
            Some(room) => format!("{} (it stays in the project)", room.name),
            None => placement.block_name.clone(),
        };
        let start = if placement.is_starting_block { "; the dungeon has no start cell now" } else { "" };
        self.status_message = format!("Removed {name} from the dungeon{start}");
    }

    /// Makes room `room_index` of the open project dungeon its starting cell,
    /// in the project and in the view; whichever cell was the start stops being it.
    fn make_start_room(&mut self, room_index: usize) {
        let Some(OpenedItem::Project(id)) = self.opened_item else { return };
        let Workspace::Dungeon(view) = &mut self.workspace else { return };
        let Some(layout_index) = view.layout_index(room_index) else { return };
        if let Some(problems) = view.room(room_index).map(|room| room.start_problems()) {
            if !problems.is_empty() {
                self.status_message = format!("{}, so it cannot be the start", problems.join(" and "));
                return;
            }
        }
        let Some(dungeon) = self.project.dungeon_mut(id) else { return };
        for (index, placement) in dungeon.rooms.iter_mut().enumerate() {
            placement.is_starting_block = index == layout_index;
        }
        let block_name = dungeon.rooms.get(layout_index).map(|placement| placement.block_name.clone());
        // Cell windows hold their cell's objects and copy them back every frame,
        // which would undo the change of exit doors, so they are closed.
        self.close_room_windows();
        let (Workspace::Dungeon(view), Some(library)) = (&mut self.workspace, self.library.as_mut()) else {
            return;
        };
        view.set_start_room(room_index, library);
        self.status_message = format!("{} is now the start cell", block_name.unwrap_or_default());
    }

    /// Draws every room editor window and syncs their edits back into the dungeon view.
    fn show_room_windows(&mut self, context: &egui::Context) {
        let window_used_in = self.room_window_used_in();
        // A room window's cell has unsaved changes if its project cell differs from the saved one.
        let window_unsaved: Vec<bool> = self
            .room_windows
            .iter()
            .map(|window| {
                let Workspace::Dungeon(view) = &self.workspace else { return false };
                window
                    .dungeon_room_index
                    .and_then(|index| view.room(index))
                    .and_then(|room| room.project_room)
                    .is_some_and(|room_id| self.item_has_unsaved_changes(room_id))
            })
            .collect();
        let mut save_requested = false;
        let mut open_request = None;
        let mut context_requests = Vec::new();
        let mut cell_open_request = None;
        // The dungeon selected around each window's cell, whose textures it is drawn with.
        let shown_dungeons: Vec<Option<OpenedItem>> =
            self.room_windows.iter().map(|window| window.editor.context_dungeon().cloned()).collect();
        let places: Vec<_> = shown_dungeons
            .iter()
            .map(|dungeon| match dungeon {
                Some(dungeon) => self.dungeon_person_place(dungeon),
                None => Err(room_editor::NO_DUNGEON_FOR_NAMES),
            })
            .collect();
        let identities: Vec<Option<DungeonIdentity>> = shown_dungeons
            .into_iter()
            .map(|dungeon| dungeon.and_then(|dungeon| self.dungeon_identity(&dungeon)))
            .collect();
        for ((window, identity), place) in self.room_windows.iter_mut().zip(identities).zip(places) {
            window.editor.set_dungeon_identity(identity);
            window.editor.set_person_place(place);
        }
        let Some(library) = self.library.as_mut() else { return };
        let mut resources = EditorResources {
            library,
            render_state: self.render_state.as_ref(),
            renderer: self.renderer.as_mut(),
            catalog: &self.catalog,
            custom_names: &mut self.custom_names,
            prefabs: &mut self.prefabs,
            custom_models: &self.custom_models,
            texture_setting: &mut self.texture_setting,
            game_texture_mode: self.game_texture_mode,
            unity: self.unity_text.as_deref(),
        };

        let prompt = self.copy_prompt;
        let mut copy_choice = None;
        let mut custom_model_request = None;
        for ((window, used_in), unsaved) in
            self.room_windows.iter_mut().zip(window_used_in).zip(window_unsaved)
        {
            window.editor.set_used_in(used_in);
            // egui compares the builder with the previous frame's, so a renamed room updates the title.
            let builder = egui::ViewportBuilder::default()
                .with_title(editor_window_title(&window.editor, resources.custom_names, unsaved))
                .with_inner_size(ROOM_WINDOW_SIZE);
            context.show_viewport_immediate(window.viewport_id, builder, |window_context, _class| {
                // Ctrl+S saves the dungeon the window's cell belongs to, with its cells.
                let save = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
                save_requested |= window_context.input_mut(|input| input.consume_shortcut(&save));
                show_room_window_contents(window_context, window, &mut resources);
                if let Some(prompt) = prompt.filter(|prompt| prompt.viewport == Some(window.viewport_id)) {
                    copy_choice = show_copy_prompt(window_context, prompt.subject).or(copy_choice);
                }
            });

            if let Some(item) = window.editor.take_open_request() {
                open_request = Some(item);
            }
            if let Some(dungeon) = window.editor.take_context_request() {
                context_requests.push((window.viewport_id, dungeon));
            }
            if let Some(request) = window.editor.take_cell_open_request() {
                cell_open_request = Some(request);
            }
            if let Some(request) = window.editor.take_custom_model_request() {
                custom_model_request = Some(request);
            }
            if window.editor.take_blocked_edit() && self.copy_prompt.is_none() {
                let subject = match self.opened_item {
                    Some(OpenedItem::DaggerfallDungeon(_)) => CopySubject::DungeonAndRoom,
                    _ => CopySubject::RoomInDungeon,
                };
                self.copy_prompt = Some(CopyPrompt { subject, viewport: Some(window.viewport_id) });
            }
            if let (Some(room_index), Workspace::Dungeon(view)) =
                (window.dungeon_room_index, &mut self.workspace)
            {
                view.update_room_objects(room_index, &window.editor.scene.objects);
                let project_room = view.room(room_index).and_then(|room| room.project_room);
                if let (Some(category), Some(room_id)) = (window.editor.cell_category(), project_room) {
                    view.set_room_category(room_index, category);
                    if let Some(room) = self.project.room_mut(room_id) {
                        room.category = category;
                    }
                }
            }
        }

        if let Some(request) = custom_model_request {
            self.handle_custom_model_request(request);
        }

        // The prompt belongs to a window that has since closed.
        if let Some(CopyPrompt { viewport: Some(viewport), .. }) = self.copy_prompt {
            if !self.room_windows.iter().any(|window| window.viewport_id == viewport && window.is_open) {
                self.copy_prompt = None;
            }
        }

        // Release closed windows.
        let render_state = self.render_state.as_ref();
        self.room_windows.retain_mut(|window| {
            if !window.is_open {
                if let Some(render_state) = render_state {
                    window.editor.release(render_state);
                }
            }
            window.is_open
        });

        // Load the surrounding cells of dungeons newly selected under "Used in".
        for (viewport_id, dungeon) in context_requests {
            let Some(window_index) =
                self.room_windows.iter().position(|window| window.viewport_id == viewport_id)
            else {
                continue;
            };
            let room_index = self.room_windows[window_index].dungeon_room_index;
            let cell = match &self.workspace {
                Workspace::Dungeon(view) => room_index.and_then(|index| {
                    let room = view.room(index)?;
                    Some((room.block_name.clone(), room.project_room, view.layout_index(index)))
                }),
                _ => None,
            };
            let context = match (dungeon, cell) {
                (Some(dungeon), Some((block_name, project_room, layout_index))) => {
                    // The window's own place counts only in the dungeon it was opened from.
                    let layout_index = layout_index.filter(|_| self.opened_item.as_ref() == Some(&dungeon));
                    self.dungeon_context(&dungeon, &block_name, project_room, layout_index)
                }
                _ => None,
            };
            self.room_windows[window_index].editor.set_dungeon_context(context);
        }

        if let Some(choice) = copy_choice {
            self.answer_copy_prompt(choice);
        }
        if save_requested {
            self.save_open_item();
        }
        if let Some((dungeon, layout_index)) = cell_open_request {
            self.open_context_cell(context, dungeon, layout_index, true);
        }
        if let Some(item) = open_request {
            self.open_item(&item);
        }
    }

    /// Acts on the answer to the copy prompt.
    fn answer_copy_prompt(&mut self, choice: CopyChoice) {
        let Some(prompt) = self.copy_prompt.take() else { return };
        if choice == CopyChoice::Cancel {
            return;
        }
        match prompt.viewport {
            Some(viewport) => self.copy_room_in_open_dungeon(viewport),
            None => self.convert_open_item_to_copy(),
        }
    }

    /// Draws the copy prompt in the main window, if it belongs there.
    fn show_main_copy_prompt(&mut self, context: &egui::Context) {
        let Some(prompt) = self.copy_prompt.filter(|prompt| prompt.viewport.is_none()) else { return };
        if let Some(choice) = show_copy_prompt(context, prompt.subject) {
            self.answer_copy_prompt(choice);
        }
    }

    /// Asks the user to confirm deleting a project room or dungeon.
    fn show_delete_prompt(&mut self, context: &egui::Context) {
        let Some(id) = self.delete_prompt else { return };
        let Some(name) = self.project.find(id).map(|entry| entry.name().to_string()) else {
            self.delete_prompt = None;
            return;
        };
        let users = self.project.dungeons_using(id).join(", ");
        let mut answer = None;
        modal_window(context, "Delete", |ui| {
            ui.label(format!("Delete \"{name}\" from the project?"));
            if !users.is_empty() {
                ui.label(format!("{users} will use the Daggerfall cell again."));
            }
            ui.label(egui::RichText::new("This cannot be undone once the project is saved.").weak());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Delete").clicked() {
                    answer = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
        if let Some(confirmed) = answer {
            self.delete_prompt = None;
            if confirmed {
                self.delete_project_item(id);
            }
        }
    }

    /// Asks whether to save unsaved changes before discarding them.
    fn show_unsaved_changes_prompt(&mut self, context: &egui::Context) {
        let Some(action) = self.unsaved_changes_prompt else { return };
        let title = self.project_title();
        let mut answer = None;
        modal_window(context, "Unsaved changes", |ui| {
            ui.label(format!("Save changes to \"{title}\"?"));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    answer = Some(Some(true));
                }
                if ui.button("Don't save").clicked() {
                    answer = Some(Some(false));
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(None);
                }
            });
        });
        let Some(answer) = answer else { return };
        self.unsaved_changes_prompt = None;
        match answer {
            Some(true) => {
                if self.save_project() {
                    self.perform(action);
                }
            }
            Some(false) => self.perform(action),
            None => {}
        }
        if self.exit_confirmed {
            context.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Stops the program closing while the project has unsaved changes, and asks first.
    fn handle_close_request(&mut self, context: &egui::Context) {
        if !context.input(|input| input.viewport().close_requested()) || self.exit_confirmed {
            return;
        }
        if self.has_unsaved_changes() {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.unsaved_changes_prompt = Some(DiscardingAction::Exit);
        }
    }

    /// Draws the export window, and checks or exports when it asks.
    fn show_export_window(&mut self, context: &egui::Context) {
        let title = self.project_title();
        let request = match self.export_window.needs_check() {
            true => Some(ExportRequest::Check),
            false => self.export_window.show(context, &mut self.project, &title),
        };
        let Some(request) = request else { return };
        // Edits in the open cell or dungeon count.
        self.store_workspace_changes();
        let Some(library) = self.library.as_mut() else { return };
        match request {
            ExportRequest::Check => self.export_window.run_checks(&self.project, library),
            ExportRequest::Export => self.export_window.export(&mut self.project, library, &title),
        }
    }

    /// Draws the error popup, if there is an error to report.
    fn show_error_window(&mut self, context: &egui::Context) {
        let Some(message) = self.error_message.clone() else { return };
        egui::Window::new("Error")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.label(message);
                if ui.button("OK").clicked() {
                    self.error_message = None;
                }
            });
    }

    /// Updates the main window title to show what is open.
    fn update_window_title(&mut self, context: &egui::Context) {
        // A * marks unsaved changes: the open cell's or dungeon's, and the project's.
        self.store_workspace_changes();
        let item_mark = match self.opened_project_item() {
            Some(id) if self.item_has_unsaved_changes(id) => "*",
            _ => "",
        };
        let item = match &self.workspace {
            Workspace::Room(editor) => {
                Some(format!("{}{item_mark}", editor_title(editor, &self.custom_names)))
            }
            Workspace::Dungeon(view) => Some(format!("{}{item_mark}", view.description())),
            Workspace::Exterior(view) => {
                let locations =
                    self.library.as_ref().map(library_locations).unwrap_or_else(|| std::sync::Arc::new([]));
                Some(view.description(&locations))
            }
            Workspace::Interior(view) => Some(format!("Inside {}", view.description())),
            Workspace::Empty => None,
        };
        let project_mark = if self.has_unsaved_changes() { "*" } else { "" };
        let project = format!("{}{project_mark}", self.project_title());
        let title = match item {
            Some(item) => format!("{item} - {project} - {APPLICATION_NAME}"),
            None => format!("{project} - {APPLICATION_NAME}"),
        };
        if title != self.window_title {
            context.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }
}

impl eframe::App for DungeonMakerApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        // Images dropped last frame are freed now that no frame draws them.
        kept_texture::free_put_aside();
        self.handle_close_request(context);
        self.poll_openings_job(context);
        self.poll_blend_conversion(context);
        self.handle_shortcuts(context);
        self.show_current_names();
        self.show_menu_bar(context);
        self.show_status_bar(context);
        self.show_project_panel(context);
        self.show_workspace(context);

        if let Some(folder) = self.arena2_prompt.show(context, self.library.is_some()) {
            self.load_library(&folder);
        }
        if let Some(room_name) = self.room_browser.show(context, &mut self.custom_names) {
            self.open_room(&room_name);
        }
        if self.dungeon_browser.is_open {
            if let Some(library) = self.library.as_mut() {
                let dungeons = library.dungeons();
                if let Some(dungeon_index) = self.dungeon_browser.show(context, &dungeons) {
                    self.open_dungeon(dungeon_index);
                }
            }
        }
        self.show_export_window(context);
        show_controls_window(context, &mut self.show_controls_window);
        self.show_room_windows(context);
        self.show_interior_window(context);
        self.show_main_copy_prompt(context);
        self.show_delete_prompt(context);
        self.show_unsaved_changes_prompt(context);
        self.show_test_prompts(context);
        self.show_error_window(context);
        self.update_window_title(context);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Some(library) = &self.library {
            storage.set_string(ARENA2_FOLDER_STORAGE_KEY, library.root().display().to_string());
        }
        let project_file =
            self.project_path.as_deref().map(|path| path.display().to_string()).unwrap_or_default();
        storage.set_string(PROJECT_FILE_STORAGE_KEY, project_file);
        eframe::set_value(storage, CUSTOM_NAMES_STORAGE_KEY, &self.custom_names);
        storage.set_string(PREFABS_STORAGE_KEY, self.prefabs.to_text());
        eframe::set_value(storage, PANEL_VIEW_STORAGE_KEY, &self.project_panel.view);
        eframe::set_value(storage, EXPORT_SETTINGS_STORAGE_KEY, &self.export_window.settings);
        eframe::set_value(storage, DUNGEON_TEXTURES_STORAGE_KEY, &self.texture_setting);
        let blender =
            self.blender_program.as_ref().map(|path| path.display().to_string()).unwrap_or_default();
        storage.set_string(BLENDER_STORAGE_KEY, blender);
        let unity = self.unity_folder.as_ref().map(|path| path.display().to_string()).unwrap_or_default();
        storage.set_string(UNITY_FOLDER_STORAGE_KEY, unity);
        if let Some(openings) = &self.stored_openings {
            eframe::set_value(storage, OPENINGS_STORAGE_KEY, openings);
        }
    }
}

/// Short title of the room in an editor: the project room's name, or the
/// Daggerfall room with the user's name for it.
fn editor_title(editor: &RoomEditor, custom_names: &CustomNames) -> String {
    match custom_names.room_name(&editor.scene.block_name).filter(|_| !editor.is_project_room()) {
        Some(room_name) => format!("{room_name} - {}", editor.title()),
        None => editor.title().to_string(),
    }
}

/// Window title of a room editor window.
fn editor_window_title(editor: &RoomEditor, custom_names: &CustomNames, unsaved: bool) -> String {
    let mark = if unsaved { "*" } else { "" };
    format!("{}{mark} - {APPLICATION_NAME}", editor_title(editor, custom_names))
}

/// Draws the "You cannot edit … from Daggerfall" question. Returns the user's answer once given.
fn show_copy_prompt(context: &egui::Context, subject: CopySubject) -> Option<CopyChoice> {
    let (message, explanation) = match subject {
        CopySubject::Room => ("You cannot edit cells from Daggerfall", None),
        CopySubject::Dungeon => ("You cannot edit dungeons from Daggerfall", None),
        CopySubject::DungeonAndRoom => (
            "You cannot edit dungeons from Daggerfall",
            Some("Create a copy copies the dungeon and this cell."),
        ),
        CopySubject::RoomInDungeon => (
            "You cannot edit cells from Daggerfall",
            Some("Create a copy copies this cell for this dungeon."),
        ),
    };
    let mut choice = None;
    modal_window(context, "Read-only", |ui| {
        ui.label(message);
        if let Some(explanation) = explanation {
            ui.label(egui::RichText::new(explanation).weak());
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Create a copy").clicked() {
                choice = Some(CopyChoice::CreateCopy);
            }
            if ui.button("Cancel").clicked() {
                choice = Some(CopyChoice::Cancel);
            }
        });
    });
    choice
}

/// Draws a small dialog in the middle of the screen over a dimmed background
/// that swallows clicks, so the rest of the window cannot be used until it is answered.
fn modal_window(context: &egui::Context, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    let screen = context.screen_rect();
    egui::Area::new(egui::Id::new(("modal_background", title)))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(context, |ui| {
            ui.painter().rect_filled(screen, 0.0, egui::Color32::from_black_alpha(110));
            ui.allocate_rect(screen, egui::Sense::click_and_drag());
        });
    let dialog = egui::Area::new(egui::Id::new(("modal_dialog", title)))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(context, |ui| {
            egui::Frame::window(ui.style()).show(ui, |ui| {
                ui.set_min_width(260.0);
                ui.strong(title);
                ui.separator();
                add_contents(ui);
            });
        });
    // Keep the dialog above its dimmed background.
    context.move_to_top(dialog.response.layer_id);
}

/// Draws the menu bar, status bar and editor inside a room editor window.
fn show_room_window_contents(
    context: &egui::Context,
    window: &mut RoomWindow,
    resources: &mut EditorResources<'_>,
) {
    let editor_id = window.editor.id();
    window.editor.handle_shortcuts(context, resources.library);

    egui::TopBottomPanel::top(editor_id.with("menu_bar")).show(context, |ui| {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("Edit", |ui| window.editor.edit_menu(ui));
            ui.menu_button("View", |ui| {
                window.editor.view_menu(ui, resources.library);
                ui.separator();
                viewport_tools::dungeon_texture_menu(
                    ui,
                    resources.texture_setting,
                    resources.game_texture_mode,
                );
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Viewport controls").clicked() {
                    window.show_controls_window = true;
                    ui.close_menu();
                }
            });
            ui.separator();
            window.editor.toolbar(ui);
        });
    });
    egui::TopBottomPanel::bottom(editor_id.with("status_bar")).show(context, |ui| {
        ui.horizontal(|ui| {
            ui.label(window.editor.status_message());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new("Changes appear in the dungeon view.").weak());
            });
        });
    });
    window.editor.show(context, resources);
    show_controls_window(context, &mut window.show_controls_window);

    if context.input(|input| input.viewport().close_requested()) {
        window.is_open = false;
    }
}

/// Why a model has no texture: `missing` (what it names), which could not be found or read.
fn missing_reason(missing: &[String]) -> String {
    format!("these could not be found or read: {}", missing.join("; "))
}

/// Why a model has no texture: its materials `materials` give only a colour.
fn colour_only_reason(materials: &[String]) -> String {
    let names = materials.iter().map(|name| format!("'{name}'")).collect::<Vec<_>>().join(", ");
    let (its, names_none) = match materials.len() {
        1 => ("its material", "names no texture image"),
        _ => ("its materials", "name no texture image"),
    };
    format!(
        "{its} {names} {names_none}, only a colour, though the model has texture coordinates for one. (Blender \
         names a material's image in an OBJ export only when an Image Texture node is plugged straight into the \
         material's Base Color; colours from other nodes, painted vertices or procedural textures are not exported \
         as an image.)"
    )
}

/// Every location of the game, as the library last read them.
fn library_locations(library: &Arena2Library) -> std::sync::Arc<[crate::arena2::LocationDefinition]> {
    library.cached_locations()
}

/// Draws a hint in the middle of an otherwise empty window.
fn show_centered_hint(context: &egui::Context, hint: &str) {
    egui::CentralPanel::default().show(context, |ui| {
        ui.centered_and_justified(|ui| ui.label(egui::RichText::new(hint).size(15.0).weak()));
    });
}

/// Draws the viewport controls help window.
fn show_controls_window(context: &egui::Context, is_open: &mut bool) {
    const CONTROLS: [(&str, &str); 23] = [
        ("Hover a cell (dungeon view)", "Highlight the whole cell"),
        ("Double-click a cell (dungeon view)", "Open it in a cell editor window"),
        ("Click an asset tile", "Arm that asset for placement"),
        ("Left click (while placing)", "Place the asset; hold Shift to keep placing"),
        ("Q / E or R (while placing)", "Turn the asset 90° anticlockwise / clockwise"),
        ("Alt (while placing)", "Place without snapping to open faces"),
        ("Esc (while placing)", "Stop placing"),
        ("Left click", "Select object"),
        ("Left drag on arrow", "Move selection along that axis"),
        ("Left drag on ring", "Turn the selected object around that axis (15° steps with grid lock)"),
        ("G", "Turn grid lock on or off"),
        ("Ctrl while dragging", "Switch grid lock for that drag"),
        ("Right drag", "Orbit"),
        ("Middle drag / Shift + right drag", "Pan"),
        ("Mouse wheel", "Zoom"),
        ("W A S D", "Move forward / left / back / right along the ground (hold Shift to go faster)"),
        ("Space / Ctrl", "Raise / lower the camera (also Right mouse + E / Q)"),
        ("F", "Frame selection"),
        ("Home", "Frame cell or dungeon"),
        ("E or R / Q or Shift + R", "Turn the selected object 90° clockwise / anticlockwise"),
        ("Ctrl + D", "Duplicate selection"),
        ("Delete", "Delete selection"),
        ("Ctrl + Z / Ctrl + Y", "Undo / redo"),
    ];
    egui::Window::new("Viewport controls").open(is_open).resizable(false).show(context, |ui| {
        egui::Grid::new("controls_grid").striped(true).show(ui, |ui| {
            for (input, action) in CONTROLS {
                ui.strong(input);
                ui.label(action);
                ui.end_row();
            }
        });
    });
}
