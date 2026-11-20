//! The project panel: every room and dungeon that can be opened.
//!
//! A toggle at the very top chooses between **Dungeons** and **Settlements**.
//! For dungeons, a switch below shows either **Dungeons** or **Cells**
//! (rooms), and a toggle shows or hides Daggerfall's own. For settlements, it
//! shows **Settlements** (every location of the world: towns, homes,
//! temples, dungeon entrances…, by region and type), **Cells** (the outdoor
//! cells, RMB blocks, they are built from, by kind) or **Interiors** (the
//! insides of the cells' buildings, by type and cell kind); these are
//! Daggerfall's own and open read-only. The **Daggerfall** list holds the
//! game's own dungeons (grouped by region) or rooms; they open read-only.
//! Below it are the project's own: loose at the top level, or in folders the
//! user creates (folders show in both, with what they hold of each kind).
//! Right-clicking an entry offers what can be done with it.
//!
//! Entries can also be dragged: onto a folder to move them into it, onto an
//! entry to place them just before it, or onto the space below the list to move
//! them to the top level. Dragging a Daggerfall room or dungeon into the project
//! adds a copy there. A cell (the project's or Daggerfall's) can also be dragged
//! into the dungeon view, which adds it to the open dungeon.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui;
use serde::{Deserialize, Serialize};

use super::interior_view::InteriorKey;
use super::room_browser::category_description;
use crate::arena2::{
    building_type_name, rmb_block_kind, DungeonDefinition, LocationDefinition, LocationType, RMB_BLOCK_KINDS,
};
use crate::custom_names::CustomNames;
use crate::project::{ItemId, Project, ProjectEntry};

/// Most search matches listed per section, so a short search stays responsive.
const MAX_SEARCH_MATCHES: usize = 200;
/// Name given to new folders.
const NEW_FOLDER_NAME: &str = "New folder";

/// What the main window is showing, so the panel can highlight it.
#[derive(Debug, Clone, PartialEq)]
pub enum OpenedItem {
    /// A room from Daggerfall, by RDB block name.
    DaggerfallRoom(String),
    /// A dungeon from Daggerfall, by index in the library's dungeon list.
    DaggerfallDungeon(usize),
    /// A settlement (any location of the world) from Daggerfall, by index in
    /// the library's location list.
    DaggerfallSettlement(usize),
    /// An outdoor cell from Daggerfall, by RMB block name.
    DaggerfallOutdoorCell(String),
    /// The inside of a building of one of Daggerfall's outdoor cells.
    DaggerfallInterior(InteriorKey),
    /// A room or dungeon of the project.
    Project(ItemId),
}

/// Something the user asked for that the application carries out.
#[derive(Debug, Clone, PartialEq)]
pub enum PanelAction {
    Open(OpenedItem),
    /// Copy a Daggerfall room or dungeon into the project.
    CopyFromDaggerfall {
        item: OpenedItem,
        /// Folder to put the copy in; `None` for the top level.
        folder: Option<ItemId>,
        /// Open the copy (or, if the item is open, turn it into the copy) rather than just adding it.
        open: bool,
    },
    Duplicate(ItemId),
    /// A new, empty room or dungeon was added to the project; open it.
    Created(ItemId),
    /// Delete a room or dungeon (the application asks first).
    Delete(ItemId),
}

/// What is being dragged from the panel.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum DragPayload {
    /// A project room, dungeon or folder.
    Project(ItemId),
    /// A Daggerfall room or dungeon, which is copied where it is dropped.
    Daggerfall(OpenedItem),
}

/// Where a dragged entry was dropped.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DropTarget {
    /// Into a folder, or the top level for `None`.
    Into(Option<ItemId>),
    /// Just before an entry, in the same folder.
    Before(ItemId),
}

/// A change to the project tree, applied after the tree has been drawn.
enum TreeChange {
    Rename(ItemId, String),
    Move(ItemId, Option<ItemId>),
    Drop(DragPayload, DropTarget),
    NewFolder(Option<ItemId>),
    RemoveFolder(ItemId),
}

/// A region's dungeons: a range of the (region-sorted) dungeon list.
struct RegionGroup {
    name: &'static str,
    dungeons: std::ops::Range<usize>,
}

