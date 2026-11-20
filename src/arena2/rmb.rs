//! Parser for RMB blocks stored in `BLOCKS.BSA`: the above-ground part of a
//! location (town quarters, castles, ruins, dungeon entrances).
//!
//! An RMB block is a square of 4096 native units. It holds a fixed size header
//! (positions and data of its buildings, ground tiles, scenery, automap), then
//! one record per building with the building's outside and inside objects,
//! then loose ("misc") models and flats. What is seen from outside is placed
//! as Daggerfall Unity's `RMBLayout` does, in world units relative to the
//! block's corner: the buildings' outside models and flats, the loose models
//! and flats (with their markers, such as where fast travel puts the player)
//! and the nature scenery. Each building's inside is kept as stored, in its
//! own coordinates, to be laid out as `DaggerfallInterior` does (see
//! [`crate::buildings`]).

use glam::{IVec3, Quat, Vec3};

use super::error::{Arena2Error, Arena2Result};
use super::maps::LocationBuilding;
use super::reader::ByteReader;
use super::{TextureKey, NATIVE_ROTATION_UNITS_PER_DEGREE, WORLD_SCALE};

/// Width and depth of an RMB block, in native units.
pub const RMB_BLOCK_SIDE_NATIVE: f32 = 4096.0;
/// Width and depth of an RMB block, in world units.
pub const RMB_BLOCK_SIDE: f32 = RMB_BLOCK_SIDE_NATIVE * WORLD_SCALE;

/// Most building records an RMB block can have.
const MAX_BUILDINGS: usize = 32;
/// Size of one building position entry: two unknown values, x, z and rotation.
const BUILDING_POSITION_SIZE: usize = 20;
/// Size of one building data entry.
const BUILDING_DATA_SIZE: usize = 26;
/// Number of ground tiles along each side of a block.
const GROUND_TILES: usize = 16;
/// Side of one ground tile, in native units.
const GROUND_TILE_SIDE: f32 = 256.0;
/// Side of the town map image of a block, in pixels (64 native units each).
pub const AUTOMAP_SIDE: usize = 64;
/// Size of the automap image.
const AUTOMAP_SIZE: usize = AUTOMAP_SIDE * AUTOMAP_SIDE;
/// Length of the block's name and of each of its buildings' other names.
const NAME_LENGTH: usize = 13;
/// Size of one model record.
const MODEL_RECORD_SIZE: usize = 66;
/// Size of one flat record, and of one person record.
const FLAT_RECORD_SIZE: usize = 17;
/// Size of one "section 3" record (spawn points inside buildings).
const SECTION3_RECORD_SIZE: usize = 16;
/// Size of one door record.
const DOOR_RECORD_SIZE: usize = 19;
/// Size of a building's object header.
const OBJECT_HEADER_SIZE: usize = 17;
/// Flats of this archive only mark things for the original editor.
const EDITOR_FLATS_ARCHIVE: u16 = 199;
/// Nature flats (trees, rocks) come in one archive per climate, 500 to 511.
const NATURE_ARCHIVES: std::ops::RangeInclusive<u16> = 500..=511;
/// The nature archive used in previews: temperate woodland.
pub const PREVIEW_NATURE_ARCHIVE: u16 = 504;
/// Vertical offsets Daggerfall Unity applies, in native units.
const PROPS_OFFSET_Y: f32 = -4.0;
const BLOCK_FLATS_OFFSET_Y: f32 = -6.0;
const NATURE_FLATS_OFFSET_Y: f32 = -2.0;

/// An RMB block: its outside, and each building's inside.
#[derive(Debug, Clone, Default)]
pub struct RmbBlock {
    pub name: String,
    pub models: Vec<RmbModel>,
    pub flats: Vec<RmbFlat>,
    /// Its ground: 16 × 16 tiles, as stored (rows from the north, each from
    /// the west), drawn with the ground textures of the location's climate.
    pub ground: Vec<GroundTile>,
    /// Its buildings, in the order the block lists them.
    pub buildings: Vec<RmbBuilding>,
    /// Its town map, as the game shows it: 64 × 64 bytes, rows from the
    /// north, each from the west; 0 where nothing is drawn, else the type of
    /// the building there plus 1 (see [`AUTOMAP_SIDE`]).
    pub automap: Vec<u8>,
}

