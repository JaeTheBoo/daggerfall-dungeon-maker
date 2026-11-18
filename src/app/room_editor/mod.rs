//! A complete editor for one room: outliner, properties, asset palette and 3D viewport.
//!
//! A [`RoomEditor`] owns everything specific to the room being edited
//! (scene, camera, selection, undo history) and borrows the shared game data
//! and renderer through [`EditorResources`]. That lets the same editor run in
//! the main window or in its own window opened from the dungeon view.
//!
//! The objects of a placed prefab (see [`crate::prefabs`]) are always selected
//! together: selecting one selects them all, so they move, turn, copy and
//! delete as one. Their own properties cannot be changed until the prefab is
//! broken up (**Break Prefab** in their right-click menu).
//!
//! The outliner lists the object lists the user made (folders, see
//! [`ObjectFolder`]) first, then the rest of the objects by the quarter of the
//! cell they are in, as Daggerfall lists them ([`crate::scene::quarter_of`]).
//! Objects join the quarter they are placed, pasted or moved into.

mod asset_palette;
pub use asset_palette::CustomModelRequest;
mod history;
mod panels;
mod viewport;

use eframe::{egui, egui_wgpu};
use glam::Vec3;

use super::project_panel::OpenedItem;
use super::viewport_tools::EditorResources;
use crate::arena2::{Arena2Library, DoorKind, RDB_BLOCK_SIDE};
use crate::camera::OrbitCamera;
use crate::dungeon_textures::DungeonIdentity;
use crate::encounters::{enemy_chances, DungeonType, EncounterTable, EnemyChance};
use crate::project::CellCategory;
use crate::render::{RoomOutline, ViewSettings, ViewportTarget};
use crate::scene::{
    quarter_of, ActionMotion, ObjectAction, ObjectFolder, ObjectId, PrefabMember, Scene, SceneObject,
    SceneObjectKind, OPEN_DOOR_ACTION, OUTSIDE_CELL_TOLERANCE,
};
use asset_palette::{AssetPalette, PREFAB_COLOR, PREFAB_MARK};
use history::{EditHistory, SceneEdit};
use viewport::ViewportState;

/// How far a duplicated object is offset from the original, in world units.
const DUPLICATE_OFFSET: Vec3 = Vec3::new(1.0, 0.0, 0.0);
/// Numbers every editor made, so copied objects know which editor they came from.
static NEXT_EDITOR_INSTANCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Objects copied with Ctrl+C. They are kept in egui's memory, so every cell
/// editor (the main window's and each cell window's) can paste them.
#[derive(Clone)]
struct CopiedObjects {
    objects: Vec<SceneObject>,
    /// The editor they were copied in (see [`RoomEditor::instance`]).
    source: u64,
    /// The text put on the system clipboard with them. Ctrl+V only reaches
    /// the program with the clipboard's text, so a paste counts when it
    /// brings this text back, and not after something else was copied.
    marker: String,
    /// The editor pasted into last, and how many times in a row, so each
    /// paste there lands a little further along.
    pasted_into: Option<u64>,
    pastes: u32,
}

/// Where [`CopiedObjects`] are kept in egui's memory.
fn copied_objects_id() -> egui::Id {
    egui::Id::new("ddm_copied_objects")
}

/// Radius used to frame objects without a mesh (flats and lights), in world units.
const DEFAULT_FOCUS_RADIUS: f32 = 2.0;
/// Highest player level offered when previewing random enemies; past it the tables no longer change.
const MAX_PREVIEW_LEVEL: u32 = 30;

/// Snap steps offered for grid lock, in world units, with their labels. Each
/// divides the 3.2 m floor grid evenly, so snap lines meet its lines.
const SNAP_STEPS: [(f32, &str); 4] = [
    (0.4, "0.4 m (16 units)"),
    (0.8, "0.8 m (32 units)"),
    (1.6, "1.6 m (64 units)"),
    (3.2, "3.2 m (128 units)"),
];

/// Grid lock: snapping objects to a grid when moving or placing them.
#[derive(Debug, Clone, Copy)]
pub struct SnapSettings {
    /// Whether grid lock is on. Holding Ctrl while dragging inverts this for that drag.
    pub enabled: bool,
    /// Snap increment in world units, one of [`SNAP_STEPS`].
    pub step: f32,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            step: 0.4, // 16 native units.
        }
    }
}

/// Editor state for one room.
pub struct RoomEditor {
    /// The room being edited.
    pub scene: Scene,
    /// Title shown in window titles, for example the room name and its dungeon.
    title: String,
    /// Prefix for widget ids so several editors can be open at once.
    id: egui::Id,
    /// Number of this editor, unique for as long as the program runs.
    instance: u64,
    /// The selected object; with several selected, the one selected last.
    selected_object: Option<usize>,
    /// Every selected object when several are (including `selected_object`). It
    /// only counts while it contains `selected_object`, so selecting one object
    /// any other way ends the multiple selection.
    selection_set: Vec<usize>,
    camera: OrbitCamera,
    view_settings: ViewSettings,
    snap: SnapSettings,
    history: EditHistory,
    /// Object currently being edited in the properties panel, so a drag records one undo step.
    property_edit_session: Option<usize>,
    viewport: ViewportState,
    palette: AssetPalette,
    /// Height of the floor grid, placed just under the room.
    grid_height: f32,
    target: ViewportTarget,
    status_message: String,
    /// Set for rooms from Daggerfall, which can be looked at but not changed.
    read_only: bool,
    /// Set when the user tried to change a read-only room, so the application can offer a copy.
    edit_blocked: bool,
    /// Name of the project room being edited; `None` for rooms from Daggerfall.
    project_room_name: Option<String>,
    /// The project room's category, which the user can change; `None` for rooms
    /// from Daggerfall, whose category comes from their block name.
    cell_category: Option<CellCategory>,
    /// Type of the dungeon the room was opened from, which decides what random enemy markers become.
    dungeon_type: Option<DungeonType>,
    /// Player level used to preview which enemies random enemy markers can become.
    enemy_preview_level: u32,
    /// Dungeons that use this cell, listed under "Used in"; set by the application.
    used_in: Vec<UsedInEntry>,
    /// A dungeon the user double-clicked under "Used in", for the application to open.
    open_request: Option<OpenedItem>,
    /// An action being played back in the viewport, if any.
    preview: Option<ActionPreview>,
    /// Open faces of the models placed or being placed, for snapping, by model id.
    open_faces: std::collections::HashMap<u32, std::sync::Arc<[crate::connectors::OpenFace]>>,
    /// Openings of the cell's models that lead to empty space, shown as yellow panes.
    unclosed: Vec<crate::connectors::UnclosedFace>,
    /// What the openings were last checked for (a hash of the objects), and when.
    unclosed_checked: Option<(u64, f64)>,
    /// The dungeon selected under "Used in", whose other cells are shown faded around this one.
    selected_dungeon: Option<OpenedItem>,
    /// The dungeon and layout place this cell was opened at, so the cells around
    /// that place (not another use of the same cell) are shown for that dungeon.
    home_placement: Option<(OpenedItem, usize)>,
    /// How far each object reaches outside the cell's square, refreshed every frame.
    outside_cell: Vec<f32>,
    /// A surrounding cell the user double-clicked: its dungeon and layout place.
    cell_open_request: Option<(OpenedItem, usize)>,
    /// Set when the dungeon was selected for the user, so its row scrolls into view once.
    scroll_to_selected_dungeon: bool,
    /// Set when `selected_dungeon` changes, for the application to load its cells.
    context_request: Option<Option<OpenedItem>>,
    /// The selected dungeon's other cells, placed around this one, and their outlines.
    dungeon_context: Option<DungeonContext>,
    /// What the game picks the selected dungeon's textures from, if known.
    dungeon_identity: Option<DungeonIdentity>,
    /// The cell's doors (object indexes) the game switches off, refreshed every frame.
    switched_off_doors: Vec<usize>,
    /// What is typed in the open right-click menu.
    menu_text: ObjectMenuText,
    /// Whether the rename typed in the open right-click menu has its undo step yet.
    rename_recorded: bool,
    /// The name typed for an object list in its right-click menu in the outliner.
    folder_rename: String,
    /// The object clicked last without Shift, where Shift+click in the
    /// outliner selects from. Kept by id, so edits to the cell do not move it.
    selection_anchor: Option<ObjectId>,
    /// The outliner's rows drawn this frame, in order (rows under a closed heading are not drawn).
    outliner_shown: Vec<usize>,
    /// A row clicked in the outliner this frame and the keys held, handled
    /// once every row is drawn.
    outliner_click: Option<(usize, egui::Modifiers)>,
}

/// A heading of the outliner.
pub(super) enum OutlinerSection {
    /// An object list the user made.
    Folder(ObjectFolder),
    /// A quarter of the cell (a group index), for the objects in no object list.
    Quarter(usize),
}

/// The cells around the edited one in a dungeon, shown faded for context.
pub struct DungeonContext {
    /// The dungeon they belong to.
    pub dungeon: OpenedItem,
    /// Their objects, positioned relative to the edited cell. Each object's
    /// `group_index` is the index of its cell in `cells`.
    pub scene: Scene,
    /// Outlines of the edited cell (first, at the origin) and of each surrounding
    /// cell, in the order of `cells`.
    pub outlines: Vec<RoomOutline>,
    /// The surrounding cells.
    pub cells: Vec<ContextCell>,
    /// Their doors, for finding the doors the game switches off.
    pub doors: ContextDoors,
}