/// A region's locations, by type.
struct LocationRegion {
    name: &'static str,
    count: usize,
    /// Each type's name (plural) and its locations, as indices into the location list.
    kinds: Vec<(&'static str, Vec<usize>)>,
}

/// The buildings of one type, by cell kind: the type's name, and each kind's
/// name with its buildings (as indices into the panel's list of interiors).
type InteriorGroup = (String, Vec<(&'static str, Vec<usize>)>);

/// Location types in the order the panel lists them, with their plural names.
const LOCATION_KINDS: [(Option<LocationType>, &str); 16] = [
    (Some(LocationType::City), "Cities"),
    (Some(LocationType::Hamlet), "Hamlets"),
    (Some(LocationType::Village), "Villages"),
    (Some(LocationType::Tavern), "Taverns"),
    (Some(LocationType::Temple), "Temples"),
    (Some(LocationType::Cult), "Cults"),
    (Some(LocationType::Coven), "Covens"),
    (Some(LocationType::WealthyHome), "Wealthy homes"),
    (Some(LocationType::PoorHome), "Poor homes"),
    (Some(LocationType::Farm), "Farms"),
    (Some(LocationType::LargeDungeon), "Large dungeons"),
    (Some(LocationType::MediumDungeon), "Medium dungeons"),
    (Some(LocationType::SmallDungeon), "Small dungeons"),
    (Some(LocationType::Graveyard), "Graveyards"),
    (Some(LocationType::Ship), "Ships"),
    (None, "Other"),
];

/// Which kind of item the panel lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum PanelSection {
    #[default]
    Dungeons,
    Cells,
    /// Every location of the world (Settlements on the panel's top toggle).
    Settlements,
    /// The outdoor cells (RMB blocks) locations are built from.
    OutdoorCells,
    /// The insides of the outdoor cells' buildings.
    Interiors,
}

impl PanelSection {
    /// Whether the section is one of the settlements' (rather than the dungeons').
    pub fn is_outdoor(self) -> bool {
        matches!(self, Self::Settlements | Self::OutdoorCells | Self::Interiors)
    }

    /// The section listing `entry`, or `None` for folders, which show in both.
    fn of(entry: &ProjectEntry) -> Option<Self> {
        match entry {
            ProjectEntry::Room(_) => Some(Self::Cells),
            ProjectEntry::Dungeon(_) => Some(Self::Dungeons),
            ProjectEntry::Folder(_) => None,
        }
    }
}

/// What the panel shows, remembered between runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelView {
    pub section: PanelSection,
    /// Whether Daggerfall's own dungeons or cells are listed above the project's.
    pub show_daggerfall: bool,
}

impl Default for PanelView {
    fn default() -> Self {
        Self { section: PanelSection::default(), show_daggerfall: true }
    }
}

/// State of the project panel.
pub struct ProjectPanel {
    /// Whether the panel is shown.
    pub is_open: bool,
    /// The section shown and whether Daggerfall's items are listed.
    pub view: PanelView,
    /// The item open when the panel was last drawn, to switch to its section when another opens.
    last_opened: Option<OpenedItem>,
    search_text: String,
    /// Entry being renamed and the name typed so far.
    renaming: Option<(ItemId, String)>,
    /// Set when renaming starts, to put the cursor in the name field.
    focus_rename_field: bool,
    room_names: Vec<String>,
    dungeons: Arc<[DungeonDefinition]>,
    regions: Vec<RegionGroup>,
    /// Every location of the world, and the same grouped by region and type.
    locations: Arc<[LocationDefinition]>,
    location_regions: Vec<LocationRegion>,
    /// Every outdoor cell (RMB block name), and the same grouped by kind (as indices into it).
    outdoor_cells: Vec<String>,
    outdoor_kinds: Vec<(&'static str, Vec<usize>)>,
    /// The locations using each outdoor cell, as indices into `locations`.
    outdoor_usage: Arc<HashMap<String, Vec<usize>>>,
    /// Every building with an inside, with its type.
    interiors: Vec<(InteriorKey, u8)>,
    /// The same by building type (by name), then by cell kind, as indices into `interiors`.
    interior_groups: Vec<InteriorGroup>,
}

impl Default for ProjectPanel {
    fn default() -> Self {
        Self {
            is_open: true,
            view: PanelView::default(),
            last_opened: None,
            search_text: String::new(),
            renaming: None,
            focus_rename_field: false,
            room_names: Vec::new(),
            dungeons: Arc::new([]),
            regions: Vec::new(),
            locations: Arc::new([]),
            location_regions: Vec::new(),
            outdoor_cells: Vec::new(),
            outdoor_kinds: Vec::new(),
            outdoor_usage: Arc::default(),
            interiors: Vec::new(),
            interior_groups: Vec::new(),
        }
    }
}

impl ProjectPanel {
    /// Sets the Daggerfall rooms and dungeons listed, after the game data is loaded.
    ///
    /// `dungeons` must be sorted by region, as the library returns them.
    pub fn set_daggerfall_contents(&mut self, room_names: Vec<String>, dungeons: Arc<[DungeonDefinition]>) {
        self.regions.clear();
        for (index, dungeon) in dungeons.iter().enumerate() {
            match self.regions.last_mut() {
                Some(region) if region.name == dungeon.region_name => region.dungeons.end = index + 1,
                _ => self.regions.push(RegionGroup { name: dungeon.region_name, dungeons: index..index + 1 }),
            }
        }
        self.room_names = room_names;
        self.dungeons = dungeons;
    }

