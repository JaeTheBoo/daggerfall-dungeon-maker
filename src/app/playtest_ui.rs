//! The Test button at the bottom of the window: testing the cell, dungeon
//! or settlement open in Daggerfall Unity (see [`crate::playtest`]).
//!
//! For a dungeon or a settlement it first asks whether to test it on its
//! own or where it is in the world. The test copy of Daggerfall Unity is
//! readied in the background; the bottom bar tells how far it has got, then
//! what is being tested and what the test leaves out. A test asked for while
//! another still runs in Daggerfall Unity closes that one first, once the
//! user agrees (both cannot use the test copy at once).

use std::path::PathBuf;
use std::process::Child;

use eframe::egui;
use glam::Vec2;

use super::project_panel::OpenedItem;
use super::{modal_window, DungeonMakerApp, Workspace};
use crate::arena2::Season;
use crate::export::unity::{editor_version, find_unity_editors, is_compatible_version};
use crate::playtest::{self, Setting, TestEvent, TestJob, TestPlan, TestRequest};
use crate::project::{CellCategory, ItemId};
use crate::scene::SceneObject;

/// What a test is of: what was open when Test was pressed.
#[derive(Debug, Clone)]
pub enum TestSubject {
    /// A dungeon cell, as the editor has it now.
    DungeonCell {
        name: String,
        objects: Vec<SceneObject>,
        category: CellCategory,
        based_on: Option<String>,
    },
    /// One of Daggerfall's dungeons, by index in the library's list.
    DaggerfallDungeon(usize),
    ProjectDungeon(ItemId),
    /// A settlement, by index in the library's location list, with the
    /// climate and season it is drawn in and the spot a test on its own starts at.
    Settlement {
        location_index: usize,
        climate: u8,
        season: Season,
        spot: Vec2,
    },
    /// An outdoor cell, with the climate and season it is drawn in and the spot a test starts at.
    OutdoorCell {
        block_name: String,
        climate: u8,
        season: Season,
        spot: Vec2,
    },
}

impl TestSubject {
    /// The season the game starts in: the one an outdoor view is drawn in.
    fn season(&self) -> Option<Season> {
        match self {
            Self::Settlement { season, .. } | Self::OutdoorCell { season, .. } => Some(*season),
            _ => None,
        }
    }

    /// Whether the user chooses between testing it on its own and where it is.
    fn has_setting_choice(&self) -> bool {
        !matches!(self, Self::DungeonCell { .. } | Self::OutdoorCell { .. })
    }

    fn is_dungeon(&self) -> bool {
        matches!(self, Self::DaggerfallDungeon(_) | Self::ProjectDungeon(_))
    }
}

/// The state of testing.
#[derive(Default)]
pub struct PlaytestState {
    /// The question asked before testing a dungeon or settlement: its subject and name,
    /// and why it cannot be tested where it is, if it cannot.
    choice: Option<(TestSubject, String, Option<String>)>,
    /// A test waiting for the user to agree to close the one running.
    waiting: Option<TestPlan>,
    job: Option<TestJob>,
    /// The plan of the test being readied.
    readying: Option<TestPlan>,
    /// What the bottom bar says about testing.
    message: Option<String>,
    /// The game of the last test, while it may still run.
    game: Option<Child>,
}

impl PlaytestState {
    /// Whether a test is being readied.
    pub fn is_busy(&self) -> bool {
        self.job.is_some()
    }

    /// Whether the game of the last test is still running.
    fn game_runs(&mut self) -> bool {
        match self.game.as_mut().map(Child::try_wait) {
            Some(Ok(None)) => true,
            _ => {
                self.game = None;
                false
            }
        }
    }
}