/// The doors of the cells around the edited one, for finding the doors
/// Daggerfall Unity switches off (see [`crate::doors`]). Positions are in the
/// dungeon's coordinates.
#[derive(Debug, Clone, Default)]
pub struct ContextDoors {
    /// Where the edited cell's corner is in the dungeon.
    pub origin: Vec3,
    /// Whether the edited cell is the dungeon's start cell.
    pub is_start: bool,
    /// The exit door's point in the game's list, when the start is another cell.
    pub exit: Option<Vec3>,
    /// The other cells' action doors in the game's order: those of the cells
    /// before the edited one in the dungeon's layout, and those after it.
    pub before: Vec<Vec3>,
    pub after: Vec<Vec3>,
}

/// One cell shown faded around the edited one.
pub struct ContextCell {
    /// Name shown when the pointer is over it.
    pub label: String,
    /// Its place in the dungeon's layout.
    pub layout_index: usize,
}

/// How long a finished preview stays in its end state before everything returns.
const PREVIEW_HOLD_SECONDS: f64 = 0.5;

/// An action chain being played back in the viewport, without changing the cell.
struct ActionPreview {
    /// Time the preview started, from egui's clock.
    started: f64,
    /// Each moving object and how it moves; they all start together, as in the game.
    motions: Vec<(usize, ActionMotion)>,
}

/// A dungeon listed under "Used in".
pub struct UsedInEntry {
    pub label: String,
    pub item: OpenedItem,
}

/// What the right-click menu of an object offers.
pub(super) struct ObjectMenuInfo {
    name: String,
    /// Whether the object can have an action (models and flats).
    can_tie: bool,
    /// How many objects' actions trigger this object.
    triggered_by: usize,
    /// The name of the placed prefab the object is part of, if it is.
    prefab: Option<String>,
    /// How many objects are selected (a prefab counts all of its objects).
    selected: usize,
    /// Whether the selected objects can become a prefab: several, not all one prefab already.
    can_create_prefab: bool,
    /// Whether the menu can rename what is selected: one object, or one prefab.
    can_rename: bool,
    /// The name of the object list the object is in, if it is in one.
    folder: Option<String>,
}

/// What is typed in an object's right-click menu, kept while it is open.
#[derive(Debug, Clone, Default)]
pub(super) struct ObjectMenuText {
    /// The new name of the object, or of the prefab it is part of.
    pub rename: String,
    /// The name of a new prefab.
    pub prefab_name: String,
    /// The name of a new object list.
    pub list_name: String,
}

/// What the user chose in an object's right-click menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ObjectMenuChoice {
    /// The name typed under Rename changed: rename the object (or its prefab) to it.
    Rename,
    Duplicate,
    Delete,
    /// Select every object inside the box of what is selected.
    SelectAllWithin,
    TieAction,
    RemoveActions,
    /// Make the selected objects a prefab, called as typed in the menu.
    CreatePrefab,
    /// Turn the prefab the object is part of back into separate objects.
    BreakPrefab,
    /// Put every object inside the box of what is selected in a new object list, called as typed.
    CreateObjectList,
    /// Take the selected objects out of their object lists.
    LeaveObjectList,
}

/// What the user chose in an object list's right-click menu in the outliner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FolderMenuChoice {
    /// The name typed under Rename changed.
    Rename,
    SelectAll,
    Delete,
}

/// Marks the object lists the user made, in the outliner.
pub(super) const FOLDER_MARK: &str = "🗀";

/// Draws the entries of the right-click menu of the object list `folder`;
/// `name` is the name typed for it. Returns the user's choice.
pub(super) fn folder_menu(
    ui: &mut egui::Ui,
    folder: &ObjectFolder,
    name: &mut String,
) -> Option<FolderMenuChoice> {
    let mut choice = None;
    ui.label(egui::RichText::new(format!("{FOLDER_MARK} {}", folder.name)).weak());
    ui.horizontal(|ui| {
        ui.label("Rename");
        let field = ui.add(egui::TextEdit::singleline(name).desired_width(150.0));
        if field.changed() {
            choice = Some(FolderMenuChoice::Rename);
        }
        if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            ui.close_menu();
        }
    });
    if ui.button("Select All").on_hover_text("Selects every object in the list").clicked() {
        choice = Some(FolderMenuChoice::SelectAll);
    }
    let hover = "Removes the list; its objects stay in the cell, listed by the quarter they are in again";
    if ui.button("Delete List").on_hover_text(hover).clicked() {
        choice = Some(FolderMenuChoice::Delete);
    }
    if choice.is_some_and(|choice| choice != FolderMenuChoice::Rename) {
        ui.close_menu();
    }
    choice
}

/// Draws the entries of an object's right-click menu: what can be done with
/// the object, or with the prefab it is part of, and with the selection when
/// several objects are selected. `text` holds what is typed in the menu.
/// Returns the user's choice.
pub(super) fn object_menu(
    ui: &mut egui::Ui,
    info: &ObjectMenuInfo,
    text: &mut ObjectMenuText,
) -> Option<ObjectMenuChoice> {
    let mut choice = None;
    let several = info.selected > 1;
    match &info.prefab {
        Some(prefab) => {
            ui.label(egui::RichText::new(format!("{PREFAB_MARK} Prefab: {prefab}")).color(PREFAB_COLOR));
        }
        None if several => {
            ui.label(egui::RichText::new(format!("{} objects selected", info.selected)).weak());
        }
        None => {
            ui.label(egui::RichText::new(&info.name).weak());
        }
    }

    // What can be done with any selection.
    if info.can_rename {
        ui.horizontal(|ui| {
            ui.label("Rename");
            let hover = match info.prefab {
                Some(_) => "The prefab's name, as the outliner and the pointer show it",
                None => "The object's name, as the outliner shows it",
            };
            let field = ui
                .add(egui::TextEdit::singleline(&mut text.rename).desired_width(150.0))
                .on_hover_text(hover);
            if field.changed() {
                choice = Some(ObjectMenuChoice::Rename);
            }
            if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                ui.close_menu();
            }
        });
    }
    let what = match (&info.prefab, several) {
        (Some(_), _) => "the prefab",
        (None, true) => "the selected objects",
        (None, false) => "the object",
    };
    let duplicate = egui::Button::new("Duplicate").shortcut_text("Ctrl+D");
    if ui
        .add(duplicate)
        .on_hover_text(format!("Adds a copy of {what} next to it, and selects the copy"))
        .clicked()
    {
        choice = Some(ObjectMenuChoice::Duplicate);
    }
    let delete = egui::Button::new("Delete").shortcut_text("Del");
    if ui.add(delete).on_hover_text(format!("Removes {what} from the cell; Ctrl+Z brings it back")).clicked()
    {
        choice = Some(ObjectMenuChoice::Delete);
    }
    let within = match (&info.prefab, several) {
        (Some(_), _) => "the box around the prefab (the green one)",
        (None, true) => "the box around the selected objects",
        (None, false) => "the object's box (the one drawn around it when it is selected)",
    };
    let included = if several || info.prefab.is_some() { "the selection" } else { "the object" };
    if ui
        .button("Select All Within")
        .on_hover_text(format!("Selects every object whose middle is inside {within}, {included} included"))
        .clicked()
    {
        choice = Some(ObjectMenuChoice::SelectAllWithin);
    }

    // Object lists.
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("List name");
        ui.add(egui::TextEdit::singleline(&mut text.list_name).desired_width(130.0))
            .on_hover_text("The name of the new object list");
    });
    let hover = format!(
        "Puts every object whose middle is inside {within}, {included} included, in a new object list: a folder \
         in the outliner, to keep the cell organised. The game never sees object lists."
    );
    if ui.button("Create Object List").on_hover_text(hover).clicked() {
        choice = Some(ObjectMenuChoice::CreateObjectList);
    }
    if let Some(folder) = &info.folder {
        let hover = "Takes the selected objects out of their object lists; they stay in the cell";
        if ui.button(format!("Remove from {FOLDER_MARK} {folder}")).on_hover_text(hover).clicked() {
            choice = Some(ObjectMenuChoice::LeaveObjectList);
        }
    }

    match &info.prefab {
        Some(_) => {
            ui.separator();
            let hover = "Turns the prefab back into separate objects, each of which can then be changed";
            if ui.button("Break Prefab").on_hover_text(hover).clicked() {
                choice = Some(ObjectMenuChoice::BreakPrefab);
            }
        }
        None if several => {}
        None => {
            ui.separator();
            if info.can_tie
                && ui
                    .button("Tie action to…")
                    .on_hover_text("Choose the object this one's action triggers")
                    .clicked()
            {
                choice = Some(ObjectMenuChoice::TieAction);
            }
            if info.triggered_by > 0 {
                let hover =
                    format!("Remove the action of the {} object(s) that trigger this one", info.triggered_by);
                if ui.button("Remove actions").on_hover_text(hover).clicked() {
                    choice = Some(ObjectMenuChoice::RemoveActions);
                }
            }
            if !info.can_tie && info.triggered_by == 0 {
                ui.label(egui::RichText::new("Lights cannot have actions").weak());
            }
        }
    }
    if info.can_create_prefab {
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut text.prefab_name).desired_width(140.0))
                .on_hover_text("The name of the new prefab");
        });
        let hover = format!(
            "Groups the {} selected objects into a prefab, and adds it to the Prefabs tab of the asset palette \
             to place again in any cell",
            info.selected
        );
        if ui.button("Create Prefab").on_hover_text(hover).clicked() {
            choice = Some(ObjectMenuChoice::CreatePrefab);
        }
    }
    // The menu stays open while a name is typed.
    if choice.is_some_and(|choice| choice != ObjectMenuChoice::Rename) {
        ui.close_menu();
    }
    choice
}