impl RmbBlock {
    /// The type of each of its buildings (Daggerfall Unity's `BuildingTypes` numbers).
    pub fn building_types(&self) -> Vec<u8> {
        self.buildings.iter().map(|building| building.data.building_type).collect()
    }
}

/// One building of an RMB block.
#[derive(Debug, Clone, Default)]
pub struct RmbBuilding {
    /// The building's data as the block gives it. A location's own building
    /// list replaces the name seed, faction and quality of its named
    /// buildings (see [`crate::buildings::settlement_buildings`]).
    pub data: LocationBuilding,
    /// The file name the original editor gave the building, for example `KRAVE01.HS2`.
    pub other_name: String,
    /// Its outside models, as a range of [`RmbBlock::models`]; the first is the building itself.
    pub models: std::ops::Range<usize>,
    /// Its inside.
    pub interior: RmbInterior,
}

impl RmbBuilding {
    /// Whether the building has an inside to enter.
    pub fn has_interior(&self) -> bool {
        !self.interior.models.is_empty()
    }
}

/// The inside of a building, as stored: positions in native units in the
/// interior's own space (Y down), rotations in degrees as the editor's
/// objects store them.
#[derive(Debug, Clone, Default)]
pub struct RmbInterior {
    pub models: Vec<InteriorModel>,
    /// Flats: furniture, lights, and editor markers (archive 199) such as
    /// where the player enters or where quests put people and items.
    pub flats: Vec<InteriorFlat>,
    /// The people: shopkeepers, guild members, quest givers.
    pub people: Vec<InteriorFlat>,
    /// Doors that open and close inside the building.
    pub doors: Vec<InteriorDoor>,
    /// Points where the game can place enemies inside ("section 3" records).
    pub spawn_points: Vec<IVec3>,
}

/// A model inside a building.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteriorModel {
    pub model_id: u32,
    /// Position as stored, in native units.
    pub native_position: IVec3,
    pub yaw_degrees: f32,
    /// Whether it is a prop (furniture, clutter): its position is its
    /// bottom's, as Daggerfall Unity reads it, rather than its origin's.
    pub is_prop: bool,
}

/// A flat or person inside a building.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InteriorFlat {
    pub texture: TextureKey,
    /// Position as stored, in native units.
    pub native_position: IVec3,
    /// For people, their faction (see `FACTION.TXT`); 0 for none.
    pub faction_id: u16,
    /// For people, bit 32 makes a woman.
    pub flags: u8,
    /// Where its record is in the block's data, which seeds the name the
    /// game makes up for a person.
    pub record_offset: u32,
}

/// A door inside a building.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteriorDoor {
    /// Position as stored, in native units.
    pub native_position: IVec3,
    pub yaw_degrees: f32,
    /// Which of the five interior door models it is (Daggerfall Unity uses model 9000 + this, modulo 5).
    pub model_index: u8,
}

/// One tile of an RMB block's ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GroundTile {
    /// Record in the climate's ground texture archive.
    pub record: u8,
    /// Whether the texture is turned a quarter round.
    pub rotated: bool,
    /// Whether the texture is turned upside down.
    pub flipped: bool,
}

impl GroundTile {
    fn from_byte(byte: u8) -> Self {
        Self { record: byte & 0x3f, rotated: byte & 0x40 != 0, flipped: byte & 0x80 != 0 }
    }
}

/// A model placed in an RMB block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RmbModel {
    pub model_id: u32,
    /// World position relative to the block's corner.
    pub position: Vec3,
    /// Rotation around the vertical axis, in degrees, as the editor's objects store it.
    pub yaw_degrees: f32,
}

/// A flat placed in an RMB block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RmbFlat {
    pub texture: TextureKey,
    /// World position of the flat's base (it stands on this point), relative to the block's corner.
    pub base_position: Vec3,
    /// The faction of a person outdoors (Daggerfall Unity makes a flat with
    /// one a person to talk to); 0 for none.
    pub faction_id: u16,
    pub flags: u8,
    /// Where its record is in the block's data, which seeds the name the
    /// game makes up for a person; 0 for scenery made from the ground.
    pub record_offset: u32,
}