    /// Sets the Daggerfall settlements and outdoor cells listed, after the game data is loaded.
    ///
    /// `locations` must be sorted by region, as the library returns them;
    /// `usage` gives the locations using each outdoor cell.
    pub fn set_outdoor_contents(
        &mut self,
        locations: Arc<[LocationDefinition]>,
        outdoor_cells: Vec<String>,
        usage: Arc<HashMap<String, Vec<usize>>>,
    ) {
        self.location_regions.clear();
        for (index, location) in locations.iter().enumerate() {
            if self.location_regions.last().is_none_or(|region| region.name != location.region_name) {
                let kinds = LOCATION_KINDS.iter().map(|&(_, name)| (name, Vec::new())).collect();
                self.location_regions.push(LocationRegion { name: location.region_name, count: 0, kinds });
            }
            let region = self.location_regions.last_mut().expect("a region was just added");
            let kind = LOCATION_KINDS
                .iter()
                .position(|&(kind, _)| kind == location.location_type)
                .unwrap_or(LOCATION_KINDS.len() - 1);
            region.kinds[kind].1.push(index);
            region.count += 1;
        }
        for region in &mut self.location_regions {
            region.kinds.retain(|(_, indices)| !indices.is_empty());
        }

        let mut kinds: Vec<(&'static str, Vec<usize>)> = Vec::new();
        for (index, name) in outdoor_cells.iter().enumerate() {
            let kind = rmb_block_kind(name);
            match kinds.iter_mut().find(|(known, _)| *known == kind) {
                Some((_, indices)) => indices.push(index),
                None => kinds.push((kind, vec![index])),
            }
        }
        // In the order the game numbers the kinds, then any others.
        let order =
            |kind: &str| RMB_BLOCK_KINDS.iter().position(|(_, known)| *known == kind).unwrap_or(usize::MAX);
        kinds.sort_by_key(|(kind, _)| order(kind));
        self.outdoor_kinds = kinds;
        self.outdoor_cells = outdoor_cells;
        self.locations = locations;
        self.outdoor_usage = usage;
    }

    /// Sets the buildings with an inside listed under Interiors, with their types.
    pub fn set_interiors(&mut self, interiors: Vec<(InteriorKey, u8)>) {
        let mut types: Vec<u8> = interiors.iter().map(|(_, building_type)| *building_type).collect();
        types.sort_unstable();
        types.dedup();
        self.interior_groups = types
            .into_iter()
            .map(|building_type| {
                let mut kinds: Vec<(&'static str, Vec<usize>)> = Vec::new();
                for (index, (key, _)) in
                    interiors.iter().enumerate().filter(|(_, (_, kind))| *kind == building_type)
                {
                    let kind = rmb_block_kind(&key.block);
                    match kinds.iter_mut().find(|(known, _)| *known == kind) {
                        Some((_, indices)) => indices.push(index),
                        None => kinds.push((kind, vec![index])),
                    }
                }
                (building_type_name(building_type), kinds)
            })
            .collect();
        self.interiors = interiors;
    }

    /// Why a new item of the section shown cannot be called `name`, if it cannot:
    /// the name is taken by something in the project, or by one of Daggerfall's
    /// own dungeons or cells of the same kind.
    fn new_item_problem(&self, project: &Project, name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        if project.has_item_named(name) {
            return Some(format!("Your project already has something called \"{name}\""));
        }
        let taken_in_daggerfall = match self.view.section {
            PanelSection::Dungeons => {
                self.dungeons.iter().any(|dungeon| dungeon.name.eq_ignore_ascii_case(name))
            }
            PanelSection::Cells => self.room_names.iter().any(|block| {
                block.eq_ignore_ascii_case(name) || block.trim_end_matches(".RDB").eq_ignore_ascii_case(name)
            }),
            PanelSection::Settlements | PanelSection::OutdoorCells | PanelSection::Interiors => false,
        };
        let kind = match self.view.section {
            PanelSection::Dungeons => "dungeon",
            PanelSection::Cells => "cell",
            PanelSection::Settlements => "settlement",
            PanelSection::OutdoorCells => "outdoor cell",
            PanelSection::Interiors => "interior",
        };
        taken_in_daggerfall.then(|| format!("Daggerfall already has a {kind} called \"{name}\""))
    }

    /// Starts renaming the entry `id`, for example a folder that was just created.
    pub fn start_renaming(&mut self, id: ItemId, current_name: &str) {
        self.renaming = Some((id, current_name.to_string()));
        self.focus_rename_field = true;
    }

    /// Draws the panel on the left of the main window. Returns what the user asked for, if anything.
    pub fn show(
        &mut self,
        context: &egui::Context,
        project: &mut Project,
        project_title: &str,
        custom_names: &CustomNames,
        opened: Option<&OpenedItem>,
    ) -> Option<PanelAction> {
        if !self.is_open {
            return None;
        }
        let mut action = None;
        let mut changes = Vec::new();

        // Opening something of the other kind (from a menu, a link or a cell
        // window) switches to its section, so it shows highlighted.
        if opened != self.last_opened.as_ref() {
            self.last_opened = opened.cloned();
            let section = match opened {
                Some(OpenedItem::DaggerfallDungeon(_)) => Some(PanelSection::Dungeons),
                Some(OpenedItem::DaggerfallRoom(_)) => Some(PanelSection::Cells),
                Some(OpenedItem::DaggerfallSettlement(_)) => Some(PanelSection::Settlements),
                Some(OpenedItem::DaggerfallOutdoorCell(_)) => Some(PanelSection::OutdoorCells),
                Some(OpenedItem::DaggerfallInterior(_)) => Some(PanelSection::Interiors),
                Some(OpenedItem::Project(id)) => project.find(*id).and_then(PanelSection::of),
                None => None,
            };
            if let Some(section) = section {
                self.view.section = section;
            }
        }

        egui::SidePanel::left("project_panel").resizable(true).default_width(250.0).show(context, |ui| {
            let outdoor = self.view.section.is_outdoor();
            ui.horizontal(|ui| {
                ui.heading(project_title);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !outdoor
                        && ui.button("New folder").on_hover_text("Add a folder for organising the project").clicked()
                    {
                        changes.push(TreeChange::NewFolder(None));
                    }
                });
            });
            self.show_mode_toggle(ui);
            if self.view.section.is_outdoor() {
                self.show_outdoor(ui, custom_names, opened, &mut action);
                return;
            }
            ui.horizontal(|ui| {
                for (section, name, hover) in [
                    (PanelSection::Dungeons, "Dungeons", "List dungeons"),
                    (PanelSection::Cells, "Cells", "List cells (the blocks dungeons are built from)"),
                ] {
                    let selected = self.view.section == section;
                    let text = egui::RichText::new(name).strong();
                    if ui.selectable_label(selected, text).on_hover_text(hover).clicked() {
                        self.view.section = section;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let hover = "Show or hide Daggerfall's own dungeons and cells above your project's";
                    toggle_switch(ui, &mut self.view.show_daggerfall).on_hover_text(hover);
                    ui.label("Daggerfall").on_hover_text(hover);
                });
            });
            let (hint, kind) = match self.view.section {
                PanelSection::Dungeons => ("Search, or name a new dungeon", "dungeon"),
                _ => ("Search, or name a new cell", "cell"),
            };
            ui.horizontal(|ui| {
                // The + button sits at the right; the search field takes the rest of the row.
                let button_width = ui.spacing().interact_size.y + ui.spacing().button_padding.x;
                let field_width = ui.available_width() - button_width - ui.spacing().item_spacing.x;
                // An exact size, so the field never widens the panel.
                let field = ui.add_sized(
                    [field_width.max(40.0), ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.search_text).hint_text(hint),
                );
                let name = self.search_text.trim().to_string();
                let problem = self.new_item_problem(project, &name);
                let can_create = !name.is_empty() && problem.is_none();
                let button = ui
                    .add_enabled(can_create, egui::Button::new(egui::RichText::new("+").strong()))
                    .on_hover_text(format!("Create an empty {kind} called \"{name}\" (or press Enter)"))
                    .on_disabled_hover_text(
                        problem.unwrap_or_else(|| format!("Type a name to create an empty {kind} with it")),
                    );
                let entered = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                if can_create && (button.clicked() || entered) {
                    let id = match self.view.section {
                        PanelSection::Dungeons => project.add_empty_dungeon(&name),
                        _ => project.add_empty_room(&name),
                    };
                    self.search_text.clear();
                    action = Some(PanelAction::Created(id));
                }
            });
            ui.separator();

            let search = self.search_text.trim().to_lowercase();
            let section = self.view.section;
            egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(("project_panel_list", section)).show(
                ui,
                |ui| {
                    if self.view.show_daggerfall {
                        self.show_daggerfall_list(ui, section, &search, custom_names, opened, &mut action);
                    }
                    let folders = project.folders();
                    let context = TreeContext { search: &search, section, opened, folders: &folders, project };
                    if self.view.show_daggerfall {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("Your project").strong());
                    }
                    for entry in folders_first(&project.entries) {
                        self.show_entry(ui, entry, &context, &mut action, &mut changes);
                    }
                    let has_any = project.entries.iter().any(|entry| entry_holds(entry, section));
                    if !has_any && search.is_empty() {
                        ui.add_space(6.0);
                        let hint = match section {
                            PanelSection::Dungeons => {
                                "Your dungeons appear here. Change one of Daggerfall's dungeons to make a copy."
                            }
                            _ => "Your cells appear here. Change one of Daggerfall's cells to make a copy.",
                        };
                        ui.label(egui::RichText::new(hint).weak());
                    }
                // Leave room below the last entry to right-click, or to drop entries at the top level.
                let rest = ui.allocate_response(
                    egui::vec2(ui.available_width(), ui.available_height().max(40.0)),
                    egui::Sense::click(),
                );
                drop_target(ui, &rest, DropTarget::Into(None), DropHighlight::Area, &mut changes);
                    rest.context_menu(|ui| {
                        if ui.button("New folder").clicked() {
                            changes.push(TreeChange::NewFolder(None));
                            ui.close_menu();
                        }
                    });
                },
            );
        });

        show_drag_preview(context, project, &self.dungeons);
        for change in changes {
            if let TreeChange::Drop(DragPayload::Daggerfall(item), target) = change {
                let folder = match target {
                    DropTarget::Into(folder) => folder,
                    DropTarget::Before(entry) => project.parent_of(entry).flatten(),
                };
                action = Some(PanelAction::CopyFromDaggerfall { item, folder, open: false });
                continue;
            }
            self.apply_change(project, change);
        }
        action
    }

