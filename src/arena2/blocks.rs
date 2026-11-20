//! Parser for RDB dungeon blocks stored in `BLOCKS.BSA`.
//!
//! Each dungeon in Daggerfall is assembled from square blocks of 2048 native
//! units. An RDB block contains a table of model identifiers followed by
//! several linked lists of objects: 3D models, flats (billboard sprites,
//! including editor markers for enemies, treasure and quest locations) and
//! light sources.

use std::collections::HashSet;

use glam::IVec3;

use super::error::{Arena2Error, Arena2Result};
use super::reader::ByteReader;
use super::TextureKey;

/// Number of entries in the model reference table of every RDB block.
const MODEL_REFERENCE_COUNT: usize = 750;
/// Size of one model reference entry: a five character id and a three character tag.
const MODEL_REFERENCE_SIZE: usize = 8;
/// Size of the block header that precedes the model reference table.
const BLOCK_HEADER_SIZE: usize = 20;
/// Safety limit for linked list traversal, far above anything found in game data.
const MAX_OBJECTS_PER_LIST: usize = 100_000;

/// Resource type tag for a 3D model object.
const RESOURCE_TYPE_MODEL: u8 = 0x01;
/// Resource type tag for a light object.
const RESOURCE_TYPE_LIGHT: u8 = 0x02;
/// Resource type tag for a flat (billboard) object.
const RESOURCE_TYPE_FLAT: u8 = 0x03;

/// A fully parsed RDB dungeon block.
#[derive(Debug, Clone)]
pub struct RdbBlock {
    /// Block file name, for example `N0000004.RDB`.
    pub name: String,
    /// Every object found in the block, in file order.
    pub objects: Vec<RdbObject>,
}

/// An object placed inside an RDB block.
#[derive(Debug, Clone)]
pub struct RdbObject {
    /// Offset of the object's record in the block; actions link objects by it.
    pub offset: i32,
    /// Index of the object list this object came from. Lists roughly correspond to rooms.
    pub group_index: usize,
    /// Position in native Daggerfall units relative to the block origin.
    pub position: IVec3,
    /// Type specific data.
    pub kind: RdbObjectKind,
}

/// The type specific part of an [`RdbObject`].
#[derive(Debug, Clone)]
pub enum RdbObjectKind {
    /// A static or animated 3D model.
    Model(RdbModel),
    /// A billboard sprite or editor marker.
    Flat(RdbFlat),
    /// A point light.
    Light(RdbLight),
}

/// Model specific object data.
#[derive(Debug, Clone)]
pub struct RdbModel {
    /// Record identifier of the model in `ARCH3D.BSA`.
    pub model_id: u32,
    /// Rotation around each axis in native units (2048 units per full turn).
    pub rotation: IVec3,
    /// What sets the model's action off (low bits) and, for doors, the starting lock (bits 4 and up).
    pub trigger_and_lock: u32,
    /// Sound played by the action.
    pub sound: u8,
    /// The model's action (door, lever, moving platform and so on), if it has one.
    pub action: Option<RdbAction>,
}

/// An action attached to a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RdbAction {
    /// Direction of movement (1-6), or an amount for some action types.
    pub axis: u8,
    pub duration: u16,
    pub magnitude: u16,
    /// Offset of the object the action triggers next, or negative for none.
    pub next_offset: i32,
    /// What the action does (move, rotate, open a door, teleport, hurt, …).
    pub action_type: u8,
}

/// Flat specific object data.
#[derive(Debug, Clone)]
pub struct RdbFlat {
    /// Texture shown by the billboard.
    pub texture: TextureKey,
    /// Raw flag bits (gender for enemies, behaviour for some markers).
    pub flags: u16,
    /// For fixed enemy markers, the enemy (in the low byte); for other flats, a faction id.
    pub faction_or_mobile_id: u16,
    /// Action byte; 99 makes a fixed enemy passive.
    pub action: u8,
    /// Where its resource record is in the block's data, which seeds the
    /// name the game makes up for a person.
    pub resource_offset: u32,
    /// Offset of the object this flat's action triggers next, or negative for none.
    pub next_offset: i32,
}

/// Light specific object data.
#[derive(Debug, Clone)]
pub struct RdbLight {
    /// Light radius in native units.
    pub radius: u16,
    /// Two values of unknown meaning, kept as read.
    pub unknown_1: u32,
    pub unknown_2: u32,
}