/// One model record as stored in the file.
struct ModelRecord {
    model_id: u32,
    object_type: u8,
    /// Position in native units, as stored (Y down).
    native: IVec3,
    rotation: f32,
}

impl ModelRecord {
    /// The position in native units with Y flipped up.
    fn position(&self) -> Vec3 {
        Vec3::new(self.native.x as f32, -self.native.y as f32, self.native.z as f32)
    }
}

/// One flat (or person) record as stored in the file.
struct FlatRecord {
    texture: TextureKey,
    native: IVec3,
    faction_id: u16,
    flags: u8,
    /// Where the record is in the block's data.
    offset: u32,
}

impl FlatRecord {
    /// The position in native units with Y flipped up.
    fn position(&self) -> Vec3 {
        Vec3::new(self.native.x as f32, -self.native.y as f32, self.native.z as f32)
    }
}

/// The objects of one side (outside or inside) of a building record.
#[derive(Default)]
struct Subrecord {
    models: Vec<ModelRecord>,
    flats: Vec<FlatRecord>,
    spawn_points: Vec<IVec3>,
    people: Vec<FlatRecord>,
    doors: Vec<InteriorDoor>,
}

/// Prop models: their stored position is their bottom's.
const PROP_OBJECT_TYPE: u8 = 3;