impl RoomEditor {
    /// Creates an editor for `scene` and frames the camera on it.
    pub fn new(scene: Scene, title: String, id: egui::Id, library: &mut Arena2Library) -> Self {
        let status_message = format!("{title}: {} objects", scene.objects.len());
        let mut editor = Self {
            scene,
            title,
            id,
            instance: NEXT_EDITOR_INSTANCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            selected_object: None,
            selection_set: Vec::new(),
            camera: OrbitCamera::default(),
            view_settings: ViewSettings::default(),
            snap: SnapSettings::default(),
            history: EditHistory::default(),
            property_edit_session: None,
            viewport: ViewportState::default(),
            palette: AssetPalette::default(),
            grid_height: 0.0,
            target: ViewportTarget::default(),
            status_message,
            read_only: false,
            edit_blocked: false,
            project_room_name: None,
            cell_category: None,
            dungeon_type: None,
            enemy_preview_level: 1,
            used_in: Vec::new(),
            open_request: None,
            preview: None,
            open_faces: std::collections::HashMap::new(),
            unclosed: Vec::new(),
            unclosed_checked: None,
            selected_dungeon: None,
            scroll_to_selected_dungeon: false,
            home_placement: None,
            cell_open_request: None,
            outside_cell: Vec::new(),
            context_request: None,
            dungeon_context: None,
            dungeon_identity: None,
            switched_off_doors: Vec::new(),
            menu_text: ObjectMenuText::default(),
            rename_recorded: false,
            folder_rename: String::new(),
            selection_anchor: None,
            outliner_shown: Vec::new(),
            outliner_click: None,
        };
        editor.frame_room(library);
        if let Some((minimum, _)) = editor.scene.bounds(library) {
            editor.grid_height = minimum.y - 0.05;
        }
        editor
    }

