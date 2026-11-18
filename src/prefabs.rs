//! Prefabs: groups of objects the user saved, to place them again as one in any cell.
//!
//! A prefab is made from objects selected in a cell (the cell editor's
//! **Create Prefab**). Its objects are kept around its *base*: the middle of
//! the bottom of the box around them, which rests on whatever the prefab is
//! placed on. Prefabs belong to the user, like their favourite assets: they
//! are saved with the application's settings, not in a project, so every
//! project can use them.
//!
//! The objects of a placed prefab are marked with a [`PrefabMember`], so the
//! editor keeps them together until the user breaks the prefab up. Deleting a
//! prefab from the menu leaves the copies already placed as they are.

use std::collections::{HashMap, HashSet};

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::project::ObjectFile;
use crate::scene::{ObjectId, PrefabMember, SceneObject, SceneObjectKind, ADDED_OBJECTS_GROUP};

/// Identifies a prefab in the library while the program runs.
pub type PrefabId = u64;

/// A saved group of objects.
#[derive(Debug, Clone, PartialEq)]
pub struct Prefab {
    pub id: PrefabId,
    pub name: String,
    /// Its objects, around its base at the origin. Their ids only serve the
    /// action links between them; placing the prefab gives them new ones.
    pub objects: Vec<SceneObject>,
}

impl Prefab {
    /// Copies of its objects with its base at `base`, turned by `yaw_degrees`
    /// around the vertical, numbered from `first_id` (the links between them
    /// follow) and marked as the placed prefab `instance`.
    pub fn place(&self, base: Vec3, yaw_degrees: f32, first_id: ObjectId, instance: u32) -> Vec<SceneObject> {
        let turn = Quat::from_rotation_y(yaw_degrees.to_radians());
        let ids: HashMap<ObjectId, ObjectId> =
            self.objects.iter().zip(first_id..).map(|(object, id)| (object.id, id)).collect();
        self.objects
            .iter()
            .map(|object| {
                let mut placed = SceneObject {
                    id: ids[&object.id],
                    next: object.next.and_then(|next| ids.get(&next).copied()),
                    position: base + turn * object.position,
                    prefab: Some(PrefabMember { instance, name: self.name.clone() }),
                    ..object.clone()
                };
                // Flats always face the camera and lights have no direction, so only models turn.
                if yaw_degrees != 0.0 && matches!(object.kind, SceneObjectKind::Model { .. }) {
                    placed.set_rotation(turn * object.rotation());
                }
                placed
            })
            .collect()
    }

    /// The model drawn on its tile in the asset palette: its biggest, by `size_of`.
    pub fn largest_model(&self, mut size_of: impl FnMut(u32) -> f32) -> Option<u32> {
        self.objects
            .iter()
            .filter_map(|object| match object.kind {
                SceneObjectKind::Model { model_id, .. } => Some((model_id, size_of(model_id))),
                _ => None,
            })
            .max_by(|first, second| first.1.total_cmp(&second.1))
            .map(|(model_id, _)| model_id)
    }

    /// Its name as shown: as typed, without surrounding spaces, or "Unnamed prefab".
    pub fn label(&self) -> &str {
        match self.name.trim() {
            "" => "Unnamed prefab",
            name => name,
        }
    }
}

/// The user's prefabs, in the order they were made.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrefabLibrary {
    prefabs: Vec<Prefab>,
    next_id: PrefabId,
}

impl PrefabLibrary {
    /// Every prefab, oldest first.
    pub fn prefabs(&self) -> &[Prefab] {
        &self.prefabs
    }

    /// The prefab `id`, if it is still in the library.
    pub fn get(&self, id: PrefabId) -> Option<&Prefab> {
        self.prefabs.iter().find(|prefab| prefab.id == id)
    }

    /// Adds a prefab called `name` made of `objects`, as they are placed in a
    /// cell, with its base at `base`. Links to objects left out are dropped,
    /// and the objects lose any prefab they were part of. Returns its id.
    pub fn add(&mut self, name: &str, objects: &[SceneObject], base: Vec3) -> PrefabId {
        let ids: HashSet<ObjectId> = objects.iter().map(|object| object.id).collect();
        let objects = objects
            .iter()
            .map(|object| SceneObject {
                position: object.position - base,
                next: object.next.filter(|next| ids.contains(next)),
                group_index: ADDED_OBJECTS_GROUP,
                prefab: None,
                ..object.clone()
            })
            .collect();
        self.push(name.trim().to_string(), objects)
    }