/// Parses the RMB block called `name` from its `BLOCKS.BSA` record.
pub fn parse_rmb_block(name: &str, data: &[u8]) -> Arena2Result<RmbBlock> {
    let mut reader = ByteReader::new(data);
    let building_count = usize::from(reader.read_u8()?);
    let misc_model_count = usize::from(reader.read_u8()?);
    let misc_flat_count = usize::from(reader.read_u8()?);
    if building_count > MAX_BUILDINGS {
        return Err(Arena2Error::format(format!("{name} has {building_count} buildings")));
    }

    let mut building_positions = Vec::with_capacity(building_count);
    for index in 0..MAX_BUILDINGS {
        reader.skip(8);
        let x = reader.read_i32()? as f32;
        let z = reader.read_i32()? as f32;
        let rotation = reader.read_i32()? as f32;
        if index < building_count {
            building_positions.push((x, z, rotation));
        }
    }
    debug_assert_eq!(BUILDING_POSITION_SIZE, 20);
    let building_data = reader.read_bytes(MAX_BUILDINGS * BUILDING_DATA_SIZE)?;
    let building_data: Vec<LocationBuilding> = (0..building_count)
        .map(|building| read_building_data(&building_data[building * BUILDING_DATA_SIZE..]))
        .collect::<Arena2Result<_>>()?;
    reader.skip(MAX_BUILDINGS * 4); // Unknown section.
    let mut building_sizes = [0usize; MAX_BUILDINGS];
    for size in &mut building_sizes {
        *size = reader.read_i32()?.max(0) as usize;
    }
    reader.skip(8); // Ground header.
    let ground = reader
        .read_bytes(GROUND_TILES * GROUND_TILES)?
        .iter()
        .map(|&byte| GroundTile::from_byte(byte))
        .collect();
    let scenery = reader.read_bytes(GROUND_TILES * GROUND_TILES)?;
    let automap = reader.read_bytes(AUTOMAP_SIZE)?.to_vec();
    reader.skip(NAME_LENGTH); // The block's own name.
    let names = reader.read_bytes(MAX_BUILDINGS * NAME_LENGTH)?;
    let other_name = |building: usize| {
        let field = &names[building * NAME_LENGTH..(building + 1) * NAME_LENGTH];
        field.iter().take_while(|&&byte| byte != 0).map(|&byte| char::from(byte)).collect::<String>()
    };

    let mut block = RmbBlock { name: name.to_string(), ground, automap, ..RmbBlock::default() };
    let to_world = |native: Vec3| native * WORLD_SCALE;

    // Buildings: their outside models turn with the building, their flats do not.
    let mut position = reader.position();
    for (index, &(building_x, building_z, building_rotation)) in building_positions.iter().enumerate() {
        let end = position + building_sizes[index];
        let (outside, inside_start) = read_subrecord(data, position, end)?;
        // A building without room for an inside after its outside has none.
        let inside = match inside_start + OBJECT_HEADER_SIZE <= end {
            true => {
                read_subrecord(data, inside_start, end).map(|(inside, _)| inside).unwrap_or_else(|error| {
                    log::warn!("{name}: the inside of building {index} does not read: {error}");
                    Subrecord::default()
                })
            }
            false => Subrecord::default(),
        };

        let building_yaw = -building_rotation / NATIVE_ROTATION_UNITS_PER_DEGREE;
        let turn = Quat::from_rotation_y(building_yaw.to_radians());
        let origin = to_world(Vec3::new(building_x, 0.0, RMB_BLOCK_SIDE_NATIVE - building_z));
        let first_model = block.models.len();
        for record in &outside.models {
            block.models.push(RmbModel {
                model_id: record.model_id,
                position: origin + turn * to_world(record.position()),
                yaw_degrees: building_yaw - record.rotation / NATIVE_ROTATION_UNITS_PER_DEGREE,
            });
        }
        let flats_origin = to_world(Vec3::new(building_x, 0.0, -building_z));
        for record in &outside.flats {
            // Daggerfall Unity has no use for the markers outside buildings.
            if record.texture.archive == EDITOR_FLATS_ARCHIVE {
                continue;
            }
            let native = record.position() + Vec3::new(0.0, BLOCK_FLATS_OFFSET_Y, RMB_BLOCK_SIDE_NATIVE);
            block.flats.push(RmbFlat {
                texture: preview_texture(record.texture),
                base_position: flats_origin + to_world(native),
                faction_id: record.faction_id,
                flags: record.flags,
                record_offset: record.offset,
            });
        }
        block.buildings.push(RmbBuilding {
            data: building_data[index],
            other_name: other_name(index),
            models: first_model..block.models.len(),
            interior: interior_of(inside),
        });
        position = end;
    }

    // Loose models and flats, with the markers among them (where fast travel puts the player).
    let mut misc = ByteReader::at(data, position);
    for _ in 0..misc_model_count {
        let record = read_model_record(&mut misc)?;
        let native = record.position() + Vec3::new(0.0, PROPS_OFFSET_Y, RMB_BLOCK_SIDE_NATIVE);
        block.models.push(RmbModel {
            model_id: record.model_id,
            position: to_world(native),
            yaw_degrees: -record.rotation / NATIVE_ROTATION_UNITS_PER_DEGREE,
        });
    }
    for _ in 0..misc_flat_count {
        let record = read_flat_record(&mut misc)?;
        let native = record.position() + Vec3::new(0.0, BLOCK_FLATS_OFFSET_Y, RMB_BLOCK_SIDE_NATIVE);
        block.flats.push(RmbFlat {
            texture: preview_texture(record.texture),
            base_position: to_world(native),
            faction_id: record.faction_id,
            flags: record.flags,
            record_offset: record.offset,
        });
    }

    // Nature scenery: one tree or rock per marked ground tile. Rows are stored north first.
    for tile_y in 0..GROUND_TILES {
        for tile_x in 0..GROUND_TILES {
            let value = scenery[(GROUND_TILES - 1 - tile_y) * GROUND_TILES + tile_x];
            if value == 255 {
                continue;
            }
            let record = i32::from(value) / 4 - 1;
            if record < 1 {
                continue;
            }
            let native = Vec3::new(
                tile_x as f32 * GROUND_TILE_SIDE,
                NATURE_FLATS_OFFSET_Y,
                tile_y as f32 * GROUND_TILE_SIDE + GROUND_TILE_SIDE,
            );
            block.flats.push(RmbFlat {
                texture: TextureKey::new(PREVIEW_NATURE_ARCHIVE, record as u8),
                base_position: to_world(native),
                faction_id: 0,
                flags: 0,
                record_offset: 0,
            });
        }
    }
    Ok(block)
}

/// Reads one building data entry of the block's header.
fn read_building_data(bytes: &[u8]) -> Arena2Result<LocationBuilding> {
    let mut reader = ByteReader::new(bytes);
    let name_seed = reader.read_u16()?;
    reader.skip(16); // Service time limit, then four values of unknown meaning.
    let faction_id = reader.read_u16()?;
    let sector = reader.read_i16()?;
    let _location_id = reader.read_u16()?;
    let building_type = reader.read_u8()?;
    let quality = reader.read_u8()?;
    Ok(LocationBuilding { name_seed, faction_id, sector, building_type, quality })
}