    /// Title of the room, for window titles.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Changes the title of the room, for example after it was renamed.
    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    /// Makes the room read-only (a room from Daggerfall) or editable (a project room).
    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
        self.edit_blocked = false;
    }

    /// Sets the project room name shown above the outliner; `None` for rooms from Daggerfall.
    pub fn set_project_room_name(&mut self, name: Option<String>) {
        self.project_room_name = name;
    }

    /// Sets the project room's category, shown and changed in the cell's
    /// properties; `None` for rooms from Daggerfall.
    pub fn set_cell_category(&mut self, category: Option<CellCategory>) {
        self.cell_category = category;
    }

    /// The project room's category, as the user may have changed it.
    pub fn cell_category(&self) -> Option<CellCategory> {
        self.cell_category
    }

    /// Sets the type of the dungeon the room is in, to preview what random enemy markers become.
    pub fn set_dungeon_type(&mut self, dungeon_type: Option<DungeonType>) {
        self.dungeon_type = dungeon_type;
    }

    /// The table the random enemy marker `object_index` picks from: the
    /// underwater table below the cell's water level, otherwise the dungeon's.
    /// `None` above water in a cell that is not open from a dungeon.
    fn encounter_table(&self, object_index: usize) -> Option<EncounterTable> {
        let object = self.scene.objects.get(object_index)?;
        if self.scene.is_under_water(object) {
            return Some(EncounterTable::Underwater);
        }
        self.dungeon_type.map(EncounterTable::Dungeon)
    }

    /// The enemies the random enemy marker `object_index` can become at the
    /// preview level, most likely first; `None` if that depends on an unknown dungeon.
    fn possible_enemies(&self, object_index: usize) -> Option<Vec<EnemyChance>> {
        self.encounter_table(object_index).map(|table| enemy_chances(table, self.enemy_preview_level))
    }

    /// What moves when object `start`'s action is triggered: every object down
    /// its action chain, to the end, whose action moves it, each with its motion.
    /// Doors whose action opens them swing open. Empty if nothing moves.
    pub(super) fn preview_motions(
        &self,
        start: usize,
        library: &mut Arena2Library,
    ) -> Vec<(usize, ActionMotion)> {
        self.scene
            .action_chain(start)
            .into_iter()
            .filter_map(|index| Some((index, self.object_motion(index, library)?)))
            .collect()
    }

    /// How object `index` moves when its action plays, if it moves.
    pub(super) fn object_motion(&self, index: usize, library: &mut Arena2Library) -> Option<ActionMotion> {
        match self.scene.objects.get(index)?.kind {
            SceneObjectKind::Model { model_id, data } => {
                let action = data.action?;
                let tags = library.model_tags();
                if let Some(motion) = action.model_motion(tags.get(&model_id).map(String::as_str)) {
                    Some(motion)
                } else if action.action_type == OPEN_DOOR_ACTION
                    && library.model(model_id).and_then(|mesh| mesh.door_kind()) == Some(DoorKind::Building)
                {
                    Some(ActionMotion::door_swing())
                } else {
                    None
                }
            }
            SceneObjectKind::Flat { data, .. } if data.action != 0 => {
                // Flats' actions use their magnitude byte as both axis and magnitude, with no duration.
                let [magnitude, _] = data.faction_or_mobile_id.to_le_bytes();
                let action = ObjectAction {
                    action_type: data.action,
                    axis: magnitude,
                    duration: 0,
                    magnitude: u16::from(magnitude),
                };
                action.motion()
            }
            _ => None,
        }
    }

    /// Starts playing object `index`'s action chain in the viewport.
    pub(super) fn start_preview(&mut self, index: usize, now: f64, library: &mut Arena2Library) {
        let motions = self.preview_motions(index, library);
        if !motions.is_empty() {
            self.status_message = format!("Previewing {} moving object(s)", motions.len());
            self.preview = Some(ActionPreview { started: now, motions });
        }
    }

    /// The scene as it looks at time `now` during a preview, or `None` when no
    /// preview is playing. A preview ends half a second after its slowest motion.
    pub(super) fn preview_scene(&mut self, now: f64) -> Option<Scene> {
        let preview = self.preview.as_ref()?;
        let elapsed = now - preview.started;
        let longest = preview.motions.iter().map(|(_, motion)| f64::from(motion.seconds)).fold(0.0, f64::max);
        if elapsed > longest + PREVIEW_HOLD_SECONDS {
            self.preview = None;
            return None;
        }
        let mut scene = self.scene.clone();
        for (index, motion) in &preview.motions {
            let fraction = if motion.seconds <= 0.0 {
                1.0
            } else {
                (elapsed / f64::from(motion.seconds)).min(1.0) as f32
            };
            if let Some(object) = scene.objects.get_mut(*index) {
                *object = object.moved_by(motion, fraction);
            }
        }
        Some(scene)
    }

    /// Sets the dungeons listed under "Used in".
    pub fn set_used_in(&mut self, used_in: Vec<UsedInEntry>) {
        self.used_in = used_in;
    }

    /// Returns, once, the dungeon newly selected under "Used in" (or `Some(None)`
    /// when the selection was cleared), so its cells can be loaded with
    /// [`Self::set_dungeon_context`].
    pub fn take_context_request(&mut self) -> Option<Option<OpenedItem>> {
        self.context_request.take()
    }

    /// How far object `index` reaches outside the cell's square, in world units.
    pub(super) fn distance_outside_cell(&self, index: usize, library: &mut Arena2Library) -> f32 {
        let Some(object) = self.scene.objects.get(index) else { return 0.0 };
        object_distance_outside_cell(object, library)
    }

    /// Whether object `index` reaches noticeably outside the cell's square (as of this frame).
    pub(super) fn is_outside_cell(&self, index: usize) -> bool {
        self.outside_cell.get(index).is_some_and(|&distance| distance > OUTSIDE_CELL_TOLERANCE)
    }

    /// Records which place in `dungeon`'s layout this cell was opened at.
    pub fn set_home_placement(&mut self, dungeon: OpenedItem, layout_index: usize) {
        self.home_placement = Some((dungeon, layout_index));
    }

    /// The layout place this cell was opened at in `dungeon`, if it was opened from it.
    pub fn home_layout_index(&self, dungeon: &OpenedItem) -> Option<usize> {
        self.home_placement.as_ref().filter(|(home, _)| home == dungeon).map(|&(_, index)| index)
    }

    /// Returns, once, a surrounding cell the user double-clicked to open.
    pub fn take_cell_open_request(&mut self) -> Option<(OpenedItem, usize)> {
        self.cell_open_request.take()
    }

    /// Sets the cells shown faded around this one, or removes them.
    pub fn set_dungeon_context(&mut self, context: Option<DungeonContext>) {
        self.dungeon_context = context;
    }

    /// The dungeon whose cells are shown around this one, if any.
    pub fn context_dungeon(&self) -> Option<&OpenedItem> {
        self.dungeon_context.as_ref().map(|context| &context.dungeon)
    }

    /// Sets what the game picks the textures of the dungeon around the cell
    /// from; the cell is drawn with them (`None` for the standard ones).
    pub fn set_dungeon_identity(&mut self, identity: Option<DungeonIdentity>) {
        self.dungeon_identity = identity;
    }

    /// The cell's doors the game switches off, among the selected dungeon's
    /// cells (or, without one, among the cell's own doors).
    fn find_switched_off_doors(&self, library: &mut Arena2Library) -> Vec<usize> {
        let model_tags = library.model_tags();
        let objects = &self.scene.objects;
        let own = crate::doors::action_doors_in_order(objects, &model_tags);
        if own.is_empty() {
            return Vec::new();
        }
        let empty = ContextDoors::default();
        let doors = self.dungeon_context.as_ref().map_or(&empty, |context| &context.doors);
        let exit = match doors.is_start {
            true => crate::doors::exit_door_point(objects, library),
            false => doors.exit,
        };
        let list = doors
            .before
            .iter()
            .map(|&position| (None, position))
            .chain(own.iter().map(|&index| (Some(index), doors.origin + objects[index].position)))
            .chain(doors.after.iter().map(|&position| (None, position)));
        crate::doors::switched_off(exit, list.collect::<Vec<_>>()).into_iter().flatten().collect()
    }

    /// Selects `dungeon` under "Used in" (or clears the selection) and asks for its cells.
    pub(super) fn select_used_in(&mut self, dungeon: Option<OpenedItem>) {
        if self.selected_dungeon != dungeon {
            self.scroll_to_selected_dungeon = dungeon.is_some();
            self.selected_dungeon = dungeon.clone();
            self.context_request = Some(dungeon);
            if self.selected_dungeon.is_none() {
                self.dungeon_context = None;
            }
        }
    }

    /// Returns, once, a dungeon the user asked to open from "Used in".
    pub fn take_open_request(&mut self) -> Option<OpenedItem> {
        self.open_request.take()
    }

    /// What the right-click menu of object `index` offers, with the current selection.
    pub(super) fn object_menu_info(
        &self,
        index: usize,
        resources: &EditorResources<'_>,
    ) -> Option<ObjectMenuInfo> {
        let object = self.scene.objects.get(index)?;
        let selected = self.selected_indices();
        Some(ObjectMenuInfo {
            name: panels::object_display_name(object, resources.custom_names, resources.catalog),
            can_tie: object.can_have_action(),
            triggered_by: self.scene.objects_triggering(object.id).len(),
            prefab: object.prefab.as_ref().map(|member| member.name.clone()),
            selected: selected.len(),
            can_create_prefab: selected.len() > 1 && self.selected_prefab().is_none(),
            can_rename: selected.len() <= 1 || self.selected_prefab().is_some(),
            folder: object.folder.as_ref().map(|folder| folder.name.clone()),
        })
    }

    /// Gets the right-click menu of object `index` ready to open: the name it
    /// shows to rename the object (or its prefab), and the one it offers a new prefab.
    pub(super) fn prepare_object_menu(&mut self, index: Option<usize>, resources: &EditorResources<'_>) {
        let object = index.and_then(|index| self.scene.objects.get(index));
        self.menu_text = ObjectMenuText {
            rename: match object {
                Some(SceneObject { prefab: Some(member), .. }) => member.name.clone(),
                Some(object) => {
                    panels::object_display_name(object, resources.custom_names, resources.catalog)
                }
                None => String::new(),
            },
            prefab_name: resources.prefabs.new_name(),
            list_name: self.scene.new_folder_name(),
        };
        self.rename_recorded = false;
    }

    /// Carries out the choice made in the right-click menu of object `index`.
    pub(super) fn apply_object_menu(
        &mut self,
        choice: ObjectMenuChoice,
        index: usize,
        resources: &mut EditorResources<'_>,
    ) {
        match choice {
            ObjectMenuChoice::Rename => self.rename_from_menu(index),
            ObjectMenuChoice::Duplicate => self.duplicate_selection(),
            ObjectMenuChoice::Delete => self.delete_selection(),
            ObjectMenuChoice::SelectAllWithin => self.select_all_within(index, resources),
            ObjectMenuChoice::TieAction => self.start_tying(index, resources),
            ObjectMenuChoice::RemoveActions => self.remove_actions_triggering(index, resources),
            ObjectMenuChoice::CreatePrefab => self.create_prefab(resources),
            ObjectMenuChoice::BreakPrefab => self.break_prefab(index),
            ObjectMenuChoice::CreateObjectList => self.create_object_list(index, resources),
            ObjectMenuChoice::LeaveObjectList => self.leave_object_lists(),
        }
    }

    /// Carries out the choice made in the right-click menu of the object list `id`.
    pub(super) fn apply_folder_menu(&mut self, choice: FolderMenuChoice, id: u32) {
        let members = self.scene.folder_members(id);
        match choice {
            FolderMenuChoice::Rename => {
                if !self.can_edit() {
                    return;
                }
                if !self.rename_recorded {
                    self.record_replacements(&members);
                    self.rename_recorded = true;
                }
                for &index in &members {
                    if let Some(folder) = self.scene.objects[index].folder.as_mut() {
                        folder.name = self.folder_rename.clone();
                    }
                }
                self.status_message = format!("Renamed the object list to {}", self.folder_rename.trim());
            }
            FolderMenuChoice::SelectAll => {
                self.status_message = format!("Selected the {} objects of the list", members.len());
                self.select_objects(members);
                self.viewport.selection_changed_in_viewport = true;
            }
            FolderMenuChoice::Delete => {
                if !self.can_edit() {
                    return;
                }
                self.record_replacements(&members);
                for &index in &members {
                    self.scene.objects[index].folder = None;
                }
                self.status_message = format!(
                    "Removed the object list; its {} objects are listed by quarter again",
                    members.len()
                );
            }
        }
    }

    /// Gets the right-click menu of the object list `folder` ready to open.
    pub(super) fn prepare_folder_menu(&mut self, folder: &ObjectFolder) {
        self.folder_rename = folder.name.clone();
        self.rename_recorded = false;
    }

    /// Records, as one undo step, the objects `indices` as they are, before they are changed.
    fn record_replacements(&mut self, indices: &[usize]) {
        let reverts = indices
            .iter()
            .map(|&index| SceneEdit::Replace { index, object: self.scene.objects[index].clone() })
            .collect();
        self.history.record(SceneEdit::Group(reverts));
    }

    /// The placed prefab that is selected, when the selection is exactly one
    /// prefab: its mark and its objects.
    pub(super) fn selected_prefab(&self) -> Option<(PrefabMember, Vec<usize>)> {
        let selected = self.selected_indices();
        let member = self.scene.objects.get(*selected.first()?)?.prefab.clone()?;
        let members = self.scene.prefab_members(member.instance);
        (members.len() == selected.len()).then_some((member, members))
    }

    /// Makes the selected objects a prefab: adds it to the user's prefabs,
    /// called as typed in the right-click menu, and groups the objects as a
    /// placed copy of it (one undo step). In a read-only cell the prefab is
    /// only added to the user's prefabs.
    fn create_prefab(&mut self, resources: &mut EditorResources<'_>) {
        let indices = self.selected_indices();
        if indices.len() < 2 {
            self.status_message = "Select at least two objects to make a prefab of them".to_string();
            return;
        }
        let name = match self.menu_text.prefab_name.trim() {
            "" => resources.prefabs.new_name(),
            name => name.to_string(),
        };
        // Its base: the middle of the bottom of the box around the objects.
        let base = match self.scene.bounds_of(&indices, resources.library) {
            Some((minimum, maximum)) => {
                Vec3::new((minimum.x + maximum.x) * 0.5, minimum.y, (minimum.z + maximum.z) * 0.5)
            }
            None => {
                indices.iter().map(|&index| self.scene.objects[index].position).sum::<Vec3>()
                    / indices.len() as f32
            }
        };
        let objects: Vec<SceneObject> =
            indices.iter().map(|&index| self.scene.objects[index].clone()).collect();
        resources.prefabs.add(&name, &objects, base);
        self.palette.show_prefabs();
        if self.read_only {
            self.status_message = format!(
                "Added the prefab {name} ({} objects) to the Prefabs tab; this cell is Daggerfall's own, so it \
                 stays as it was",
                indices.len()
            );
            return;
        }
        let member = PrefabMember { instance: self.scene.new_prefab_instance(), name: name.clone() };
        let reverts = indices
            .iter()
            .map(|&index| SceneEdit::Replace { index, object: self.scene.objects[index].clone() })
            .collect();
        for &index in &indices {
            self.scene.objects[index].prefab = Some(member.clone());
        }
        self.history.record(SceneEdit::Group(reverts));
        self.status_message = format!(
            "Created the prefab {name} from {} objects; it is in the Prefabs tab of the asset palette",
            indices.len()
        );
    }

    /// Renames object `index` (or the placed prefab it is part of) to the name
    /// typed in its right-click menu. The whole rename, however many letters
    /// are typed, is one undo step. A model given a blank name goes back to
    /// being listed by its model's name, and other objects to their usual name.
    fn rename_from_menu(&mut self, index: usize) {
        let Some(object) = self.scene.objects.get(index) else { return };
        let indices = match &object.prefab {
            Some(member) => self.scene.prefab_members(member.instance),
            None => vec![index],
        };
        if !self.can_edit() {
            return;
        }
        let name = self.menu_text.rename.clone();
        if !self.rename_recorded {
            let reverts = indices
                .iter()
                .map(|&index| SceneEdit::Replace { index, object: self.scene.objects[index].clone() })
                .collect();
            self.history.record(SceneEdit::Group(reverts));
            self.rename_recorded = true;
        }
        for &index in &indices {
            let object = &mut self.scene.objects[index];
            match object.prefab.as_mut() {
                Some(member) => member.name = name.clone(),
                None if name.trim().is_empty() => object.name = default_object_name(object),
                None => object.name = name.clone(),
            }
        }
        let what = if indices.len() > 1 { "the prefab" } else { "the object" };
        self.status_message = format!("Renamed {what} to {}", name.trim());
    }

    /// The objects whose middle is inside the box of what is selected (`index`
    /// was right-clicked): one model's own box, turned with it, or the box
    /// around a prefab or several objects. Only objects the view shows count;
    /// prefabs the box reaches into come whole. Returns them, what was
    /// selected first and `index` last, or `None` for a lone flat or light,
    /// which has no box.
    fn objects_within(&self, index: usize, library: &mut Arena2Library) -> Option<Vec<usize>> {
        let selected = self.selected_indices();
        let object = self.scene.objects.get(index)?;
        let contains: Box<dyn Fn(Vec3) -> bool> = match (selected.len(), &object.kind) {
            (0 | 1, SceneObjectKind::Model { model_id, .. }) => {
                let mesh = library.model(*model_id)?;
                let to_local = object.model_matrix().inverse();
                let (minimum, maximum) = (
                    mesh.bounds_min - Vec3::splat(BOX_TOLERANCE),
                    mesh.bounds_max + Vec3::splat(BOX_TOLERANCE),
                );
                Box::new(move |point| {
                    let local = to_local.transform_point3(point);
                    local.cmpge(minimum).all() && local.cmple(maximum).all()
                })
            }
            (0 | 1, _) => return None,
            _ => {
                let (minimum, maximum) = self.scene.bounds_of(&selected, library)?;
                let (minimum, maximum) =
                    (minimum - Vec3::splat(BOX_TOLERANCE), maximum + Vec3::splat(BOX_TOLERANCE));
                Box::new(move |point| point.cmpge(minimum).all() && point.cmple(maximum).all())
            }
        };
        let mut found: Vec<usize> = selected;
        for (other, candidate) in self.scene.objects.iter().enumerate() {
            if !found.contains(&other)
                && self.is_shown(candidate)
                && contains(object_middle(candidate, library))
            {
                found.push(other);
            }
        }
        found.retain(|&other| other != index);
        found.push(index);
        Some(self.with_whole_prefabs(found))
    }

    /// Selects every object inside the box of what is selected (see
    /// [`Self::objects_within`]); what was selected stays selected.
    fn select_all_within(&mut self, index: usize, resources: &mut EditorResources<'_>) {
        let before = self.selected_indices().len().max(1);
        let Some(found) = self.objects_within(index, resources.library) else {
            self.status_message =
                "Only models have a box to select within; right-click a model, a prefab or several objects"
                    .to_string();
            return;
        };
        let total = found.len();
        self.select_objects(found);
        self.viewport.selection_changed_in_viewport = true;
        self.status_message = format!(
            "Selected {total} objects within the box ({} more than before)",
            total.saturating_sub(before)
        );
    }

    /// Puts every object inside the box of what is selected (see
    /// [`Self::objects_within`]; a lone flat or light on its own) in a new
    /// object list, called as typed in the right-click menu, and selects them.
    /// Objects already in a list move to the new one. One undo step.
    fn create_object_list(&mut self, index: usize, resources: &mut EditorResources<'_>) {
        let members =
            self.objects_within(index, resources.library).unwrap_or_else(|| self.selected_indices());
        if members.is_empty() || !self.can_edit() {
            return;
        }
        let name = match self.menu_text.list_name.trim() {
            "" => self.scene.new_folder_name(),
            name => name.to_string(),
        };
        let folder = ObjectFolder { id: self.scene.new_folder_id(), name: name.clone() };
        self.record_replacements(&members);
        for &index in &members {
            self.scene.objects[index].folder = Some(folder.clone());
        }
        self.status_message = format!("Made the object list {name} of {} objects", members.len());
        self.select_objects(members);
        self.viewport.selection_changed_in_viewport = true;
    }

    /// Takes the selected objects out of their object lists (one undo step).
    fn leave_object_lists(&mut self) {
        let indices: Vec<usize> = self
            .selected_indices()
            .into_iter()
            .filter(|&index| self.scene.objects[index].folder.is_some())
            .collect();
        if indices.is_empty() || !self.can_edit() {
            return;
        }
        self.record_replacements(&indices);
        for &index in &indices {
            self.scene.objects[index].folder = None;
        }
        self.status_message = format!("Took {} objects out of their object lists", indices.len());
    }

    /// Whether the view shows `object`: it is visible, and its kind is shown (View menu).
    pub(super) fn is_shown(&self, object: &SceneObject) -> bool {
        let settings = &self.view_settings;
        object.visible
            && match object.kind {
                SceneObjectKind::Model { .. } => settings.show_models,
                SceneObjectKind::Flat { .. } if object.is_marker() => settings.show_markers,
                SceneObjectKind::Flat { .. } => settings.show_flats,
                SceneObjectKind::Light { .. } => settings.show_lights,
            }
    }

    /// Turns the placed prefab object `index` is part of back into separate
    /// objects, which stay selected (one undo step).
    fn break_prefab(&mut self, index: usize) {
        let Some(member) = self.scene.objects.get(index).and_then(|object| object.prefab.clone()) else {
            return;
        };
        if !self.can_edit() {
            return;
        }
        let members = self.scene.prefab_members(member.instance);
        let reverts = members
            .iter()
            .map(|&index| SceneEdit::Replace { index, object: self.scene.objects[index].clone() })
            .collect();
        for &index in &members {
            self.scene.objects[index].prefab = None;
        }
        self.history.record(SceneEdit::Group(reverts));
        self.status_message = format!(
            "Broke up the prefab {}: its {} objects can now be changed one by one",
            member.name,
            members.len()
        );
    }

    /// Starts choosing the object that object `index`'s action triggers.
    fn start_tying(&mut self, index: usize, resources: &EditorResources<'_>) {
        if !self.can_edit() {
            return;
        }
        let Some(object) = self.scene.objects.get(index) else { return };
        self.palette.armed = None;
        self.viewport.tying_from = Some(object.id);
        self.selected_object = Some(index);
        let name = panels::object_display_name(object, resources.custom_names, resources.catalog);
        self.status_message =
            format!("Tying the action of {name}: click the object it should trigger (Esc to cancel)");
    }

    /// Stops choosing an action target without changing anything.
    fn cancel_tying(&mut self) {
        if self.viewport.tying_from.take().is_some() {
            self.status_message = "Tying cancelled".to_string();
        }
    }

    /// Makes the action of object `source_id` trigger object `target_index`.
    fn tie_action(&mut self, source_id: ObjectId, target_index: usize, resources: &EditorResources<'_>) {
        self.viewport.tying_from = None;
        let Some(source_index) = self.scene.objects.iter().position(|object| object.id == source_id) else {
            return;
        };
        let Some(target_id) = self.scene.objects.get(target_index).map(|object| object.id) else { return };
        if source_index == target_index || !self.can_edit() {
            return;
        }
        let object = &mut self.scene.objects[source_index];
        self.history.record(SceneEdit::Replace { index: source_index, object: object.clone() });
        object.tie_action_to(target_id);
        let describe = |index: usize| {
            panels::object_display_name(&self.scene.objects[index], resources.custom_names, resources.catalog)
        };
        self.status_message = format!("{} now triggers {}", describe(source_index), describe(target_index));
        self.selected_object = Some(source_index);
    }

    /// Removes the actions of every object that triggers object `index`, as one undo step.
    fn remove_actions_triggering(&mut self, index: usize, resources: &EditorResources<'_>) {
        let Some(target_id) = self.scene.objects.get(index).map(|object| object.id) else { return };
        let sources = self.scene.objects_triggering(target_id);
        if sources.is_empty() || !self.can_edit() {
            return;
        }
        let reverts = sources
            .iter()
            .map(|&source| SceneEdit::Replace { index: source, object: self.scene.objects[source].clone() })
            .collect();
        for &source in &sources {
            self.scene.objects[source].remove_action();
        }
        self.history.record(SceneEdit::Group(reverts));
        let name = panels::object_display_name(
            &self.scene.objects[index],
            resources.custom_names,
            resources.catalog,
        );
        self.status_message = format!("Removed {} action(s) that triggered {name}", sources.len());
    }

    /// Whether the room belongs to the project (rather than being a Daggerfall room).
    pub fn is_project_room(&self) -> bool {
        self.project_room_name.is_some()
    }

    /// Returns `true` once after the user tried to change a read-only room.
    pub fn take_blocked_edit(&mut self) -> bool {
        std::mem::take(&mut self.edit_blocked)
    }

    /// Whether the room may be changed. For a read-only room this records the
    /// attempt (see [`Self::take_blocked_edit`]) and returns `false`.
    fn can_edit(&mut self) -> bool {
        if self.read_only {
            self.edit_blocked = true;
        }
        !self.read_only
    }

    /// Base id of this editor's widgets, unique among open editors.
    pub fn id(&self) -> egui::Id {
        self.id
    }

    /// Latest status message, for the status bar.
    pub fn status_message(&self) -> &str {
        &self.status_message
    }

    /// Replaces the status message, for example to report something the application did.
    pub fn set_status_message(&mut self, message: String) {
        self.status_message = message;
    }

    /// Frees the GPU image this editor renders into. Call when the editor is closed.
    pub fn release(&mut self, render_state: &egui_wgpu::RenderState) {
        self.target.release(render_state);
    }

    /// Draws the side panels, asset palette and viewport into `context`.
    ///
    /// Call this after any top or bottom bars of the window have been added.
    pub fn show(&mut self, context: &egui::Context, resources: &mut EditorResources<'_>) {
        self.outside_cell = (0..self.scene.objects.len())
            .map(|index| self.distance_outside_cell(index, resources.library))
            .collect();
        self.show_outliner(context, resources);
        self.show_properties(context, resources);
        self.palette.show(context, self.id, resources);
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(context, |ui| self.show_viewport(ui, resources));
    }

    /// Handles editor keyboard shortcuts in `context`.
    pub fn handle_shortcuts(&mut self, context: &egui::Context, library: &mut Arena2Library) {
        use egui::{Key, KeyboardShortcut, Modifiers};

        let shortcut = |modifiers, key| {
            context.input_mut(|input| input.consume_shortcut(&KeyboardShortcut::new(modifiers, key)))
        };
        if shortcut(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || shortcut(Modifiers::COMMAND, Key::Y) {
            self.redo();
        }
        if shortcut(Modifiers::COMMAND, Key::Z) {
            self.undo();
        }
        if shortcut(Modifiers::COMMAND, Key::D) {
            self.duplicate_selection();
        }

        // Single key shortcuts must not fire while the user is typing in a text field.
        if context.wants_keyboard_input() {
            return;
        }
        // Ctrl+C and Ctrl+V arrive as copy and paste events, not as keys.
        let (copy, pasted) = context.input(|input| {
            let mut copy = false;
            let mut pasted = None;
            for event in &input.events {
                match event {
                    egui::Event::Copy => copy = true,
                    egui::Event::Paste(text) => pasted = Some(text.clone()),
                    _ => {}
                }
            }
            (copy, pasted)
        });
        if copy {
            self.copy_selection(context);
        }
        if let Some(text) = pasted {
            let ours = context
                .data(|data| data.get_temp::<CopiedObjects>(copied_objects_id()))
                .is_some_and(|copied| copied.marker == text);
            if ours {
                self.paste(context);
            }
        }
        if shortcut(Modifiers::NONE, Key::Delete) {
            self.delete_selection();
        }
        if shortcut(Modifiers::NONE, Key::F) {
            self.focus_selection(library);
        }
        if shortcut(Modifiers::NONE, Key::Home) {
            self.frame_room(library);
        }
        if shortcut(Modifiers::NONE, Key::G) {
            self.toggle_grid_lock();
        }

        // While an asset is armed for placement, Escape and R act on it instead of the selection.
        let is_placing = self.palette.armed.is_some();
        if shortcut(Modifiers::NONE, Key::Escape) {
            if self.viewport.tying_from.is_some() {
                self.cancel_tying();
            } else if is_placing {
                self.palette.armed = None;
            } else {
                self.selected_object = None;
            }
        }
        // R (Shift+R the other way), and Q and E, turn the object being placed or
        // the one selected object by a quarter turn. Q and E are left to the
        // camera while the right mouse button is held (flying).
        let flying = context.input(|input| input.pointer.secondary_down());
        let mut turns = vec![(Modifiers::SHIFT, Key::R, -90.0), (Modifiers::NONE, Key::R, 90.0)];
        if !flying {
            turns.extend([(Modifiers::NONE, Key::Q, -90.0), (Modifiers::NONE, Key::E, 90.0)]);
        }
        for (modifiers, key, degrees) in turns {
            if shortcut(modifiers, key) {
                if is_placing {
                    self.palette.rotate_armed(degrees);
                } else {
                    self.rotate_selection(degrees);
                }
            }
        }
    }

    /// Adds the Edit menu entries.
    pub fn edit_menu(&mut self, ui: &mut egui::Ui) {
        let has_selection = self.selected_object.is_some();
        // Several objects can be copied, duplicated, deleted and moved, but only turned as prefabs.
        let has_one = has_selection && self.can_turn_selection();
        let has_copied = ui.ctx().data(|data| data.get_temp::<CopiedObjects>(copied_objects_id())).is_some();
        let menu_button = |ui: &mut egui::Ui, enabled: bool, label: &str, shortcut: &str| {
            let clicked = ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(shortcut)).clicked();
            if clicked {
                ui.close_menu();
            }
            clicked
        };

        if menu_button(ui, self.history.can_undo(), "Undo", "Ctrl+Z") {
            self.undo();
        }
        if menu_button(ui, self.history.can_redo(), "Redo", "Ctrl+Y") {
            self.redo();
        }
        ui.separator();
        if menu_button(ui, has_selection, "Copy", "Ctrl+C") {
            self.copy_selection(ui.ctx());
        }
        if menu_button(ui, has_copied, "Paste", "Ctrl+V") {
            self.paste(ui.ctx());
        }
        if menu_button(ui, has_selection, "Duplicate", "Ctrl+D") {
            self.duplicate_selection();
        }
        if menu_button(ui, has_one, "Rotate 90°", "R") {
            self.rotate_selection(90.0);
        }
        if menu_button(ui, has_selection, "Delete", "Del") {
            self.delete_selection();
        }
        ui.separator();
        let grid_lock = egui::Checkbox::new(&mut self.snap.enabled, "Grid lock");
        ui.add(grid_lock).on_hover_text("Snap objects to the grid while moving and placing them (G)");
        ui.horizontal(|ui| {
            ui.label("Snap step");
            snap_step_selector(ui, self.id.with("menu_snap_step"), &mut self.snap.step);
        });
    }

    /// Adds the grid lock button and its snap step to a menu bar.
    pub fn toolbar(&mut self, ui: &mut egui::Ui) {
        let label = if self.snap.enabled { "Grid lock: on" } else { "Grid lock: off" };
        let response = ui.selectable_label(self.snap.enabled, label).on_hover_text(
            "Snap objects to the grid while moving and placing them (G).\nHold Ctrl while dragging to switch it for that drag.",
        );
        if response.clicked() {
            self.toggle_grid_lock();
        }
        snap_step_selector(ui, self.id.with("toolbar_snap_step"), &mut self.snap.step);
    }

    /// Turns grid lock on or off and says so in the status bar.
    fn toggle_grid_lock(&mut self) {
        self.snap.enabled = !self.snap.enabled;
        self.status_message = if self.snap.enabled {
            format!("Grid lock on: objects snap to a {} m grid", self.snap.step)
        } else {
            "Grid lock off".to_string()
        };
    }

    /// Adds the View menu entries.
    pub fn view_menu(&mut self, ui: &mut egui::Ui, library: &mut Arena2Library) {
        if ui.add(egui::Button::new("Frame cell").shortcut_text("Home")).clicked() {
            self.frame_room(library);
            ui.close_menu();
        }
        let has_selection = self.selected_object.is_some();
        if ui.add_enabled(has_selection, egui::Button::new("Frame selection").shortcut_text("F")).clicked() {
            self.focus_selection(library);
            ui.close_menu();
        }
        ui.separator();
        ui.checkbox(&mut self.palette.is_open, "Asset palette");
        ui.separator();
        view_settings_menu(ui, &mut self.view_settings);
    }

    /// Frames the whole room.
    fn frame_room(&mut self, library: &mut Arena2Library) {
        match self.scene.bounds(library) {
            Some((minimum, maximum)) => self.camera.frame_bounds(minimum, maximum),
            None => self
                .camera
                .frame_bounds(Vec3::new(0.0, -5.0, 0.0), Vec3::new(RDB_BLOCK_SIDE, 5.0, RDB_BLOCK_SIDE)),
        }
    }

    /// Centres the camera on the selected object, or frames all of them when several are.
    fn focus_selection(&mut self, library: &mut Arena2Library) {
        let selected = self.selected_indices();
        if selected.len() > 1 {
            if let Some((minimum, maximum)) = self.scene.bounds_of(&selected, library) {
                self.camera.frame_bounds(minimum, maximum);
                return;
            }
        }
        let Some(object) = self.selected_object.and_then(|index| self.scene.objects.get(index)) else {
            return;
        };
        let radius = match object.kind {
            SceneObjectKind::Model { model_id, .. } => {
                library.model(model_id).map_or(DEFAULT_FOCUS_RADIUS, |mesh| mesh.bounding_radius())
            }
            _ => DEFAULT_FOCUS_RADIUS,
        };
        let center = object.position;
        self.camera.frame_bounds(center - Vec3::splat(radius), center + Vec3::splat(radius));
    }

    /// Every selected object, the last selected last. Empty if none is. The
    /// objects of a placed prefab are selected together, so each selected
    /// one brings the rest of its prefab.
    pub(super) fn selected_indices(&self) -> Vec<usize> {
        let count = self.scene.objects.len();
        let chosen = match self.selected_object.filter(|&index| index < count) {
            Some(primary) if self.selection_set.contains(&primary) => {
                self.selection_set.iter().copied().filter(|&index| index < count).collect()
            }
            Some(primary) => vec![primary],
            None => Vec::new(),
        };
        self.with_whole_prefabs(chosen)
    }

    /// `indices` with the rest of each placed prefab among them; the last one stays last.
    fn with_whole_prefabs(&self, indices: Vec<usize>) -> Vec<usize> {
        let in_prefab =
            |index: &usize| self.scene.objects.get(*index).is_some_and(|object| object.prefab.is_some());
        if !indices.iter().any(in_prefab) {
            return indices;
        }
        let mut whole = Vec::with_capacity(indices.len());
        for &index in &indices {
            for member in self.scene.prefab_of(index) {
                if !whole.contains(&member) {
                    whole.push(member);
                }
            }
        }
        // The one chosen last leads a move (it lands on the snap grid), so it stays last.
        if let Some(&last) = indices.last() {
            whole.retain(|&index| index != last);
            whole.push(last);
        }
        whole
    }

    /// Whether more than one object is selected; then they can only be moved.
    pub(super) fn has_multiple_selection(&self) -> bool {
        self.selected_indices().len() > 1
    }

    /// Selects exactly `indices` (the last one becomes the main selection).
    pub(super) fn select_objects(&mut self, mut indices: Vec<usize>) {
        let mut seen = std::collections::HashSet::new();
        indices.retain(|index| seen.insert(*index));
        self.selected_object = indices.last().copied();
        self.selection_set = if indices.len() > 1 { indices } else { Vec::new() };
    }

    /// Adds object `index` to the selection, or takes it out if it is selected
    /// (Ctrl+click, or Shift+click in the viewport); with it, the rest of the
    /// prefab it is part of.
    pub(super) fn toggle_selected(&mut self, index: usize) {
        let mut indices = self.selected_indices();
        let prefab = self.scene.prefab_of(index);
        if indices.contains(&index) {
            indices.retain(|selected| !prefab.contains(selected));
        } else {
            indices.extend(prefab.into_iter().filter(|&member| member != index));
            indices.push(index);
        }
        self.select_objects(indices);
    }

    /// Shows the Custom Models tab of the asset palette, for example once a model was imported.
    pub fn show_custom_models(&mut self) {
        self.palette.show_custom_models();
    }

    /// What the user asked to do with one of their imported models in the asset palette, once.
    pub fn take_custom_model_request(&mut self) -> Option<asset_palette::CustomModelRequest> {
        self.palette.take_model_request()
    }

    /// Stops placing the imported model `model_id`, which was deleted.
    pub fn forget_custom_model(&mut self, model_id: u32) {
        self.palette.forget_model(model_id);
    }

    /// Makes object `index` the one Shift+click in the outliner selects from.
    pub(super) fn set_selection_anchor(&mut self, index: usize) {
        self.selection_anchor = self.scene.objects.get(index).map(|object| object.id);
    }

    /// The outliner's headings in order, each with its objects in order: the
    /// object lists the user made, oldest first, then the quarters of the cell
    /// with the objects in no list.
    pub(super) fn outliner_sections(&self) -> Vec<(OutlinerSection, Vec<usize>)> {
        let mut sections: Vec<(OutlinerSection, Vec<usize>)> = self
            .scene
            .folders()
            .into_iter()
            .map(|folder| {
                let members = self.scene.folder_members(folder.id);
                (OutlinerSection::Folder(folder), members)
            })
            .collect();
        let unlisted: Vec<usize> = (0..self.scene.objects.len())
            .filter(|&index| self.scene.objects[index].folder.is_none())
            .collect();
        let mut groups: Vec<usize> =
            unlisted.iter().map(|&index| self.scene.objects[index].group_index).collect();
        groups.sort_unstable();
        groups.dedup();
        for group_index in groups {
            let rows = unlisted
                .iter()
                .copied()
                .filter(|&index| self.scene.objects[index].group_index == group_index)
                .collect();
            sections.push((OutlinerSection::Quarter(group_index), rows));
        }
        sections
    }

    /// Handles the row clicked in the outliner this frame, once every row is
    /// drawn: a click selects the row; Ctrl+click adds it to the selection or
    /// takes it out; Shift+click selects every row shown from the one clicked
    /// last without Shift to this one (Ctrl+Shift+click adds them).
    pub(super) fn apply_outliner_click(&mut self) -> bool {
        let Some((index, modifiers)) = self.outliner_click.take() else { return false };
        if index >= self.scene.objects.len() {
            return false;
        }
        if !modifiers.shift {
            match modifiers.command {
                true => self.toggle_selected(index),
                false => self.select_objects(vec![index]),
            }
            self.set_selection_anchor(index);
            return true;
        }
        let anchor = self
            .selection_anchor
            .and_then(|id| self.scene.objects.iter().position(|object| object.id == id))
            .or(self.selected_object)
            .filter(|&anchor| anchor < self.scene.objects.len());
        let order: Vec<usize> = self.outliner_sections().into_iter().flat_map(|(_, rows)| rows).collect();
        let position = |row: usize| order.iter().position(|&other| other == row);
        let (Some(anchor), Some(to)) = (anchor, position(index)) else {
            self.select_objects(vec![index]);
            self.set_selection_anchor(index);
            return true;
        };
        let from = position(anchor).unwrap_or(to);
        let mut range: Vec<usize> = order[from.min(to)..=from.max(to)]
            .iter()
            .copied()
            .filter(|row| self.outliner_shown.contains(row) || *row == anchor)
            .collect();
        // The row clicked becomes the main selection.
        range.retain(|&row| row != index);
        range.push(index);
        if modifiers.command {
            let mut selection = self.selected_indices();
            selection.retain(|row| !range.contains(row));
            selection.extend(range);
            range = selection;
        }
        self.select_objects(range);
        true
    }

    /// Says in the status bar that this cannot be done with several objects
    /// selected (they can be moved, copied, duplicated and deleted, not turned),
    /// and returns `true` if that is the case.
    fn refuse_for_multiple_selection(&mut self, action: &str) -> bool {
        let count = self.selected_indices().len();
        if count > 1 {
            self.status_message = format!(
                "Cannot {action} {count} objects at once; several objects can be moved, copied, duplicated \
                 or deleted"
            );
        }
        count > 1
    }

    /// The selected objects, as many as there are.
    fn selected_objects(&self) -> Vec<SceneObject> {
        self.selected_indices()
            .into_iter()
            .filter_map(|index| self.scene.objects.get(index).cloned())
            .collect()
    }

    /// A short description of `count` objects, the only one by name.
    fn count_description(objects: &[SceneObject]) -> String {
        match objects {
            [object] => object.name.clone(),
            several => format!("{} objects", several.len()),
        }
    }

    /// Whether the selection can be turned: one object, or whole placed prefabs.
    pub(super) fn can_turn_selection(&self) -> bool {
        let selected = self.selected_indices();
        selected.len() == 1 || self.is_whole_prefabs(&selected)
    }

    /// Whether `indices` are several objects, all of placed prefabs (the
    /// selection only ever holds whole prefabs).
    fn is_whole_prefabs(&self, indices: &[usize]) -> bool {
        indices.len() > 1
            && indices
                .iter()
                .all(|&index| self.scene.objects.get(index).is_some_and(|object| object.prefab.is_some()))
    }

    /// Removes the selected objects, as one undo step.
    fn delete_selection(&mut self) {
        let mut indices = self.selected_indices();
        if indices.is_empty() || !self.can_edit() {
            return;
        }
        indices.sort_unstable();
        indices.dedup();
        let mut removed = Vec::with_capacity(indices.len());
        for &index in indices.iter().rev() {
            removed.push((index, self.scene.objects.remove(index)));
        }
        removed.reverse();
        let objects: Vec<SceneObject> = removed.iter().map(|(_, object)| object.clone()).collect();
        self.status_message = format!("Deleted {}", Self::count_description(&objects));
        // Undoing puts them back from the first place up, so each goes where it was.
        self.history.record(SceneEdit::Group(
            removed.into_iter().map(|(index, object)| SceneEdit::Insert { index, object }).collect(),
        ));
        self.selected_object = None;
        self.selection_set.clear();
    }

    /// Copies the selected objects next to them, and selects the copies.
    fn duplicate_selection(&mut self) {
        let originals = self.selected_objects();
        if originals.is_empty() || !self.can_edit() {
            return;
        }
        self.insert_copies(&originals, DUPLICATE_OFFSET, true);
        self.status_message = format!("Duplicated {}", Self::count_description(&originals));
    }

    /// Copies the selected objects (Ctrl+C), to paste them into this cell or
    /// another with Ctrl+V.
    fn copy_selection(&mut self, context: &egui::Context) {
        let objects = self.selected_objects();
        if objects.is_empty() {
            self.status_message = "Select objects to copy them".to_string();
            return;
        }
        let description = Self::count_description(&objects);
        // A different text each time, so an older copy on the clipboard is not mistaken for this one.
        let marker = format!(
            "Daggerfall Dungeon Maker: {description} copied ({:.3})",
            context.input(|input| input.time)
        );
        context.copy_text(marker.clone());
        context.data_mut(|data| {
            data.insert_temp(
                copied_objects_id(),
                CopiedObjects { objects, source: self.instance, marker, pasted_into: None, pastes: 0 },
            )
        });
        self.status_message = format!("Copied {description}; Ctrl+V pastes it into this cell or another");
    }

    /// Pastes the objects copied last (Ctrl+V), and selects them. Pasted
    /// into the cell they were copied from, they land next to the originals;
    /// into another cell, where they were (each further paste a little further along).
    fn paste(&mut self, context: &egui::Context) {
        let Some(mut copied) = context.data(|data| data.get_temp::<CopiedObjects>(copied_objects_id()))
        else {
            self.status_message = "Nothing is copied: select objects and press Ctrl+C first".to_string();
            return;
        };
        if !self.can_edit() {
            return;
        }
        let same_cell = copied.source == self.instance;
        let pastes = match copied.pasted_into == Some(self.instance) {
            true => copied.pastes + 1,
            false => u32::from(same_cell),
        };
        self.insert_copies(&copied.objects, DUPLICATE_OFFSET * pastes as f32, same_cell);
        self.status_message = format!("Pasted {}", Self::count_description(&copied.objects));
        copied.pasted_into = Some(self.instance);
        copied.pastes = pastes;
        context.data_mut(|data| data.insert_temp(copied_objects_id(), copied));
    }

    /// Adds copies of `originals`, moved by `offset`, with new ids, as one
    /// undo step, and selects them. Links between the copies follow them;
    /// links to other objects are kept only in the cell they came from
    /// (`same_cell`), where those objects are. Copies are listed by the quarter
    /// they land in; in another cell, they join the object list of the same
    /// name as the one they were in, or a new one.
    fn insert_copies(&mut self, originals: &[SceneObject], offset: Vec3, same_cell: bool) {
        let mut next_id = self.scene.new_object_id();
        let ids: std::collections::HashMap<ObjectId, ObjectId> = originals
            .iter()
            .map(|object| {
                let id = next_id;
                next_id += 1;
                (object.id, id)
            })
            .collect();
        // Copied prefabs are placed prefabs of their own, apart from the originals.
        let mut next_prefab = self.scene.new_prefab_instance();
        let mut prefabs = std::collections::HashMap::new();
        // In another cell, copies join the object list of the same name, or a new one.
        let known_folders = self.scene.folders();
        let mut next_folder = self.scene.new_folder_id();
        let mut folders: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        let start = self.scene.objects.len();
        for original in originals {
            let mut copy = original.clone();
            copy.id = ids[&original.id];
            copy.next = original.next.and_then(|next| ids.get(&next).copied().or(same_cell.then_some(next)));
            copy.position += offset;
            copy.group_index = quarter_of(copy.position);
            if let (false, Some(folder)) = (same_cell, copy.folder.as_mut()) {
                let id = match folders.get(&folder.id) {
                    Some(&id) => id,
                    None => {
                        let same_name = known_folders
                            .iter()
                            .find(|known| known.name.trim().eq_ignore_ascii_case(folder.name.trim()));
                        let id = same_name.map(|known| known.id).unwrap_or_else(|| {
                            next_folder += 1;
                            next_folder - 1
                        });
                        folders.insert(folder.id, id);
                        id
                    }
                };
                folder.id = id;
            }
            if let Some(member) = copy.prefab.as_mut() {
                member.instance = *prefabs.entry(member.instance).or_insert_with(|| {
                    next_prefab += 1;
                    next_prefab - 1
                });
            }
            self.scene.objects.push(copy);
        }
        let end = self.scene.objects.len();
        // Undoing removes them from the last one back.
        self.history
            .record(SceneEdit::Group((start..end).rev().map(|index| SceneEdit::Remove { index }).collect()));
        self.select_objects((start..end).collect());
        self.viewport.selection_changed_in_viewport = true;
    }

    /// Turns the selected object around the vertical axis by `degrees`, or
    /// selected prefabs around the middle of their objects.
    fn rotate_selection(&mut self, degrees: f32) {
        let selected = self.selected_indices();
        if self.is_whole_prefabs(&selected) {
            if self.can_edit() {
                self.turn_objects(&selected, degrees);
                self.status_message = match self.selected_prefab() {
                    Some((member, _)) => format!("Turned the prefab {} by {degrees:.0}°", member.name),
                    None => format!("Turned {} objects by {degrees:.0}°", selected.len()),
                };
            }
            return;
        }
        if self.refuse_for_multiple_selection("rotate") {
            return;
        }
        let Some(index) = self.selected_object.filter(|&index| index < self.scene.objects.len()) else {
            return;
        };
        if !self.can_edit() {
            return;
        }
        let Some(object) = self.scene.objects.get_mut(index) else { return };
        self.history.record(SceneEdit::Replace { index, object: object.clone() });
        object.rotation_degrees.y = (object.rotation_degrees.y + degrees).rem_euclid(360.0);
    }

    /// Turns the objects `indices` together around the vertical line through
    /// their middle by `degrees`, as one undo step. Models turn with them;
    /// flats face the camera and lights have no direction.
    fn turn_objects(&mut self, indices: &[usize], degrees: f32) {
        let objects: Vec<&SceneObject> =
            indices.iter().filter_map(|&index| self.scene.objects.get(index)).collect();
        if objects.is_empty() {
            return;
        }
        let middle = objects.iter().map(|object| object.position).sum::<Vec3>() / objects.len() as f32;
        let turn = glam::Quat::from_rotation_y(degrees.to_radians());
        let reverts = indices
            .iter()
            .map(|&index| SceneEdit::Replace { index, object: self.scene.objects[index].clone() })
            .collect();
        for &index in indices {
            let object = &mut self.scene.objects[index];
            let from_middle = object.position - middle;
            object.position = middle + turn * from_middle;
            object.group_index = quarter_of(object.position);
            if matches!(object.kind, SceneObjectKind::Model { .. }) {
                object.set_rotation(turn * object.rotation());
            }
        }
        self.history.record(SceneEdit::Group(reverts));
    }

    /// Reverts the last edit.
    fn undo(&mut self) {
        if self.history.undo(&mut self.scene.objects) {
            self.after_history_change();
            self.status_message = "Undo".into();
        }
    }

    /// Re-applies the last undone edit.
    fn redo(&mut self) {
        if self.history.redo(&mut self.scene.objects) {
            self.after_history_change();
            self.status_message = "Redo".into();
        }
    }

    /// Keeps the selection valid after the object list changed underneath it.
    fn after_history_change(&mut self) {
        self.property_edit_session = None;
        if self.selected_object.is_some_and(|index| index >= self.scene.objects.len()) {
            self.selected_object = None;
        }
        let count = self.scene.objects.len();
        self.selection_set.retain(|&index| index < count);
    }
}