/// The base fields shared by every object in a linked list.
struct ObjectHeader {
    position: IVec3,
    resource_type: u8,
    resource_offset: usize,
}

/// Parses the RDB block called `name` from its raw bytes.
///
/// The eight cells Daggerfall Unity corrects when it reads them (see
/// [`unity_fix_description`]) come back corrected, as the game shows them.
pub fn parse_rdb_block(name: &str, data: &[u8]) -> Arena2Result<RdbBlock> {
    let mut header = ByteReader::new(data);
    let _unknown = header.read_u32()?;
    let width = header.read_u32()? as usize;
    let height = header.read_u32()? as usize;
    let object_root_offset = header.read_u32()? as usize;

    let model_ids = read_model_reference_table(data)?;

    let mut root_reader = ByteReader::at(data, object_root_offset);
    let mut lists = Vec::with_capacity(width * height);
    for group_index in 0..width * height {
        let root_offset = root_reader.read_i32()?;
        let list = match usize::try_from(root_offset) {
            Ok(root_offset) => read_object_list(data, root_offset, group_index, &model_ids)?,
            Err(_) => Vec::new(), // A negative root marks an empty group.
        };
        lists.push(list);
    }

    let mut lists = ObjectLists { lists, model_ids, next_added_offset: data.len() as i32 };
    lists.fix_like_daggerfall_unity(name);
    let objects = lists.lists.into_iter().flatten().flatten().collect();
    Ok(RdbBlock { name: name.to_string(), objects })
}

/// What Daggerfall Unity corrects in the cell called `name` when it reads
/// it, or `None` if it reads the cell as it is. The editor shows these cells
/// corrected too, and a copy of one keeps the corrections (Daggerfall Unity
/// does not correct a cell a mod replaces).
pub fn unity_fix_description(name: &str) -> Option<&'static str> {
    UNITY_FIXES.iter().find(|(cell, _)| cell.eq_ignore_ascii_case(name)).map(|(_, description)| *description)
}

/// The cells Daggerfall Unity corrects (`BlocksFile.FixRdbData`), and what it corrects in them.
const UNITY_FIXES: [(&str, &str); 8] = [
    ("N0000022.RDB", "removes a door that stands in the wrong place"),
    ("N0000023.RDB", "removes a door that stands in the wrong place"),
    ("N0000035.RDB", "puts a door at the right height"),
    ("N0000052.RDB", "moves a lever to its proper place"),
    ("N0000071.RDB", "joins the western dead end to the rest of the cell with a long corridor"),
    (
        "W0000009.RDB",
        "moves the exit to another corner, with a wall to close it when the cell is not the start",
    ),
    ("W0000018.RDB", "joins the upper rooms together and swaps a wrong door model"),
    ("W0000020.RDB", "replaces the lower western dead end with a corridor and stairs that join the rest"),
];

/// The red brick wall door Daggerfall Unity adds to `W0000009.RDB`.
const BRICK_WALL_DOOR_MODEL_ID: u32 = 72100;

/// A block's object lists while it is read. Each list keeps an entry for
/// every object in the file, in file order, with `None` for objects that are
/// skipped, so objects can be found by their place in the list as Daggerfall
/// Unity counts them.
struct ObjectLists {
    lists: Vec<Vec<Option<RdbObject>>>,
    model_ids: Vec<Option<u32>>,
    /// Offset for the next object added by a correction: past the end of the
    /// block, so it is never another object's.
    next_added_offset: i32,
}