/// Reads one side of a building record starting at `start`, which must end
/// by `end`. Returns it and where the next part starts.
fn read_subrecord(data: &[u8], start: usize, end: usize) -> Arena2Result<(Subrecord, usize)> {
    let mut reader = ByteReader::at(data, start);
    let header = reader.read_bytes(OBJECT_HEADER_SIZE)?;
    let counts = [header[0], header[1], header[2], header[3], header[4]].map(usize::from);
    let [models, flats, section3, people, doors] = counts;
    let size = OBJECT_HEADER_SIZE
        + models * MODEL_RECORD_SIZE
        + flats * FLAT_RECORD_SIZE
        + section3 * SECTION3_RECORD_SIZE
        + people * FLAT_RECORD_SIZE
        + doors * DOOR_RECORD_SIZE;
    if start + size > end.min(data.len()) {
        return Err(Arena2Error::format("objects past the end of the building record"));
    }
    let mut subrecord = Subrecord::default();
    for _ in 0..models {
        subrecord.models.push(read_model_record(&mut reader)?);
    }
    for _ in 0..flats {
        subrecord.flats.push(read_flat_record(&mut reader)?);
    }
    for _ in 0..section3 {
        let point = IVec3::new(reader.read_i32()?, reader.read_i32()?, reader.read_i32()?);
        reader.skip(4);
        subrecord.spawn_points.push(point);
    }
    for _ in 0..people {
        subrecord.people.push(read_flat_record(&mut reader)?);
    }
    for _ in 0..doors {
        let native_position = IVec3::new(reader.read_i32()?, reader.read_i32()?, reader.read_i32()?);
        let rotation = f32::from(reader.read_i16()?);
        let _open_rotation = reader.read_i16()?;
        let model_index = reader.read_u8()?;
        reader.skip(2);
        subrecord.doors.push(InteriorDoor {
            native_position,
            yaw_degrees: -rotation / NATIVE_ROTATION_UNITS_PER_DEGREE,
            model_index,
        });
    }
    Ok((subrecord, start + size))
}

/// A building's inside from its record.
fn interior_of(inside: Subrecord) -> RmbInterior {
    let flat = |record: &FlatRecord| InteriorFlat {
        texture: record.texture,
        native_position: record.native,
        faction_id: record.faction_id,
        flags: record.flags,
        record_offset: record.offset,
    };
    RmbInterior {
        models: inside
            .models
            .iter()
            .map(|record| InteriorModel {
                model_id: record.model_id,
                native_position: record.native,
                yaw_degrees: -record.rotation / NATIVE_ROTATION_UNITS_PER_DEGREE,
                is_prop: record.object_type == PROP_OBJECT_TYPE,
            })
            .collect(),
        flats: inside.flats.iter().map(flat).collect(),
        people: inside.people.iter().map(flat).collect(),
        doors: inside.doors,
        spawn_points: inside.spawn_points,
    }
}

/// Nature flats of any climate are shown with the preview climate's.
fn preview_texture(texture: TextureKey) -> TextureKey {
    match NATURE_ARCHIVES.contains(&texture.archive) {
        true => TextureKey::new(PREVIEW_NATURE_ARCHIVE, texture.record),
        false => texture,
    }
}

/// Reads one model record: its id, type, stored position and rotation.
fn read_model_record(reader: &mut ByteReader<'_>) -> Arena2Result<ModelRecord> {
    let start = reader.position();
    let id_high = i32::from(reader.read_i16()?);
    let id_low = i32::from(reader.read_u8()?);
    let object_type = reader.read_u8()?;
    reader.skip(12 + 8 + 12); // Unknowns, null, first position.
    let native = IVec3::new(reader.read_i32()?, reader.read_i32()?, reader.read_i32()?);
    reader.skip(4);
    let rotation = f32::from(reader.read_i16()?);
    reader.seek(start + MODEL_RECORD_SIZE);
    let model_id = u32::try_from(id_high * 100 + id_low)
        .map_err(|_| Arena2Error::format(format!("invalid model id {id_high}{id_low:02}")))?;
    Ok(ModelRecord { model_id, object_type, native, rotation })
}