    /// Carries out a change to the project tree.
    fn apply_change(&mut self, project: &mut Project, change: TreeChange) {
        match change {
            TreeChange::Rename(id, name) => {
                if let Some(entry) = project.find_mut(id) {
                    *entry.name_mut() = name;
                }
            }
            TreeChange::Move(id, parent)
            | TreeChange::Drop(DragPayload::Project(id), DropTarget::Into(parent)) => {
                project.move_entry(id, parent);
            }
            TreeChange::Drop(DragPayload::Project(id), DropTarget::Before(target)) => {
                project.move_before(id, target);
            }
            // Copies of Daggerfall items are made by the application, in `show`.
            TreeChange::Drop(DragPayload::Daggerfall(_), _) => {}
            TreeChange::NewFolder(parent) => {
                let id = project.add_folder(NEW_FOLDER_NAME, parent);
                self.start_renaming(id, NEW_FOLDER_NAME);
            }
            TreeChange::RemoveFolder(id) => {
                project.remove_folder_keeping_contents(id);
            }
        }
    }

    /// Draws the toggle at the top choosing between dungeons and settlements.
    fn show_mode_toggle(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).inner_margin(egui::Margin::symmetric(4.0, 2.0)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let outdoor = self.view.section.is_outdoor();
                let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                for (is_outdoor, name, hover) in [
                    (false, "Dungeons", "Dungeons and the cells (RDB blocks) they are built from"),
                    (
                        true,
                        "Settlements",
                        "Every location of the world (towns, homes, temples, dungeon entrances…) and the \
                         outdoor cells (RMB blocks) they are built from",
                    ),
                ] {
                    let text = egui::RichText::new(name).strong().size(15.0);
                    let button = egui::SelectableLabel::new(outdoor == is_outdoor, text);
                    if ui
                        .add_sized([width, ui.spacing().interact_size.y + 4.0], button)
                        .on_hover_text(hover)
                        .clicked()
                        && outdoor != is_outdoor
                    {
                        self.view.section =
                            if is_outdoor { PanelSection::Settlements } else { PanelSection::Dungeons };
                    }
                }
            });
        });
    }

    /// Draws the settlements' side of the panel: the Settlements and Cells
    /// sections, a search field and Daggerfall's settlements or outdoor cells.
    fn show_outdoor(
        &mut self,
        ui: &mut egui::Ui,
        custom_names: &CustomNames,
        opened: Option<&OpenedItem>,
        action: &mut Option<PanelAction>,
    ) {
        ui.horizontal(|ui| {
            for (section, name, hover) in [
                (
                    PanelSection::Settlements,
                    "Settlements",
                    "List every location of the world, by region and type",
                ),
                (
                    PanelSection::OutdoorCells,
                    "Cells",
                    "List the outdoor cells (RMB blocks) locations are built from",
                ),
                (
                    PanelSection::Interiors,
                    "Interiors",
                    "List the insides of the outdoor cells' buildings, by type and cell kind",
                ),
            ] {
                let selected = self.view.section == section;
                let text = egui::RichText::new(name).strong();
                if ui.selectable_label(selected, text).on_hover_text(hover).clicked() {
                    self.view.section = section;
                }
            }
        });
        let hint = match self.view.section {
            PanelSection::Settlements => "Search by name, region or type",
            PanelSection::Interiors => "Search by cell, building type or cell kind",
            _ => "Search by cell, kind or your name",
        };
        ui.add_sized(
            [ui.available_width(), ui.spacing().interact_size.y],
            egui::TextEdit::singleline(&mut self.search_text).hint_text(hint),
        );
        ui.separator();
        let search = self.search_text.trim().to_lowercase();
        let section = self.view.section;
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(("project_panel_list", section)).show(
            ui,
            |ui| {
                if self.locations.is_empty() && self.outdoor_cells.is_empty() {
                    ui.label(
                        egui::RichText::new("Choose your ARENA2 folder (with MAPS.BSA) to list the settlements.")
                            .weak(),
                    );
                    return;
                }
                match section {
                    PanelSection::Settlements => self.show_settlement_list(ui, &search, opened, action),
                    PanelSection::Interiors => self.show_interior_list(ui, &search, custom_names, opened, action),
                    _ => self.show_outdoor_cell_list(ui, &search, custom_names, opened, action),
                }
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Daggerfall's settlements and outdoor cells open read-only. Your own are not supported yet.",
                    )
                    .weak()
                    .small(),
                );
            },
        );
    }

    /// Draws Daggerfall's locations: by region and type, or the matches of `search`.
    fn show_settlement_list(
        &self,
        ui: &mut egui::Ui,
        search: &str,
        opened: Option<&OpenedItem>,
        action: &mut Option<PanelAction>,
    ) {
        if !search.is_empty() {
            let matches = (0..self.locations.len()).filter(|&index| {
                let location = &self.locations[index];
                location.name.to_lowercase().contains(search)
                    || location.region_name.to_lowercase().contains(search)
                    || location.location_type.is_some_and(|kind| kind.name().to_lowercase().contains(search))
            });
            show_limited(ui, matches, |ui, index| self.show_settlement_row(ui, index, true, opened, action));
            return;
        }
        let header = format!("Daggerfall ({})", self.locations.len());
        egui::CollapsingHeader::new(header).id_salt("daggerfall_settlements").default_open(true).show(
            ui,
            |ui| {
                for region in &self.location_regions {
                    let header = format!("{} ({})", region.name, region.count);
                    egui::CollapsingHeader::new(header).id_salt(("settlement_region", region.name)).show(
                        ui,
                        |ui| {
                            for (kind, indices) in &region.kinds {
                                let header = format!("{kind} ({})", indices.len());
                                egui::CollapsingHeader::new(header)
                                    .id_salt(("settlement_kind", region.name, *kind))
                                    .show(ui, |ui| {
                                        for &index in indices {
                                            self.show_settlement_row(ui, index, false, opened, action);
                                        }
                                    });
                            }
                        },
                    );
                }
            },
        );
    }

    /// Draws one location row; `show_region` adds its region and type, for search results.
    fn show_settlement_row(
        &self,
        ui: &mut egui::Ui,
        index: usize,
        show_region: bool,
        opened: Option<&OpenedItem>,
        action: &mut Option<PanelAction>,
    ) {
        let location = &self.locations[index];
        let kind = location.location_type.map_or("Location", |kind| kind.name());
        let label = match show_region {
            true => format!("{}   ·   {kind}   ·   {}", location.name, location.region_name),
            false => location.name.clone(),
        };
        let dungeon = if location.has_dungeon { " · has a dungeon" } else { "" };
        let item = OpenedItem::DaggerfallSettlement(index);
        let response = ui.selectable_label(opened == Some(&item), label).on_hover_text(format!(
            "{kind} · {} × {} cells{dungeon}\nClick to open (read-only)",
            location.exterior.width, location.exterior.height
        ));
        outdoor_item_interaction(&response, item, false, opened, action);
    }

    /// Draws Daggerfall's outdoor cells: by kind, or the matches of `search`.
    fn show_outdoor_cell_list(
        &self,
        ui: &mut egui::Ui,
        search: &str,
        custom_names: &CustomNames,
        opened: Option<&OpenedItem>,
        action: &mut Option<PanelAction>,
    ) {
        if !search.is_empty() {
            let matches = self.outdoor_cells.iter().filter(|name| {
                name.to_lowercase().contains(search)
                    || rmb_block_kind(name).to_lowercase().contains(search)
                    || custom_names.room_name_matches(name, search)
            });
            show_limited(ui, matches, |ui, name| {
                self.show_outdoor_cell_row(ui, name, custom_names, opened, action)
            });
            return;
        }
        let header = format!("Daggerfall ({})", self.outdoor_cells.len());
        egui::CollapsingHeader::new(header).id_salt("daggerfall_outdoor_cells").default_open(true).show(
            ui,
            |ui| {
                for (kind, indices) in &self.outdoor_kinds {
                    let header = format!("{kind} ({})", indices.len());
                    egui::CollapsingHeader::new(header).id_salt(("outdoor_cell_kind", *kind)).show(
                        ui,
                        |ui| {
                            for &index in indices {
                                self.show_outdoor_cell_row(
                                    ui,
                                    &self.outdoor_cells[index],
                                    custom_names,
                                    opened,
                                    action,
                                );
                            }
                        },
                    );
                }
            },
        );
    }

    /// Draws the buildings with an inside: by type and cell kind, or the matches of `search`.
    fn show_interior_list(
        &self,
        ui: &mut egui::Ui,
        search: &str,
        custom_names: &CustomNames,
        opened: Option<&OpenedItem>,
        action: &mut Option<PanelAction>,
    ) {
        if !search.is_empty() {
            let matches = self.interiors.iter().filter(|(key, building_type)| {
                key.block.to_lowercase().contains(search)
                    || building_type_name(*building_type).to_lowercase().contains(search)
                    || rmb_block_kind(&key.block).to_lowercase().contains(search)
                    || custom_names.room_name_matches(&key.block, search)
            });
            show_limited(ui, matches, |ui, (key, building_type)| {
                self.show_interior_row(ui, key, *building_type, true, custom_names, opened, action)
            });
            return;
        }
        let header = format!("Daggerfall ({})", self.interiors.len());
        egui::CollapsingHeader::new(header).id_salt("daggerfall_interiors").default_open(true).show(
            ui,
            |ui| {
                for (type_name, kinds) in &self.interior_groups {
                    let count: usize = kinds.iter().map(|(_, indices)| indices.len()).sum();
                    egui::CollapsingHeader::new(format!("{type_name} ({count})"))
                        .id_salt(("interior_type", type_name.as_str()))
                        .show(ui, |ui| {
                            for (kind, indices) in kinds {
                                egui::CollapsingHeader::new(format!("{kind} ({})", indices.len()))
                                    .id_salt(("interior_kind", type_name.as_str(), *kind))
                                    .show(ui, |ui| {
                                        for &index in indices {
                                            let (key, building_type) = &self.interiors[index];
                                            self.show_interior_row(
                                                ui,
                                                key,
                                                *building_type,
                                                false,
                                                custom_names,
                                                opened,
                                                action,
                                            );
                                        }
                                    });
                            }
                        });
                }
            },
        );
    }

    /// Draws one building row: its cell and place there; `show_type` adds its type, for search results.
    #[allow(clippy::too_many_arguments)]
    fn show_interior_row(
        &self,
        ui: &mut egui::Ui,
        key: &InteriorKey,
        building_type: u8,
        show_type: bool,
        custom_names: &CustomNames,
        opened: Option<&OpenedItem>,
        action: &mut Option<PanelAction>,
    ) {
        let mut label = key.label();
        if let Some(custom_name) = custom_names.room_name(&key.block) {
            label = format!("{label}   {custom_name}");
        }
        if show_type {
            label = format!("{label}   ·   {}", building_type_name(building_type));
        }
        let users = self.outdoor_usage.get(&key.block).map_or(0, Vec::len);
        let used = match users {
            0 => "its cell is used by no location".to_string(),
            1 => "its cell is used in 1 location".to_string(),
            count => format!("its cell is used in {count} locations"),
        };
        let item = OpenedItem::DaggerfallInterior(key.clone());
        let response = ui.selectable_label(opened == Some(&item), label).on_hover_text(format!(
            "{} in {} ({}) · {used}\nDouble-click to open (read-only)",
            building_type_name(building_type),
            key.block,
            rmb_block_kind(&key.block)
        ));
        outdoor_item_interaction(&response, item, true, opened, action);
    }

    /// Draws one outdoor cell row, with the user's name for it if it has one.
    fn show_outdoor_cell_row(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        custom_names: &CustomNames,
        opened: Option<&OpenedItem>,
        action: &mut Option<PanelAction>,
    ) {
        let label = match custom_names.room_name(name) {
            Some(custom_name) => format!("{name}   {custom_name}"),
            None => name.to_string(),
        };
        let users = self.outdoor_usage.get(name).map_or(0, Vec::len);
        let used = match users {
            0 => "used by no location".to_string(),
            1 => "used in 1 location".to_string(),
            count => format!("used in {count} locations"),
        };
        let item = OpenedItem::DaggerfallOutdoorCell(name.to_string());
        let response = ui
            .selectable_label(opened == Some(&item), label)
            .on_hover_text(format!("{} · {used}\nDouble-click to open (read-only)", rmb_block_kind(name)));
        outdoor_item_interaction(&response, item, true, opened, action);
    }

    /// Draws the read-only list of Daggerfall's own dungeons or rooms.
    fn show_daggerfall_list(
        &self,
        ui: &mut egui::Ui,
        section: PanelSection,
        search: &str,
        custom_names: &CustomNames,
        opened: Option<&OpenedItem>,
        action: &mut Option<PanelAction>,
    ) {
        if self.room_names.is_empty() {
            egui::CollapsingHeader::new("Daggerfall").id_salt("daggerfall_list").default_open(true).show(
                ui,
                |ui| {
                    ui.label(
                        egui::RichText::new("Choose your ARENA2 folder to list the game's cells.").weak(),
                    );
                },
            );
            return;
        }

        // Search results are a flat list, so the folded state of the lists is left as the user had it.
        if !search.is_empty() {
            ui.strong("Daggerfall");
            ui.indent("daggerfall_matches", |ui| match section {
                PanelSection::Dungeons => {
                    let dungeon_matches = (0..self.dungeons.len()).filter(|&index| {
                        let dungeon = &self.dungeons[index];
                        dungeon.name.to_lowercase().contains(search)
                            || dungeon.region_name.to_lowercase().contains(search)
                    });
                    show_limited(ui, dungeon_matches, |ui, index| {
                        self.show_daggerfall_dungeon(ui, index, true, opened, action);
                    });
                }
                _ => {
                    let room_matches = self.room_names.iter().filter(|name| {
                        name.to_lowercase().contains(search) || custom_names.room_name_matches(name, search)
                    });
                    show_limited(ui, room_matches, |ui, name| {
                        show_daggerfall_room(ui, name, custom_names, opened, action);
                    });
                }
            });
            return;
        }

        match section {
            PanelSection::Dungeons => {
                let header = format!("Daggerfall ({})", self.dungeons.len());
                egui::CollapsingHeader::new(header).id_salt("daggerfall_dungeons").default_open(true).show(
                    ui,
                    |ui| {
                        for region in &self.regions {
                            let header = format!("{} ({})", region.name, region.dungeons.len());
                            egui::CollapsingHeader::new(header)
                                .id_salt(("daggerfall_region", region.name))
                                .show(ui, |ui| {
                                    for index in region.dungeons.clone() {
                                        self.show_daggerfall_dungeon(ui, index, false, opened, action);
                                    }
                                });
                        }
                    },
                );
            }
            _ => {
                let header = format!("Daggerfall ({})", self.room_names.len());
                egui::CollapsingHeader::new(header).id_salt("daggerfall_rooms").default_open(true).show(
                    ui,
                    |ui| {
                        for name in &self.room_names {
                            show_daggerfall_room(ui, name, custom_names, opened, action);
                        }
                    },
                );
            }
        }
    }

    /// Draws one Daggerfall dungeon row; `show_region` adds its region, for search results.
    fn show_daggerfall_dungeon(
        &self,
        ui: &mut egui::Ui,
        index: usize,
        show_region: bool,
        opened: Option<&OpenedItem>,
        action: &mut Option<PanelAction>,
    ) {
        let dungeon = &self.dungeons[index];
        let label = if show_region {
            format!("{}   ·   {}", dungeon.name, dungeon.region_name)
        } else {
            dungeon.name.clone()
        };
        let item = OpenedItem::DaggerfallDungeon(index);
        let response = ui.selectable_label(opened == Some(&item), label).on_hover_text(format!(
            "{} · {} cells\nClick to open (read-only), or drag into the project to copy it",
            dungeon.dungeon_type.map_or("Unknown type", |dungeon_type| dungeon_type.name()),
            dungeon.blocks.len()
        ));
        daggerfall_item_interaction(&response, item, opened, action);
    }

    /// Draws a project entry, and a folder's contents.
    fn show_entry(
        &mut self,
        ui: &mut egui::Ui,
        entry: &ProjectEntry,
        context: &TreeContext<'_>,
        action: &mut Option<PanelAction>,
        changes: &mut Vec<TreeChange>,
    ) {
        if !entry_matches(entry, context.search, context.section) {
            return;
        }
        let id = entry.id();
        if self.renaming.as_ref().is_some_and(|(renamed, _)| *renamed == id) {
            self.show_rename_field(ui, changes);
            if let ProjectEntry::Folder(folder) = entry {
                ui.indent(("project_folder_renaming", id), |ui| {
                    for child in folders_first(&folder.entries) {
                        self.show_entry(ui, child, context, action, changes);
                    }
                });
            }
            return;
        }

        match entry {
            ProjectEntry::Folder(folder) => {
                let searching = !context.search.is_empty();
                let response = egui::CollapsingHeader::new(&folder.name)
                    .id_salt(("project_folder", id))
                    .default_open(true)
                    .open(searching.then_some(true))
                    .show(ui, |ui| {
                        if folder.entries.is_empty() {
                            ui.label(egui::RichText::new("Empty").weak());
                        }
                        for child in folders_first(&folder.entries) {
                            self.show_entry(ui, child, context, action, changes);
                        }
                    });
                let header = drag_source(&response.header_response, DragPayload::Project(id));
                drop_target(ui, &header, DropTarget::Into(Some(id)), DropHighlight::Area, changes);
                header.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        self.start_renaming(id, &folder.name);
                        ui.close_menu();
                    }
                    if ui.button("New folder inside").clicked() {
                        changes.push(TreeChange::NewFolder(Some(id)));
                        ui.close_menu();
                    }
                    move_to_menu(ui, id, context, changes);
                    ui.separator();
                    if ui.button("Remove folder").on_hover_text("Its contents move up a level").clicked() {
                        changes.push(TreeChange::RemoveFolder(id));
                        ui.close_menu();
                    }
                });
            }
            ProjectEntry::Room(_) | ProjectEntry::Dungeon(_) => {
                let (kind, details) = match entry {
                    ProjectEntry::Room(room) => {
                        let mut details = room
                            .based_on
                            .as_deref()
                            .map(|source| format!("Copy of {source}"))
                            .unwrap_or_default();
                        let users = context.project.dungeons_using(id);
                        if !users.is_empty() {
                            details.push_str(&format!("\nUsed in {}", users.join(", ")));
                        }
                        ("cell", details)
                    }
                    ProjectEntry::Dungeon(dungeon) => {
                        let dungeon_type =
                            dungeon.dungeon_type.map_or("Unknown type", |dungeon_type| dungeon_type.name());
                        let details = format!(
                            "{dungeon_type} · {} cells · {}",
                            dungeon.rooms.len(),
                            dungeon.region_name
                        );
                        ("dungeon", details)
                    }
                    ProjectEntry::Folder(_) => unreachable!("folders are handled above"),
                };
                let item = OpenedItem::Project(id);
                let is_dungeon = matches!(entry, ProjectEntry::Dungeon(_));
                let response = ui
                    .horizontal(|ui| {
                        let response = ui.selectable_label(context.opened == Some(&item), entry.name());
                        ui.label(egui::RichText::new(kind).weak().small());
                        response
                    })
                    .inner
                    .on_hover_text(format!(
                        "{details}\n{} to open, right-click for more, drag to move",
                        if is_dungeon { "Click" } else { "Double-click" }
                    ));
                let response = drag_source(&response, DragPayload::Project(id));
                drop_target(ui, &response, DropTarget::Before(id), DropHighlight::LineAbove, changes);
                if opens_item(&response, is_dungeon) && context.opened != Some(&item) {
                    *action = Some(PanelAction::Open(item.clone()));
                }
                response.context_menu(|ui| {
                    if ui.button("Open").clicked() {
                        *action = Some(PanelAction::Open(item.clone()));
                        ui.close_menu();
                    }
                    if ui.button("Rename").clicked() {
                        self.start_renaming(id, entry.name());
                        ui.close_menu();
                    }
                    if ui.button("Duplicate").clicked() {
                        *action = Some(PanelAction::Duplicate(id));
                        ui.close_menu();
                    }
                    move_to_menu(ui, id, context, changes);
                    ui.separator();
                    if ui.button("Delete…").clicked() {
                        *action = Some(PanelAction::Delete(id));
                        ui.close_menu();
                    }
                });
            }
        }
    }

    /// Draws the name field of the entry being renamed. Enter or clicking away keeps the name; Escape cancels.
    fn show_rename_field(&mut self, ui: &mut egui::Ui, changes: &mut Vec<TreeChange>) {
        let Some((id, name)) = self.renaming.as_mut() else { return };
        let response = ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
        if std::mem::take(&mut self.focus_rename_field) {
            response.request_focus();
        }
        if response.lost_focus() {
            let cancelled = ui.input(|input| input.key_pressed(egui::Key::Escape));
            if !cancelled && !name.trim().is_empty() {
                changes.push(TreeChange::Rename(*id, name.trim().to_string()));
            }
            self.renaming = None;
        }
    }
}