impl ObjectLists {
    /// Makes the corrections Daggerfall Unity makes to the cell `name`.
    fn fix_like_daggerfall_unity(&mut self, name: &str) {
        match name.to_ascii_uppercase().as_str() {
            "N0000071.RDB" => {
                self.set_model(4, 18, 10, None);
                self.move_to(4, 18, Some(640), None, None);
                self.set_model(5, 8, 11, None);
            }
            "N0000022.RDB" | "N0000023.RDB" => {
                if let Some(list) = self.lists.get_mut(2) {
                    list.clear();
                }
            }
            "N0000035.RDB" => self.move_to(0, 36, None, Some(-300), None),
            "N0000052.RDB" => self.move_to(0, 24, Some(543), None, None),
            "W0000009.RDB" => {
                // The wall takes the first empty slot of the model table.
                let wall_slot = self.model_ids.iter().position(Option::is_none);
                if let Some(slot) = wall_slot {
                    self.model_ids[slot] = Some(BRICK_WALL_DOOR_MODEL_ID);
                }
                let Some(corner) = self.object(5, 39).map(|object| object.position) else { return };
                let (x, y, z) = (corner.x + 64, corner.y, corner.z - 64);
                self.move_to(5, 39, Some(x), None, Some(z));
                self.set_model(5, 39, 35, Some(-512));
                self.move_to(8, 0, Some(x), None, Some(z + 126)); // The exit.
                self.move_to(4, 5, Some(x), None, Some(z)); // The start marker.
                if let Some(slot) = wall_slot {
                    self.add_model(8, IVec3::new(x, y, z + 128), slot, -512);
                }
            }
            "W0000018.RDB" => {
                self.set_model(4, 38, 10, Some(-512));
                self.set_model(5, 40, 10, Some(-2560));
                self.add_model(6, IVec3::new(1152, -1792, 1920), 11, -512);
                self.set_model(2, 0, 23, None);
            }
            "W0000020.RDB" => {
                self.set_model(4, 15, 5, None);
                self.set_model(5, 34, 7, Some(-512));
                self.add_model(5, IVec3::new(1152, -384, 1408), 4, 512);
            }
            _ => {}
        }
    }

    /// The object at place `index` of list `list`, if it is there and was read.
    fn object(&mut self, list: usize, index: usize) -> Option<&mut RdbObject> {
        let object = self.lists.get_mut(list)?.get_mut(index)?.as_mut();
        if object.is_none() {
            log::warn!("cannot correct object {index} of list {list}: it is not in the cell");
        }
        object
    }

    /// Moves an object along the axes given (native units).
    fn move_to(&mut self, list: usize, index: usize, x: Option<i32>, y: Option<i32>, z: Option<i32>) {
        if let Some(object) = self.object(list, index) {
            let position = &mut object.position;
            position.x = x.unwrap_or(position.x);
            position.y = y.unwrap_or(position.y);
            position.z = z.unwrap_or(position.z);
        }
    }

    /// Makes a model object show the model in slot `slot` of the model
    /// table, and turns it to `y_rotation` (native units) if one is given.
    fn set_model(&mut self, list: usize, index: usize, slot: usize, y_rotation: Option<i32>) {
        let Some(model_id) = self.model_ids.get(slot).copied().flatten() else {
            log::warn!("cannot correct object {index} of list {list}: model slot {slot} is empty");
            return;
        };
        if let Some(RdbObject { kind: RdbObjectKind::Model(model), .. }) = self.object(list, index) {
            model.model_id = model_id;
            if let Some(y_rotation) = y_rotation {
                model.rotation.y = y_rotation;
            }
        }
    }

    /// Adds a model with no action to the end of list `list`.
    fn add_model(&mut self, list: usize, position: IVec3, slot: usize, y_rotation: i32) {
        let Some(model_id) = self.model_ids.get(slot).copied().flatten() else { return };
        let Some(objects) = self.lists.get_mut(list) else { return };
        objects.push(Some(RdbObject {
            offset: self.next_added_offset,
            group_index: list,
            position,
            kind: RdbObjectKind::Model(RdbModel {
                model_id,
                rotation: IVec3::new(0, y_rotation, 0),
                trigger_and_lock: 0,
                sound: 0,
                action: None,
            }),
        }));
        self.next_added_offset += 1;
    }
}

/// Reads the model reference table of an RDB block: each `ARCH3D.BSA` model
/// id with the three letter tag the block gives it (for example `DOR` for an
/// opening door). Daggerfall Unity decides some behaviour, such as which
/// models are doors, by these tags.
pub fn read_model_tags(data: &[u8]) -> Arena2Result<Vec<(u32, String)>> {
    let mut reader = ByteReader::at(data, BLOCK_HEADER_SIZE);
    let mut tags = Vec::new();
    for _ in 0..MODEL_REFERENCE_COUNT {
        let entry = reader.read_bytes(MODEL_REFERENCE_SIZE)?;
        let id_text: String =
            entry[..5].iter().take_while(|&&byte| byte != 0).map(|&byte| byte as char).collect();
        if let Ok(model_id) = id_text.trim().parse::<u32>() {
            let tag: String = entry[5..].iter().map(|&byte| byte as char).collect();
            tags.push((model_id, tag));
        }
    }
    Ok(tags)
}