    /// The names of the prefabs holding model `model_id`.
    pub fn prefabs_using_model(&self, model_id: u32) -> Vec<&str> {
        self.prefabs
            .iter()
            .filter(|prefab| prefab.objects.iter().any(|object| object.is_model(model_id)))
            .map(|prefab| prefab.name.as_str())
            .collect()
    }

    /// Removes every copy of model `model_id` from the prefabs (and the links
    /// to them), and the prefabs left empty. Returns how many copies were removed.
    pub fn remove_model(&mut self, model_id: u32) -> usize {
        let mut removed = 0;
        for prefab in &mut self.prefabs {
            let gone: HashSet<ObjectId> = prefab
                .objects
                .iter()
                .filter(|object| object.is_model(model_id))
                .map(|object| object.id)
                .collect();
            prefab.objects.retain(|object| !object.is_model(model_id));
            for object in &mut prefab.objects {
                if object.next.is_some_and(|next| gone.contains(&next)) {
                    object.next = None;
                }
            }
            removed += gone.len();
        }
        self.prefabs.retain(|prefab| !prefab.objects.is_empty());
        removed
    }

    /// Removes the prefab `id` from the library. Copies already placed stay.
    pub fn remove(&mut self, id: PrefabId) -> Option<Prefab> {
        let position = self.prefabs.iter().position(|prefab| prefab.id == id)?;
        Some(self.prefabs.remove(position))
    }

    /// Renames the prefab `id`. The name is kept as typed, so it can be
    /// edited in a text field; copies already placed keep their name.
    pub fn rename(&mut self, id: PrefabId, name: &str) {
        if let Some(prefab) = self.prefabs.iter_mut().find(|prefab| prefab.id == id) {
            prefab.name = name.to_string();
        }
    }

    /// A name for a new prefab: "Prefab 1", "Prefab 2", …, the first not taken.
    pub fn new_name(&self) -> String {
        (1..)
            .map(|number| format!("Prefab {number}"))
            .find(|name| !self.prefabs.iter().any(|prefab| prefab.name.trim().eq_ignore_ascii_case(name)))
            .expect("an unused name always exists")
    }

    /// The library as text (JSON), to save with the application's settings.
    pub fn to_text(&self) -> String {
        let file = LibraryFile {
            prefabs: self
                .prefabs
                .iter()
                .map(|prefab| PrefabFile {
                    name: prefab.name.clone(),
                    objects: prefab.objects.iter().map(ObjectFile::from_object).collect(),
                })
                .collect(),
        };
        serde_json::to_string(&file).expect("prefabs always serialise")
    }

    /// Reads a library saved by [`Self::to_text`].
    pub fn from_text(text: &str) -> Result<Self, String> {
        let file: LibraryFile = serde_json::from_str(text).map_err(|error| error.to_string())?;
        let mut library = Self::default();
        for prefab in file.prefabs {
            library.push(prefab.name, prefab.objects.into_iter().map(ObjectFile::into_object).collect());
        }
        Ok(library)
    }

    /// Adds a prefab and returns its new id.
    fn push(&mut self, name: String, objects: Vec<SceneObject>) -> PrefabId {
        self.next_id += 1;
        self.prefabs.push(Prefab { id: self.next_id, name, objects });
        self.next_id
    }
}

#[derive(Serialize, Deserialize)]
struct LibraryFile {
    prefabs: Vec<PrefabFile>,
}

#[derive(Serialize, Deserialize)]
struct PrefabFile {
    name: String,
    objects: Vec<ObjectFile>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{LightData, ModelData, ObjectAction};

    fn model(id: ObjectId, position: Vec3, next: Option<ObjectId>) -> SceneObject {
        SceneObject {
            id,
            name: format!("Model {id}"),
            group_index: 3,
            kind: SceneObjectKind::Model {
                model_id: 55000 + id,
                data: ModelData { action: next.map(|_| ObjectAction::default()), ..ModelData::default() },
            },
            position,
            rotation_degrees: Vec3::ZERO,
            visible: true,
            next,
            scale: None,
            prefab: Some(PrefabMember { instance: 4, name: "Older".to_string() }),
            folder: None,
        }
    }