/// How far past a box an object's middle may be and still count as inside it, in world units.
const BOX_TOLERANCE: f32 = 0.01;

/// The middle of `object` in the cell: the middle of a model's box, or where a flat or light is.
pub(super) fn object_middle(object: &SceneObject, library: &mut Arena2Library) -> Vec3 {
    match object.kind {
        SceneObjectKind::Model { model_id, .. } => library
            .model(model_id)
            .map_or(object.position, |mesh| object.model_matrix().transform_point3(mesh.bounds_center())),
        _ => object.position,
    }
}

/// The name an object is given when its name is cleared: a model's default
/// name (so it is listed by its model's name), a flat's description, or "Light".
fn default_object_name(object: &SceneObject) -> String {
    match object.kind {
        SceneObjectKind::Model { model_id, .. } => crate::scene::default_model_object_name(model_id),
        SceneObjectKind::Flat { texture, .. } => crate::catalog::flat_description(texture),
        SceneObjectKind::Light { .. } => "Light".to_string(),
    }
}

/// How far `object` reaches outside its cell's square, using its model's box.
pub(super) fn object_distance_outside_cell(object: &SceneObject, library: &mut Arena2Library) -> f32 {
    let bounds = match object.kind {
        SceneObjectKind::Model { model_id, .. } => {
            library.model(model_id).map(|mesh| (mesh.bounds_min, mesh.bounds_max))
        }
        _ => None,
    };
    object.distance_outside_cell(bounds)
}