/// Reads the table that maps a model index to an `ARCH3D.BSA` model id.
fn read_model_reference_table(data: &[u8]) -> Arena2Result<Vec<Option<u32>>> {
    let mut reader = ByteReader::at(data, BLOCK_HEADER_SIZE);
    (0..MODEL_REFERENCE_COUNT)
        .map(|_| {
            let entry = reader.read_bytes(MODEL_REFERENCE_SIZE)?;
            let id_text: String =
                entry[..5].iter().take_while(|&&byte| byte != 0).map(|&byte| byte as char).collect();
            Ok(id_text.trim().parse::<u32>().ok())
        })
        .collect()
}

/// Walks one linked list of objects. Returns an entry for every object in
/// the list, in order: `None` for objects that are skipped.
fn read_object_list(
    data: &[u8],
    root_offset: usize,
    group_index: usize,
    model_ids: &[Option<u32>],
) -> Arena2Result<Vec<Option<RdbObject>>> {
    let mut objects = Vec::new();
    let mut visited_offsets = HashSet::new();
    let mut next_offset = Some(root_offset);

    while let Some(offset) = next_offset {
        if !visited_offsets.insert(offset) || visited_offsets.len() > MAX_OBJECTS_PER_LIST {
            return Err(Arena2Error::format("RDB object list contains a loop"));
        }

        let mut reader = ByteReader::at(data, offset);
        let next = reader.read_i32()?;
        let _previous = reader.read_i32()?;
        let object_header = ObjectHeader {
            position: IVec3::new(reader.read_i32()?, reader.read_i32()?, reader.read_i32()?),
            resource_type: reader.read_u8()?,
            resource_offset: reader.read_u32()? as usize,
        };

        let kind = read_object_resource(data, &object_header, model_ids)?;
        objects.push(kind.map(|kind| RdbObject {
            offset: offset as i32,
            group_index,
            position: object_header.position,
            kind,
        }));

        next_offset = usize::try_from(next).ok();
    }
    Ok(objects)
}

/// Reads the type specific resource data of an object.
///
/// Returns `Ok(None)` for objects that should be skipped, such as models that
/// reference an empty slot of the model table.
fn read_object_resource(
    data: &[u8],
    object_header: &ObjectHeader,
    model_ids: &[Option<u32>],
) -> Arena2Result<Option<RdbObjectKind>> {
    let mut reader = ByteReader::at(data, object_header.resource_offset);

    let kind = match object_header.resource_type {
        RESOURCE_TYPE_MODEL => {
            let rotation = IVec3::new(reader.read_i32()?, reader.read_i32()?, reader.read_i32()?);
            let model_index = usize::from(reader.read_u16()?);
            let trigger_and_lock = reader.read_u32()?;
            let sound = reader.read_u8()?;
            let action_offset = reader.read_i32()?;

            let Some(model_id) = model_ids.get(model_index).copied().flatten() else {
                log::warn!("RDB model object references empty model slot {model_index}");
                return Ok(None);
            };
            let action = match usize::try_from(action_offset) {
                Ok(action_offset) if action_offset > 0 => {
                    let mut action_reader = ByteReader::at(data, action_offset);
                    Some(RdbAction {
                        axis: action_reader.read_u8()?,
                        duration: action_reader.read_u16()?,
                        magnitude: action_reader.read_u16()?,
                        next_offset: action_reader.read_i32()?,
                        action_type: action_reader.read_u8()?,
                    })
                }
                _ => None,
            };
            RdbObjectKind::Model(RdbModel { model_id, rotation, trigger_and_lock, sound, action })
        }
        RESOURCE_TYPE_FLAT => {
            let resource_offset = reader.position() as u32;
            let texture = TextureKey::from_bitfield(reader.read_u16()?);
            let flags = reader.read_u16()?;
            let faction_or_mobile_id = reader.read_u16()?;
            let next_offset = reader.read_i32()?;
            let action = reader.read_u8()?;
            RdbObjectKind::Flat(RdbFlat {
                texture,
                flags,
                faction_or_mobile_id,
                action,
                next_offset,
                resource_offset,
            })
        }
        RESOURCE_TYPE_LIGHT => {
            let unknown_1 = reader.read_u32()?;
            let unknown_2 = reader.read_u32()?;
            let radius = reader.read_u16()?;
            RdbObjectKind::Light(RdbLight { radius, unknown_1, unknown_2 })
        }
        other => {
            log::warn!("skipping RDB object with unknown resource type {other}");
            return Ok(None);
        }
    };
    Ok(Some(kind))
}