    fn light(id: ObjectId, position: Vec3) -> SceneObject {
        SceneObject {
            kind: SceneObjectKind::Light { radius: 5.0, data: LightData::default() },
            next: None,
            ..model(id, position, None)
        }
    }

    #[test]
    fn removing_a_model_takes_its_copies_out_of_prefabs() {
        let mut library = PrefabLibrary::default();
        // Object 7 (model 55007) triggers object 8 (model 55008).
        let gate = library.add("Gate", &[model(7, Vec3::ZERO, Some(8)), model(8, Vec3::X, None)], Vec3::ZERO);
        library.add("Lever", &[model(8, Vec3::ZERO, None)], Vec3::ZERO);
        assert_eq!(library.prefabs_using_model(55008), ["Gate", "Lever"]);

        assert_eq!(library.remove_model(55008), 2);
        // The gate keeps its other object, no longer linked; the lever, left empty, is gone.
        assert_eq!(library.prefabs().len(), 1);
        let gate = library.get(gate).unwrap();
        assert_eq!(gate.objects.len(), 1);
        assert_eq!(gate.objects[0].next, None);
        assert!(library.prefabs_using_model(55008).is_empty());
    }

    #[test]
    fn a_prefab_keeps_its_objects_around_its_base() {
        let mut library = PrefabLibrary::default();
        // Object 7 triggers object 8, which is in the prefab, and object 8 a lever left out of it.
        let objects =
            [model(7, Vec3::new(10.0, 1.0, 20.0), Some(8)), model(8, Vec3::new(12.0, 1.0, 20.0), Some(99))];
        let id = library.add(" Gate ", &objects, Vec3::new(11.0, 1.0, 20.0));
        let prefab = library.get(id).unwrap();
        assert_eq!(prefab.name, "Gate");
        assert_eq!(prefab.objects[0].position, Vec3::new(-1.0, 0.0, 0.0));
        assert_eq!(prefab.objects[0].next, Some(8));
        assert_eq!(prefab.objects[1].next, None);
        assert!(prefab.objects.iter().all(|object| object.prefab.is_none()));
        assert!(prefab.objects.iter().all(|object| object.group_index == ADDED_OBJECTS_GROUP));
    }

    #[test]
    fn placing_turns_the_prefab_and_numbers_its_objects() {
        let mut library = PrefabLibrary::default();
        let objects = [model(7, Vec3::new(1.0, 0.0, 0.0), Some(8)), light(8, Vec3::new(0.0, 2.0, 0.0))];
        let id = library.add("Torch", &objects, Vec3::ZERO);
        let placed = library.get(id).unwrap().place(Vec3::new(5.0, 1.0, 5.0), 90.0, 40, 2);
        // A quarter turn takes +X to -Z.
        assert!(placed[0].position.distance(Vec3::new(5.0, 1.0, 4.0)) < 1e-5, "{:?}", placed[0].position);
        assert_eq!(placed[0].rotation_degrees, Vec3::new(0.0, 90.0, 0.0));
        // The light does not turn.
        assert_eq!(placed[1].rotation_degrees, Vec3::ZERO);
        assert_eq!((placed[0].id, placed[0].next, placed[1].id), (40, Some(41), 41));
        let member = PrefabMember { instance: 2, name: "Torch".to_string() };
        assert!(placed.iter().all(|object| object.prefab.as_ref() == Some(&member)));
    }

    #[test]
    fn the_library_is_saved_and_read_back() {
        let mut library = PrefabLibrary::default();
        library.add("Gate", &[model(1, Vec3::ONE, Some(2)), light(2, Vec3::ZERO)], Vec3::ZERO);
        let removed = library.add("Old", &[light(1, Vec3::ZERO)], Vec3::ZERO);
        library.add("Pillar", &[model(1, Vec3::ZERO, None)], Vec3::ZERO);
        assert!(library.remove(removed).is_some());
        library.rename(1, "Iron gate");
        assert_eq!(library.new_name(), "Prefab 1");

        let loaded = PrefabLibrary::from_text(&library.to_text()).unwrap();
        let names: Vec<&str> = loaded.prefabs().iter().map(|prefab| prefab.name.as_str()).collect();
        assert_eq!(names, ["Iron gate", "Pillar"]);
        assert_eq!(loaded.prefabs()[0].objects, library.prefabs()[0].objects);
        assert!(PrefabLibrary::from_text("not a library").is_err());
    }
}
