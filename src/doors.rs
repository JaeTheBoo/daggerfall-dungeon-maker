//! The action doors Daggerfall Unity switches off because they overlap another door.
//!
//! After laying a dungeon out, Daggerfall Unity (`DaggerfallDungeon.RemoveOverlappingDoors`)
//! goes through every action door, cell by cell in the dungeon's order and in
//! each cell in the order of its object lists. It keeps a list of door
//! positions, starting with the exit door, and switches off each door whose
//! position is within [`OVERLAP_DISTANCE`] of one in the list; the doors it
//! keeps join the list. Two cells with a door each on the edge they share end
//! up with one door, for example.
//!
//! An action door's position is its model's origin. The exit door's is the
//! middle of its door face, placed as if its cell were at the dungeon's
//! origin: the game leaves out where the cell is in the dungeon, which this
//! copies so the doors switched off are the game's.

use std::collections::HashMap;

use glam::Vec3;

use crate::arena2::{Arena2Library, DoorKind};
use crate::export::object_groups;
use crate::scene::{SceneObject, SceneObjectKind, ACTION_DOOR_TAGS, RED_BRICK_DOOR_MODEL_ID};

/// How close (in metres) a door may be to another before the game switches it off.
pub const OVERLAP_DISTANCE: f32 = 1.4;

/// Whether the game lays `object` out as an action door (one that opens and
/// closes): a model tagged as a door, except the red brick door.
pub fn is_action_door(object: &SceneObject, model_tags: &HashMap<u32, String>) -> bool {
    match object.kind {
        SceneObjectKind::Model { model_id, .. } => {
            model_id != RED_BRICK_DOOR_MODEL_ID
                && model_tags.get(&model_id).is_some_and(|tag| ACTION_DOOR_TAGS.contains(&tag.as_str()))
        }
        _ => false,
    }
}

/// The indexes of the action doors among a cell's `objects`, in the order the game goes through them.
pub fn action_doors_in_order(objects: &[SceneObject], model_tags: &HashMap<u32, String>) -> Vec<usize> {
    object_groups(objects)
        .into_iter()
        .flatten()
        .filter(|&index| is_action_door(&objects[index], model_tags))
        .collect()
}

/// The point the game puts in its list for the start cell's exit door: the
/// middle of the first exit door face among `objects`, in the cell's own
/// coordinates (which the game uses as the dungeon's).
pub fn exit_door_point(objects: &[SceneObject], library: &mut Arena2Library) -> Option<Vec3> {
    object_groups(objects).into_iter().flatten().find_map(|index| {
        let object = &objects[index];
        let SceneObjectKind::Model { model_id, .. } = object.kind else { return None };
        let mesh = library.model(model_id)?;
        let face = mesh
            .submeshes
            .iter()
            .find(|submesh| DoorKind::from_texture(submesh.texture) == Some(DoorKind::DungeonExit))?;
        let (low, high) = face.vertices.iter().fold((Vec3::MAX, Vec3::MIN), |(low, high), vertex| {
            (low.min(vertex.position), high.max(vertex.position))
        });
        Some(object.model_matrix().transform_point3((low + high) * 0.5))
    })
}

/// Which of `doors` (each a key and its position in the dungeon, in the game's
/// order) the game switches off, given the exit door's point.
pub fn switched_off<K: Copy>(exit: Option<Vec3>, doors: impl IntoIterator<Item = (K, Vec3)>) -> Vec<K> {
    let mut kept: Vec<Vec3> = exit.into_iter().collect();
    let mut off = Vec::new();
    for (key, position) in doors {
        if kept.iter().any(|other| other.distance(position) < OVERLAP_DISTANCE) {
            off.push(key);
        } else {
            kept.push(position);
        }
    }
    off
}

/// One cell of a dungeon, for [`dungeon_switched_off`].
pub struct DoorCell<'cell> {
    pub objects: &'cell [SceneObject],
    /// Where the cell's corner is in the dungeon.
    pub origin: Vec3,
    pub is_start: bool,
}

/// The doors the game switches off in a dungeon made of `cells` (in the
/// dungeon's order), as (cell, object index) pairs.
pub fn dungeon_switched_off(
    cells: &[DoorCell<'_>],
    model_tags: &HashMap<u32, String>,
    library: &mut Arena2Library,
) -> Vec<(usize, usize)> {
    let exit =
        cells.iter().find(|cell| cell.is_start).and_then(|cell| exit_door_point(cell.objects, library));
    let doors = cells.iter().enumerate().flat_map(|(cell_index, cell)| {
        action_doors_in_order(cell.objects, model_tags)
            .into_iter()
            .map(move |index| ((cell_index, index), cell.origin + cell.objects[index].position))
    });
    switched_off(exit, doors.collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_first_of_overlapping_doors() {
        let doors = [
            ("a", Vec3::new(0.0, 0.0, 0.0)),
            ("b", Vec3::new(1.0, 0.0, 0.0)),  // within 1.4 m of a
            ("c", Vec3::new(2.25, 0.0, 0.0)), // 2.25 m from a: kept, as the game's flush double doors
            ("d", Vec3::new(10.0, 0.0, 0.0)), // near the exit door
        ];
        let off = switched_off(Some(Vec3::new(10.5, 0.0, 0.0)), doors);
        assert_eq!(off, ["b", "d"]);
        // A door switched off does not switch off the doors near it.
        let chain = [("a", Vec3::ZERO), ("b", Vec3::new(1.0, 0.0, 0.0)), ("c", Vec3::new(2.0, 0.0, 0.0))];
        assert_eq!(switched_off(None, chain), ["b"]);
    }
}