/// Reads one flat (or person) record: its texture, stored position, faction and flags.
fn read_flat_record(reader: &mut ByteReader<'_>) -> Arena2Result<FlatRecord> {
    let start = reader.position();
    let native = IVec3::new(reader.read_i32()?, reader.read_i32()?, reader.read_i32()?);
    let texture = TextureKey::from_bitfield(reader.read_u16()?);
    let faction_id = reader.read_i16()? as u16;
    let flags = reader.read_u8()?;
    reader.seek(start + FLAT_RECORD_SIZE);
    Ok(FlatRecord { texture, native, faction_id, flags, offset: start as u32 })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a model record.
    fn model(id_high: i16, id_low: u8, object_type: u8, x: i32, y: i32, z: i32, rotation: i16) -> Vec<u8> {
        let mut record = Vec::new();
        record.extend_from_slice(&id_high.to_le_bytes());
        record.push(id_low);
        record.push(object_type);
        record.extend(std::iter::repeat_n(0, 12 + 8 + 12));
        for value in [x, y, z] {
            record.extend_from_slice(&value.to_le_bytes());
        }
        record.extend_from_slice(&[0; 4]);
        record.extend_from_slice(&rotation.to_le_bytes());
        record.extend(std::iter::repeat_n(0, MODEL_RECORD_SIZE - record.len()));
        record
    }

    /// Builds a flat or person record.
    fn flat(archive: u16, record: u8, x: i32, y: i32, z: i32, faction_id: i16) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [x, y, z] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&((archive << 7) | u16::from(record)).to_le_bytes());
        bytes.extend_from_slice(&faction_id.to_le_bytes());
        bytes.push(0);
        bytes
    }

    /// Builds an RMB record with one tavern (one model and one flat outside;
    /// inside a model, a prop, a quest marker, a person and a door), one loose
    /// model and a start marker.
    fn sample_block() -> Vec<u8> {
        let mut data = vec![1u8, 1, 1];
        for index in 0..MAX_BUILDINGS {
            let (x, z, rotation) = if index == 0 { (1000i32, 1096i32, 512i32) } else { (0, 0, 0) };
            data.extend_from_slice(&[0; 8]);
            data.extend_from_slice(&x.to_le_bytes());
            data.extend_from_slice(&z.to_le_bytes());
            data.extend_from_slice(&rotation.to_le_bytes());
        }
        let mut building_data = vec![0u8; MAX_BUILDINGS * BUILDING_DATA_SIZE];
        building_data[..2].copy_from_slice(&321u16.to_le_bytes());
        building_data[18..20].copy_from_slice(&42u16.to_le_bytes());
        building_data[24] = 15; // A tavern.
        building_data[25] = 12;
        data.extend(building_data);
        data.extend(std::iter::repeat_n(0, MAX_BUILDINGS * 4));

        let mut outside = vec![0u8; OBJECT_HEADER_SIZE];
        outside[0] = 1;
        outside[1] = 1;
        outside.extend(model(5, 7, 0, 40, -80, 0, 0));
        outside.extend(flat(210, 3, 0, 0, 0, 0));
        let mut inside = vec![0u8; OBJECT_HEADER_SIZE];
        inside[..5].copy_from_slice(&[2, 1, 1, 1, 1]);
        inside.extend(model(31, 0, 0, 100, 0, 200, 512));
        inside.extend(model(410, 11, PROP_OBJECT_TYPE, 300, 64, 400, 0));
        inside.extend(flat(199, 11, 10, 20, 30, 0));
        inside.extend([7u8; 12].iter().chain(&[1, 0, 0, 0])); // A spawn point.
        inside.extend(flat(182, 5, 50, 0, 60, 140));
        let mut door = Vec::new();
        for value in [70i32, 0, 80] {
            door.extend_from_slice(&value.to_le_bytes());
        }
        door.extend_from_slice(&1024i16.to_le_bytes());
        door.extend_from_slice(&0i16.to_le_bytes());
        door.extend_from_slice(&[3, 0, 0]);
        inside.extend(door);
        let building_size = outside.len() + inside.len();
        for index in 0..MAX_BUILDINGS {
            let size = if index == 0 { building_size as i32 } else { 0 };
            data.extend_from_slice(&size.to_le_bytes());
        }
        data.extend(std::iter::repeat_n(0, 8 + 256));
        let mut scenery = vec![255u8; 256];
        scenery[15 * 16 + 2] = 4 * 3; // Tile (2, 0): record 2.
        data.extend_from_slice(&scenery);
        data.extend(std::iter::repeat_n(0, AUTOMAP_SIZE + NAME_LENGTH));
        let mut names = vec![0u8; MAX_BUILDINGS * NAME_LENGTH];
        names[..11].copy_from_slice(b"KRAVE01.HS2");
        data.extend(names);

        data.extend(outside);
        data.extend(inside);
        data.extend(model(410, 11, 0, 0, 0, 0, 1024));
        data.extend(flat(199, 10, 500, 0, -500, 0));
        data
    }

    #[test]
    fn places_buildings_props_and_scenery_as_daggerfall_unity_does() {
        let block = parse_rmb_block("TEST.RMB", &sample_block()).unwrap();
        assert_eq!(block.models.len(), 2);

        // The building is a quarter turn round (512 units = 90°); its model sits 40 units east of it.
        let building = block.models[0];
        assert_eq!(building.model_id, 507);
        assert!((building.yaw_degrees + 90.0).abs() < 0.01);
        let expected = Vec3::new(1000.0, 0.0, 3000.0) * WORLD_SCALE
            + Quat::from_rotation_y((-90.0f32).to_radians()) * (Vec3::new(40.0, 80.0, 0.0) * WORLD_SCALE);
        assert!(building.position.distance(expected) < 0.001, "{:?}", building.position);

        let prop = block.models[1];
        assert_eq!(prop.model_id, 41011);
        assert!((prop.yaw_degrees + 180.0).abs() < 0.01);
        assert!(prop.position.distance(Vec3::new(0.0, -4.0, 4096.0) * WORLD_SCALE) < 0.001);

        // The building's flat, the loose start marker, then the scenery.
        assert_eq!(block.flats.len(), 3);
        assert_eq!(block.flats[0].texture, TextureKey::new(210, 3));
        assert!(block.flats[0].base_position.distance(Vec3::new(1000.0, -6.0, 3000.0) * WORLD_SCALE) < 0.001);
        assert_eq!(block.flats[1].texture, TextureKey::new(199, 10));
        assert!(block.flats[1].base_position.distance(Vec3::new(500.0, -6.0, 3596.0) * WORLD_SCALE) < 0.001);
        assert_eq!(block.flats[2].texture, TextureKey::new(PREVIEW_NATURE_ARCHIVE, 2));
        assert!(block.flats[2].base_position.distance(Vec3::new(512.0, -2.0, 256.0) * WORLD_SCALE) < 0.001);
    }

    #[test]
    fn reads_each_buildings_data_and_inside() {
        let block = parse_rmb_block("TEST.RMB", &sample_block()).unwrap();
        assert_eq!(block.building_types(), [15]);
        let building = &block.buildings[0];
        assert_eq!(building.data.name_seed, 321);
        assert_eq!(building.data.faction_id, 42);
        assert_eq!(building.data.quality, 12);
        assert_eq!(building.other_name, "KRAVE01.HS2");
        assert_eq!(building.models, 0..1);
        assert!(building.has_interior());

        let inside = &building.interior;
        assert_eq!(inside.models.len(), 2);
        assert_eq!(inside.models[0].model_id, 3100);
        assert_eq!(inside.models[0].native_position, IVec3::new(100, 0, 200));
        assert!((inside.models[0].yaw_degrees + 90.0).abs() < 0.01);
        assert!(!inside.models[0].is_prop && inside.models[1].is_prop);
        assert_eq!(inside.flats[0].texture, TextureKey::new(199, 11));
        assert_eq!(inside.spawn_points, [IVec3::splat(0x0707_0707)]);
        assert_eq!(inside.people[0].texture, TextureKey::new(182, 5));
        assert_eq!(inside.people[0].faction_id, 140);
        assert_eq!(inside.doors[0].model_index, 3);
        assert!((inside.doors[0].yaw_degrees + 180.0).abs() < 0.01);
    }
}