/// What every project entry row needs to know.
struct TreeContext<'tree> {
    search: &'tree str,
    section: PanelSection,
    opened: Option<&'tree OpenedItem>,
    folders: &'tree [crate::project::FolderListing],
    project: &'tree Project,
}

/// Draws one Daggerfall room row, with the user's name for the room if it has one.
fn show_daggerfall_room(
    ui: &mut egui::Ui,
    name: &str,
    custom_names: &CustomNames,
    opened: Option<&OpenedItem>,
    action: &mut Option<PanelAction>,
) {
    let label = match custom_names.room_name(name) {
        Some(custom_name) => format!("{name}   {custom_name}"),
        None => name.to_string(),
    };
    let item = OpenedItem::DaggerfallRoom(name.to_string());
    let response = ui.selectable_label(opened == Some(&item), label).on_hover_text(format!(
        "{}\nDouble-click to open (read-only), or drag into the project to copy it",
        category_description(name)
    ));
    daggerfall_item_interaction(&response, item, opened, action);
}

/// Clicking opens a settlement, double-clicking an outdoor cell (`is_cell`);
/// right-clicking offers to open it. They cannot be copied into the project yet.
fn outdoor_item_interaction(
    response: &egui::Response,
    item: OpenedItem,
    is_cell: bool,
    opened: Option<&OpenedItem>,
    action: &mut Option<PanelAction>,
) {
    if opens_item(response, !is_cell) && opened != Some(&item) {
        *action = Some(PanelAction::Open(item.clone()));
    }
    response.context_menu(|ui| {
        if ui.button("Open (read-only)").clicked() {
            *action = Some(PanelAction::Open(item.clone()));
            ui.close_menu();
        }
    });
}