/// Tells the user how far an object reaches outside the cell's square.
pub(super) fn outside_cell_warning(distance: f32) -> String {
    format!(
        "⚠ Reaches {distance:.2} m outside the cell's {RDB_BLOCK_SIDE} m square, so in a dungeon \
         it can overlap the next cell"
    )
}

/// Draws a drop-down choosing the grid lock snap step from [`SNAP_STEPS`].
fn snap_step_selector(ui: &mut egui::Ui, id: egui::Id, step: &mut f32) {
    let selected = SNAP_STEPS
        .iter()
        .find(|(value, _)| (*value - *step).abs() < 1e-4)
        .map_or_else(|| format!("{step} m"), |(_, label)| (*label).to_string());
    egui::ComboBox::from_id_salt(id).selected_text(selected).show_ui(ui, |ui| {
        for (value, label) in SNAP_STEPS {
            ui.selectable_value(step, value, label);
        }
    });
}

/// Adds checkboxes for every [`ViewSettings`] option.
pub fn view_settings_menu(ui: &mut egui::Ui, settings: &mut ViewSettings) {
    ui.checkbox(&mut settings.cull_back_faces, "Hide back faces (see into cells)");
    ui.checkbox(&mut settings.show_grid, "Grid");
    ui.checkbox(&mut settings.show_room_outline, "Block outlines");
    ui.separator();
    ui.checkbox(&mut settings.show_models, "Models");
    ui.checkbox(&mut settings.show_flats, "Flats");
    ui.checkbox(&mut settings.show_markers, "Editor markers");
    ui.checkbox(&mut settings.show_lights, "Lights");
    ui.checkbox(&mut settings.show_action_links, "Action links")
        .on_hover_text("Red lines from each object to the object its action triggers");
    ui.checkbox(&mut settings.show_switched_off_doors, "Switched-off doors").on_hover_text(
        "Tints red the doors Daggerfall Unity switches off because they are within 1.4 m of another door",
    );
}