impl DungeonMakerApp {
    /// Draws the right end of the bottom bar: how testing goes, and the Test button.
    pub(super) fn show_test_button(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let testable =
                matches!(self.workspace, Workspace::Room(_) | Workspace::Dungeon(_) | Workspace::Exterior(_));
            let busy = self.playtest.is_busy();
            let button = ui
                .add_enabled(testable && !busy && self.library.is_some(), egui::Button::new("▶ Test"))
                .on_hover_text(
                    "Play what is open in a copy of your Daggerfall Unity, starting at its entrance with \
                     no-clip on. Your own Daggerfall Unity, its settings and saved games are not changed.",
                )
                .on_disabled_hover_text("Open a cell, dungeon or settlement to test it in Daggerfall Unity");
            if button.clicked() {
                self.request_test();
            }
            if let Some(message) = &self.playtest.message {
                if busy {
                    ui.spinner();
                }
                ui.label(message);
            }
        });
    }

    /// Starts testing what is open: asks how first for a dungeon or settlement.
    fn request_test(&mut self) {
        // Edits in the open cell or dungeon count.
        self.store_workspace_changes();
        let subject = match self.test_subject() {
            Ok(subject) => subject,
            Err(error) => {
                self.error_message = Some(error);
                return;
            }
        };
        if subject.has_setting_choice() {
            let (name, unplaced) = self.subject_name(&subject);
            self.playtest.choice = Some((subject, name, unplaced));
        } else {
            self.begin_test(&subject, Setting::OnItsOwn);
        }
    }

    /// What is open, as a test subject.
    fn test_subject(&mut self) -> Result<TestSubject, String> {
        let library = self.library.as_mut().ok_or("Choose Daggerfall's ARENA2 folder first")?;
        match (&self.workspace, &self.opened_item) {
            (Workspace::Room(editor), item) => {
                let objects = editor.scene.objects.clone();
                match item {
                    Some(OpenedItem::Project(id)) => {
                        let room = self.project.room(*id).ok_or("The cell is no longer in the project")?;
                        Ok(TestSubject::DungeonCell {
                            name: room.name.clone(),
                            objects,
                            category: editor.cell_category().unwrap_or(room.category),
                            based_on: room.based_on.clone(),
                        })
                    }
                    _ => {
                        let name = editor.scene.block_name.clone();
                        Ok(TestSubject::DungeonCell {
                            category: CellCategory::of_copy(Some(name.as_str())),
                            based_on: Some(name.clone()),
                            name,
                            objects,
                        })
                    }
                }
            }
            (Workspace::Dungeon(_), Some(OpenedItem::DaggerfallDungeon(index))) => {
                Ok(TestSubject::DaggerfallDungeon(*index))
            }
            (Workspace::Dungeon(_), Some(OpenedItem::Project(id))) => Ok(TestSubject::ProjectDungeon(*id)),
            (Workspace::Exterior(view), _) => {
                let (climate, season) = (view.climate().value, view.season());
                let spot = view.test_spot(library);
                Ok(match &view.subject {
                    super::exterior_view::ExteriorSubject::Settlement(location_index) => {
                        TestSubject::Settlement { location_index: *location_index, climate, season, spot }
                    }
                    super::exterior_view::ExteriorSubject::Cell(block_name) => {
                        TestSubject::OutdoorCell { block_name: block_name.clone(), climate, season, spot }
                    }
                })
            }
            _ => Err("Open a cell, dungeon or settlement to test it".to_string()),
        }
    }

    /// The name of `subject`, and why it cannot be tested where it is, if it cannot.
    fn subject_name(&mut self, subject: &TestSubject) -> (String, Option<String>) {
        let Some(library) = self.library.as_mut() else { return (String::new(), None) };
        match subject {
            TestSubject::DaggerfallDungeon(index) => {
                let dungeons = library.dungeons();
                let definition = dungeons.get(*index);
                let name = definition.map(|definition| definition.name.clone()).unwrap_or_default();
                let unplaced = definition
                    .filter(|definition| definition.map_pixel.is_none())
                    .map(|_| "Where it is on the world map is not known".to_string());
                (name, unplaced)
            }
            TestSubject::ProjectDungeon(id) => match self.project.dungeon(*id) {
                Some(dungeon) => (
                    dungeon.name.clone(),
                    dungeon.location.map_pixel.is_none().then(|| {
                        "It is not placed on the world map yet (choose its spot in its properties)"
                            .to_string()
                    }),
                ),
                None => (String::new(), None),
            },
            TestSubject::Settlement { location_index, .. } => {
                let locations = library.locations();
                (
                    locations.get(*location_index).map(|location| location.name.clone()).unwrap_or_default(),
                    None,
                )
            }
            TestSubject::DungeonCell { name, .. } => (name.clone(), None),
            TestSubject::OutdoorCell { block_name, .. } => (block_name.clone(), None),
        }
    }

    /// Draws the question of how to test a dungeon or settlement, and acts on the answer.
    pub(super) fn show_test_prompts(&mut self, context: &egui::Context) {
        self.poll_test_job(context);
        if let Some((subject, name, unplaced)) = self.playtest.choice.clone() {
            let mut answer: Option<Option<Setting>> = None;
            let (own_text, placed_text) = if subject.is_dungeon() {
                (
                    "Starts inside it at its entrance, with only flat, bare land around it.",
                    "Starts outside at its entrance, where it is on the world map, to walk in.",
                )
            } else {
                (
                    "Sets it down on flat, bare land with nothing around it, and starts in its middle.",
                    "Starts where travelling to it arrives, where it is on the world map.",
                )
            };
            modal_window(context, "Test in Daggerfall Unity", |ui| {
                ui.label(format!("How do you want to test {name}?"));
                ui.add_space(6.0);
                if ui.button("On its own").on_hover_text(own_text).clicked() {
                    answer = Some(Some(Setting::OnItsOwn));
                }
                ui.small(own_text);
                ui.add_space(4.0);
                let placed = ui.add_enabled(unplaced.is_none(), egui::Button::new("Where it is"));
                let placed = match &unplaced {
                    Some(reason) => placed.on_disabled_hover_text(reason.as_str()),
                    None => placed.on_hover_text(placed_text),
                };
                if placed.clicked() {
                    answer = Some(Some(Setting::WhereItIs));
                }
                ui.small(unplaced.as_deref().unwrap_or(placed_text));
                ui.add_space(6.0);
                if ui.button("Cancel").clicked() {
                    answer = Some(None);
                }
            });
            if let Some(answer) = answer {
                self.playtest.choice = None;
                if let Some(setting) = answer {
                    self.begin_test(&subject, setting);
                }
            }
        }
        if let Some(plan) = self.playtest.waiting.clone() {
            let mut answer = None;
            modal_window(context, "A test is running", |ui| {
                ui.label("Daggerfall Unity is still running the last test. Close it and start this one?");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Close it and test").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                });
            });
            if let Some(answer) = answer {
                self.playtest.waiting = None;
                if answer {
                    if let Some(mut game) = self.playtest.game.take() {
                        let _ = game.kill();
                        let _ = game.wait();
                    }
                    self.start_test_job(plan);
                }
            }
        }
    }

    /// Makes the plan for testing `subject` and starts readying it.
    fn begin_test(&mut self, subject: &TestSubject, setting: Setting) {
        let Some(library) = self.library.as_mut() else { return };
        let plan = match subject {
            TestSubject::DungeonCell { name, objects, category, based_on } => {
                playtest::dungeon_cell(name, objects, *category, based_on.as_deref(), library)
            }
            TestSubject::DaggerfallDungeon(index) => match library.dungeons().get(*index) {
                Some(definition) => playtest::daggerfall_dungeon(&definition.clone(), setting, library),
                None => Err("The dungeon is not in the game data".to_string()),
            },
            TestSubject::ProjectDungeon(id) => {
                playtest::project_dungeon(&self.project, *id, setting, library)
            }
            TestSubject::Settlement { location_index, climate, spot, .. } => {
                let locations = library.locations();
                match locations.get(*location_index) {
                    Some(location) => {
                        let dungeons = library.dungeons();
                        let dungeon = self
                            .dungeon_under
                            .get(&(location.region_index, location.location_id))
                            .and_then(|&index| dungeons.get(index));
                        playtest::settlement(location, dungeon, setting, *climate, *spot, library)
                    }
                    None => Err("The settlement is not in the game data".to_string()),
                }
            }
            TestSubject::OutdoorCell { block_name, climate, spot, .. } => {
                playtest::outdoor_cell(block_name, *climate, *spot, library)
            }
        };
        let plan = plan.map(|plan| TestPlan { season: subject.season(), ..plan });
        match plan {
            Ok(plan) if self.playtest.game_runs() => self.playtest.waiting = Some(plan),
            Ok(plan) => self.start_test_job(plan),
            Err(error) => self.error_message = Some(error),
        }
    }

    /// Starts readying the test copy for `plan` in the background.
    fn start_test_job(&mut self, plan: TestPlan) {
        if self.unity_folder.is_none() {
            self.choose_unity_folder();
        }
        let Some(install) = self.unity_folder.clone() else {
            self.error_message = Some(
                "Testing needs your Daggerfall Unity: choose its folder with File > Set Daggerfall Unity folder."
                    .to_string(),
            );
            return;
        };
        let daggerfall_folder = self.library.as_ref().and_then(|library| {
            let arena2 = library.root();
            let is_arena2 = arena2.file_name().is_some_and(|name| name.eq_ignore_ascii_case("arena2"));
            is_arena2.then(|| arena2.parent().map(PathBuf::from)).flatten()
        });
        self.playtest.message = Some(format!("Readying the test of {}…", plan.title));
        self.playtest.readying = Some(plan.clone());
        self.playtest.job = Some(TestJob::start(TestRequest {
            plan,
            install,
            daggerfall_folder,
            unity_editor: self.test_unity_editor(),
        }));
    }

    /// The Unity editor to build the test mod with: the export's, if it is
    /// a version that can, else the first such one installed.
    fn test_unity_editor(&self) -> Option<PathBuf> {
        let compatible = |editor: &PathBuf| {
            editor.is_file() && editor_version(editor).is_none_or(|version| is_compatible_version(&version))
        };
        let chosen = PathBuf::from(self.export_window.settings.unity_editor.trim());
        if compatible(&chosen) {
            return Some(chosen);
        }
        find_unity_editors()
            .into_iter()
            .find(|editor| editor_version(editor).is_some_and(|version| is_compatible_version(&version)))
    }

    /// Takes the news of the test being readied.
    fn poll_test_job(&mut self, context: &egui::Context) {
        let Some(job) = &self.playtest.job else { return };
        context.request_repaint_after(std::time::Duration::from_millis(200));
        for event in job.poll() {
            match event {
                TestEvent::Step(step) => self.playtest.message = Some(step),
                TestEvent::Started(child) => {
                    self.playtest.job = None;
                    self.playtest.game = Some(child);
                    let plan = self.playtest.readying.take();
                    let title = plan.as_ref().map(|plan| plan.title.clone()).unwrap_or_default();
                    let mut message = format!("Testing {title} in Daggerfall Unity, with no-clip on");
                    if let Some(plan) = plan.filter(|plan| !plan.notes.is_empty()) {
                        message.push_str(&format!(" ({})", plan.notes.join("; ")));
                    }
                    self.playtest.message = Some(message);
                }
                TestEvent::Failed(error) => {
                    self.playtest.job = None;
                    self.playtest.readying = None;
                    self.playtest.message = None;
                    self.error_message = Some(format!("The test could not start: {error}"));
                }
            }
        }
    }
}