/// Whether `response` opens its item: one click for a dungeon, a double-click for a cell.
fn opens_item(response: &egui::Response, is_dungeon: bool) -> bool {
    if is_dungeon {
        response.clicked()
    } else {
        response.double_clicked()
    }
}

/// Clicking opens a Daggerfall dungeon (double-clicking a cell); right-click offers to open or copy it.
fn daggerfall_item_interaction(
    response: &egui::Response,
    item: OpenedItem,
    opened: Option<&OpenedItem>,
    action: &mut Option<PanelAction>,
) {
    let response = drag_source(response, DragPayload::Daggerfall(item.clone()));
    let is_dungeon = matches!(item, OpenedItem::DaggerfallDungeon(_));
    if opens_item(&response, is_dungeon) && opened != Some(&item) {
        *action = Some(PanelAction::Open(item.clone()));
    }
    response.context_menu(|ui| {
        if ui.button("Open (read-only)").clicked() {
            *action = Some(PanelAction::Open(item.clone()));
            ui.close_menu();
        }
        if ui.button("Create a copy").on_hover_text("Add an editable copy to the project").clicked() {
            *action = Some(PanelAction::CopyFromDaggerfall { item: item.clone(), folder: None, open: true });
            ui.close_menu();
        }
    });
}

/// Adds a "Move to" submenu listing the top level and every folder `id` can move into.
fn move_to_menu(ui: &mut egui::Ui, id: ItemId, context: &TreeContext<'_>, changes: &mut Vec<TreeChange>) {
    let current_parent = context.project.parent_of(id).flatten();
    ui.menu_button("Move to", |ui| {
        if ui.add_enabled(current_parent.is_some(), egui::Button::new("Top level")).clicked() {
            changes.push(TreeChange::Move(id, None));
            ui.close_menu();
        }
        let moved = context.project.find(id);
        for folder in context.folders {
            // A folder cannot move into itself or one of its own folders.
            let inside_moved = moved.is_some_and(|entry| match entry {
                ProjectEntry::Folder(_) => {
                    folder.id == id || context.project.parent_chain(folder.id).contains(&id)
                }
                _ => false,
            });
            if inside_moved {
                continue;
            }
            let enabled = current_parent != Some(folder.id);
            if ui.add_enabled(enabled, egui::Button::new(&folder.path)).clicked() {
                changes.push(TreeChange::Move(id, Some(folder.id)));
                ui.close_menu();
            }
        }
    });
}

/// Whether `entry` shows in `section` with the lowercase `search`: a room or
/// dungeon of that section whose name matches, or a folder holding one (or
/// matching itself, as long as it is empty or holds something of the section).
fn entry_matches(entry: &ProjectEntry, search: &str, section: PanelSection) -> bool {
    let name_matches = search.is_empty() || entry.name().to_lowercase().contains(search);
    match entry {
        ProjectEntry::Folder(folder) => {
            let holds_section = folder.entries.is_empty() || entry_holds(entry, section);
            (name_matches && holds_section)
                || folder.entries.iter().any(|child| entry_matches(child, search, section))
        }
        _ => name_matches && PanelSection::of(entry) == Some(section),
    }
}

/// Whether `entry` is, or holds, a room or dungeon of `section`.
fn entry_holds(entry: &ProjectEntry, section: PanelSection) -> bool {
    match entry {
        ProjectEntry::Folder(folder) => folder.entries.iter().any(|child| entry_holds(child, section)),
        _ => PanelSection::of(entry) == Some(section),
    }
}

/// An on/off switch, drawn as a sliding knob.
fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = ui.spacing().interact_size.y * egui::vec2(1.8, 0.9);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, ""));
    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool(response.id, *on);
        let visuals = ui.style().interact_selectable(&response, *on);
        let rect = rect.expand(visuals.expansion);
        let radius = 0.5 * rect.height();
        ui.painter().rect(rect, radius, visuals.bg_fill, visuals.bg_stroke);
        let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
        ui.painter().circle(
            egui::pos2(knob_x, rect.center().y),
            0.75 * radius,
            visuals.fg_stroke.color,
            egui::Stroke::NONE,
        );
    }
    response
}

/// Draws up to [`MAX_SEARCH_MATCHES`] rows, then says how many more there are.
fn show_limited<Item>(
    ui: &mut egui::Ui,
    items: impl Iterator<Item = Item>,
    mut show_row: impl FnMut(&mut egui::Ui, Item),
) {
    let mut count = 0;
    for item in items {
        if count < MAX_SEARCH_MATCHES {
            show_row(ui, item);
        }
        count += 1;
    }
    if count == 0 {
        ui.label(egui::RichText::new("No matches").weak());
    } else if count > MAX_SEARCH_MATCHES {
        ui.label(egui::RichText::new(format!("…and {} more", count - MAX_SEARCH_MATCHES)).weak());
    }
}

/// The entries of a list with folders before rooms and dungeons, each in their stored order.
fn folders_first(entries: &[ProjectEntry]) -> impl Iterator<Item = &ProjectEntry> {
    let folders = entries.iter().filter(|entry| entry.is_folder());
    folders.chain(entries.iter().filter(|entry| !entry.is_folder()))
}

/// How a drop target shows that something would be dropped on it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DropHighlight {
    /// Outline the whole target.
    Area,
    /// Draw a line above the target, where the entry would be inserted.
    LineAbove,
}

/// Lets `response`'s widget be dragged, carrying `payload`. Clicks still work as before.
fn drag_source(response: &egui::Response, payload: DragPayload) -> egui::Response {
    let response = response.interact(egui::Sense::drag());
    response.dnd_set_drag_payload(payload);
    response
}

/// Highlights `response` while something is dragged over it, and records a drop on it.
fn drop_target(
    ui: &egui::Ui,
    response: &egui::Response,
    target: DropTarget,
    highlight: DropHighlight,
    changes: &mut Vec<TreeChange>,
) {
    if response.dnd_hover_payload::<DragPayload>().is_some() {
        let stroke = egui::Stroke::new(2.0_f32, ui.visuals().selection.stroke.color);
        let rect = response.rect;
        match highlight {
            DropHighlight::Area => {
                ui.painter().rect_stroke(rect.expand(1.0), 3.0, stroke);
            }
            DropHighlight::LineAbove => {
                ui.painter().hline(rect.x_range(), rect.top() - 1.0, stroke);
            }
        }
    }
    if let Some(payload) = response.dnd_release_payload::<DragPayload>() {
        changes.push(TreeChange::Drop((*payload).clone(), target));
    }
}

/// Shows the name of the dragged entry next to the pointer.
fn show_drag_preview(context: &egui::Context, project: &Project, dungeons: &[DungeonDefinition]) {
    let Some(payload) = egui::DragAndDrop::payload::<DragPayload>(context) else { return };
    let Some(pointer) = context.pointer_interact_pos() else { return };
    let name = match &*payload {
        DragPayload::Project(id) => project.find(*id).map(|entry| entry.name().to_string()),
        DragPayload::Daggerfall(OpenedItem::DaggerfallRoom(name)) => Some(format!("Copy of {name}")),
        DragPayload::Daggerfall(OpenedItem::DaggerfallDungeon(index)) => {
            dungeons.get(*index).map(|dungeon| format!("Copy of {}", dungeon.name))
        }
        DragPayload::Daggerfall(_) => None,
    };
    let Some(name) = name else { return };
    egui::Area::new(egui::Id::new("project_drag_preview"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pointer + egui::vec2(14.0, 10.0))
        .interactable(false)
        .show(context, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(name));
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{CellCategory, DungeonLocation, ProjectDungeon, ProjectRoom};

    fn room(project: &mut Project, name: &str) -> ProjectEntry {
        ProjectEntry::Room(ProjectRoom {
            id: project.new_id(),
            name: name.into(),
            based_on: None,
            category: CellCategory::Interior,
            objects: Vec::new(),
        })
    }

    fn dungeon(project: &mut Project, name: &str) -> ProjectEntry {
        ProjectEntry::Dungeon(ProjectDungeon {
            id: project.new_id(),
            name: name.into(),
            region_name: String::new(),
            dungeon_type: None,
            based_on: None,
            rooms: Vec::new(),
            location: DungeonLocation::default(),
        })
    }

    #[test]
    fn sections_show_their_own_kind_and_the_folders_holding_it() {
        let mut project = Project::default();
        let cells = project.add_folder("Cells only", None);
        let entry = room(&mut project, "Great Hall");
        project.add(entry, Some(cells));
        let empty = project.add_folder("Empty", None);
        let entry = dungeon(&mut project, "Hold");
        project.add(entry, None);

        let shown = |section: PanelSection, search: &str| -> Vec<&str> {
            project
                .entries
                .iter()
                .filter(|entry| entry_matches(entry, search, section))
                .map(ProjectEntry::name)
                .collect()
        };
        assert_eq!(shown(PanelSection::Cells, ""), ["Cells only", "Empty"]);
        assert_eq!(shown(PanelSection::Dungeons, ""), ["Empty", "Hold"]);
        assert_eq!(shown(PanelSection::Cells, "hall"), ["Cells only"]);
        assert_eq!(shown(PanelSection::Dungeons, "hall"), Vec::<&str>::new());
        assert_eq!(shown(PanelSection::Dungeons, "cells"), Vec::<&str>::new());
        assert!(project.find(empty).is_some());
    }

    #[test]
    fn groups_settlements_by_region_and_type_and_outdoor_cells_by_kind() {
        let location = |name: &str, region_name: &'static str, location_type| LocationDefinition {
            region_index: 0,
            region_name,
            name: name.to_string(),
            location_type,
            map_pixel: crate::arena2::MapPixel::new(0, 0),
            location_id: 0,
            index_in_region: 0,
            map_id: 0,
            exterior: crate::arena2::LocationExterior {
                width: 1,
                height: 1,
                block_names: vec!["TVRNAL01.RMB".to_string()],
                buildings: Vec::new(),
                port_town: false,
            },
            has_dungeon: false,
            discovered: false,
        };
        let locations: Arc<[LocationDefinition]> = Arc::new([
            location("Ashby", "Alcaire", Some(LocationType::Farm)),
            location("Wayrest", "Alcaire", Some(LocationType::City)),
            location("Yarn", "Alcaire", None),
            location("Kirk", "Betony", Some(LocationType::Village)),
        ]);
        let cells =
            ["XYZW0001.RMB", "TVRNAL01.RMB", "FARMAA02.RMB", "TVRNAM03.RMB"].map(String::from).to_vec();
        let mut panel = ProjectPanel::default();
        panel.set_outdoor_contents(locations, cells, Arc::default());

        let regions: Vec<(&str, usize)> =
            panel.location_regions.iter().map(|region| (region.name, region.count)).collect();
        assert_eq!(regions, [("Alcaire", 3), ("Betony", 1)]);
        assert_eq!(
            panel.location_regions[0].kinds,
            [("Cities", vec![1]), ("Farms", vec![0]), ("Other", vec![2])]
        );
        assert_eq!(panel.location_regions[1].kinds, [("Villages", vec![3])]);
        // Kinds in the game's order, unknown ones last.
        assert_eq!(panel.outdoor_kinds, [("Tavern", vec![1, 3]), ("Farm", vec![2]), ("Other", vec![0])]);
        assert!(PanelSection::OutdoorCells.is_outdoor() && !PanelSection::Cells.is_outdoor());
    }
}
