//! Outdoor cells (RMB blocks) as the editor shows them: their buildings,
//! scenery and flats, standing on their ground, drawn with the ground tiles,
//! trees and rocks of a climate as Daggerfall Unity draws them.
//!
//! The ground is a model the program makes for each cell and climate (see
//! [`Arena2Library::generated_model`]): one square per ground tile, textured
//! with the tile's record of the climate's ground archive. As Daggerfall Unity
//! reads them, the tiles are stored in rows from the north, each from the
//! west, with each image's top to the north; a tile marked rotated is turned a
//! quarter round anticlockwise, and one marked flipped is turned half round
//! (`TerrainHelper.SetLocationTiles` and the `DaggerfallTilemapTextureArray`
//! shader). Records past the archive's mark blend space around walled cities,
//! where the land around shows: it is drawn as the climate's plain ground.
//!
//! The models are drawn with the textures of the climate and season, as
//! Daggerfall Unity swaps them (see [`ClimateTextures`]); the ground and the
//! trees take the climate's archives, with snow in winter.

use std::collections::BTreeMap;

use glam::{Vec2, Vec3};

use crate::arena2::{
    Arena2Library, Climate, GroundTile, MeshVertex, ModelMesh, RmbBlock, Season, SubMesh, TextureKey,
    RMB_BLOCK_SIDE,
};
use crate::scene::{FlatData, ModelData, Scene, SceneObject, SceneObjectKind};

/// Ground tiles along each side of a cell.
const GROUND_TILES: usize = 16;
/// Side of one ground tile, in world units.
const GROUND_TILE_SIDE: f32 = RMB_BLOCK_SIDE / GROUND_TILES as f32;
/// Height of the ground, a little below the cell's floor so buildings' floors are not hidden in it.
const GROUND_HEIGHT: f32 = -0.02;
/// Side of a ground tile's image, in texels, when it cannot be read.
const DEFAULT_TILE_TEXELS: f32 = 64.0;
/// The record of a ground archive that is the climate's plain ground (grass, or sand in the desert).
const PLAIN_GROUND_RECORD: u8 = 2;
/// The nature archive RMB blocks are read with, which the climate's replaces.
const READ_NATURE_ARCHIVE: u16 = crate::arena2::PREVIEW_NATURE_ARCHIVE;

/// The objects of the outdoor cell `block` in `climate` and `season`, in
/// the cell's own coordinates (its south-west corner at the origin): its
/// ground first, then its models and flats. Every object gets `group_index`.
/// (Its models' textures are swapped for the climate's when drawn, with
/// [`ClimateTextures`].)
pub fn cell_scene(
    block: &RmbBlock,
    climate: Climate,
    season: Season,
    group_index: usize,
    library: &mut Arena2Library,
) -> Scene {
    let (ground_archive, nature_archive) =
        (climate.ground_archive_in(season), climate.nature_archive_in(season));
    let mut objects = Vec::with_capacity(1 + block.models.len() + block.flats.len());
    let mut add = |name: String, kind: SceneObjectKind, position: Vec3, yaw_degrees: f32| {
        objects.push(SceneObject {
            id: objects.len() as u32 + 1,
            name,
            group_index,
            kind,
            position,
            rotation_degrees: Vec3::new(0.0, yaw_degrees, 0.0),
            visible: true,
            next: None,
            scale: None,
            prefab: None,
            folder: None,
        });
    };

    if block.ground.len() == GROUND_TILES * GROUND_TILES {
        let key = format!("ground {} {ground_archive}", block.name);
        let ground_id = library
            .generated_model(&key, |library, id| ground_mesh(id, &block.ground, ground_archive, library));
        add(
            "Ground".to_string(),
            SceneObjectKind::Model { model_id: ground_id, data: ModelData::default() },
            Vec3::ZERO,
            0.0,
        );
    }
    for model in &block.models {
        add(
            format!("Model {}", model.model_id),
            SceneObjectKind::Model { model_id: model.model_id, data: ModelData::default() },
            model.position,
            model.yaw_degrees,
        );
    }
    for flat in &block.flats {
        let texture = match flat.texture.archive {
            READ_NATURE_ARCHIVE => TextureKey::new(nature_archive, flat.texture.record),
            _ => flat.texture,
        };
        // Flats stand on their position; the renderer centres them on it.
        let height = library.billboard_world_size(texture).map_or(0.0, |size| size.y);
        let name = match (texture.archive, flat.faction_id) {
            (crate::scene::MARKER_TEXTURE_ARCHIVE, _)
                if texture.record == crate::scene::START_MARKER_RECORD =>
            {
                "Start marker".to_string()
            }
            (crate::scene::MARKER_TEXTURE_ARCHIVE, _) => format!("Marker {}", texture.record),
            // A flat with a faction is a person the game lets the player talk to.
            (_, 0) => format!("Flat {}.{}", texture.archive, texture.record),
            _ => format!("Person {}.{}", texture.archive, texture.record),
        };
        let data =
            FlatData { flags: u16::from(flat.flags), faction_or_mobile_id: flat.faction_id, action: 0 };
        add(
            name,
            SceneObjectKind::Flat { texture, data },
            flat.base_position + Vec3::new(0.0, height / 2.0, 0.0),
            0.0,
        );
    }
    Scene { block_name: block.name.clone(), objects }
}

/// The textures Daggerfall Unity draws an outdoor model with in a climate and
/// season: each texture swapped for the climate's variant of its set, and in
/// winter for the snowy one where there is one, as `ClimateSwaps.ApplyClimate`
/// does (Daggerfall Unity 1.1.1).
///
/// A texture archive's last two digits are its set (a castle's walls, a
/// tavern's roof…) and the hundreds its climate's base (see [`Climate::base`]);
/// the archive after a set's is its winter variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClimateTextures {
    /// The climate's base: [`Climate::DESERT`], [`Climate::MOUNTAIN`], [`Climate::TEMPERATE`] or [`Climate::SWAMP`].
    pub base: u16,
    pub winter: bool,
}

impl ClimateTextures {
    pub fn new(climate: Climate, season: Season) -> Self {
        Self { base: climate.base, winter: season == Season::Winter }
    }

    /// The texture the game draws in place of `key` on an outdoor model.
    pub fn apply(&self, key: TextureKey) -> TextureKey {
        let (archive, record) = (key.archive, key.record);
        // The third building door is never swapped, as in classic.
        if archive % 100 == DOORS_SET && record == 3 {
            return key;
        }
        let Some((set, mut has_winter)) = climate_texture_set(archive) else { return key };
        // Swamps have no marble floors of their own.
        if self.base == Climate::SWAMP && set == MARBLE_FLOORS_SET {
            return key;
        }
        // Deserts are never drawn in winter.
        if self.base == Climate::DESERT {
            has_winter = false;
        }
        // Sets missing some winter textures.
        if (self.base == Climate::SWAMP && set == MAGES_GUILD_SET) || (set == CASTLE_SET && record > 3) {
            has_winter = false;
        }
        if (archive == 82 && record > 1) || archive == 77 {
            has_winter = false;
        }
        // These have a winter variant but keep their own climate.
        let keeps_climate = matches!(archive, 75 | 76 | 77 | 79 | 80 | 82 | 83);
        let mut swapped = match archive < 500 && !keeps_climate {
            true => self.base + set,
            false => archive,
        };
        if self.winter && has_winter {
            swapped += 1;
        }
        TextureKey { archive: swapped, ..key }
    }
}

/// Whether the game swaps `object`'s textures for its location's climate:
/// Daggerfall's models, not the user's imported ones or the ground the program makes.
pub fn uses_climate_textures(object: &SceneObject) -> bool {
    match object.kind {
        SceneObjectKind::Model { model_id, .. } => !crate::custom_models::is_custom_model(model_id),
        _ => false,
    }
}

/// Texture sets (`DFLocation.ClimateTextureSet`) the climate swaps treat specially.
const DOORS_SET: u16 = 74;
const MARBLE_FLOORS_SET: u16 = 41;
const MAGES_GUILD_SET: u16 = 35;
const CASTLE_SET: u16 = 9;

/// The climate texture set of `archive` and whether the set has a winter
/// variant (`ClimateSwaps.GetClimateTextureInfo`), or `None` if the archive
/// is not swapped by climate.
fn climate_texture_set(archive: u16) -> Option<(u16, bool)> {
    if archive > 499 {
        // Nature sets.
        return match archive {
            500..=503 => Some((archive, false)),
            504 | 506 | 508 | 510 => Some((archive, true)),
            _ => None,
        };
    }
    let set = archive % 100;
    match set {
        // Terrain.
        2 => Some((set, true)),
        // Outside sets with a winter variant: ruins, castle, city A and B, city walls, farm,
        // fences, Mages Guild, manor, merchant homes, taverns, temples, village, roofs, city
        // specials A and B (and their snowy sets, as the game lists them).
        7 | 9 | 12 | 14 | 17 | 26 | 29 | 35 | 38 | 42 | 58 | 61 | 64 | 69 | 79 | 80 | 82 | 83 => {
            Some((set, true))
        }
        // Doors.
        DOORS_SET => Some((set, false)),
        // Inside sets.
        11 | 16 | 19 | 20 | 22..=25 | 28 | 37 | 40 | 41 | 44 | 45 | 47 | 48 | 60 | 63 | 66 | 68 => {
            Some((set, false))
        }
        _ => None,
    }
}

/// The ground of a cell: a square for each of its `tiles` (as an RMB block
/// stores them), textured from `archive`, facing up.
fn ground_mesh(model_id: u32, tiles: &[GroundTile], archive: u16, library: &mut Arena2Library) -> ModelMesh {
    let mut submeshes: BTreeMap<u8, SubMesh> = BTreeMap::new();
    let mut sizes: BTreeMap<u8, Vec2> = BTreeMap::new();
    let records = library.texture_record_count(archive);
    for (index, tile) in tiles.iter().enumerate() {
        // Rows are stored from the north; the cell's z grows to the north.
        let (column, row_from_north) = (index % GROUND_TILES, index / GROUND_TILES);
        let row = GROUND_TILES - 1 - row_from_north;
        let (west, south) = (column as f32 * GROUND_TILE_SIDE, row as f32 * GROUND_TILE_SIDE);
        let (east, north) = (west + GROUND_TILE_SIDE, south + GROUND_TILE_SIDE);
        // A record past the archive's (63, in a few cells) leaves the land
        // around showing: the climate's plain ground.
        let record = match usize::from(tile.record) < records {
            true => tile.record,
            false => PLAIN_GROUND_RECORD,
        };
        let texture = TextureKey::new(archive, record);
        let size = *sizes.entry(record).or_insert_with(|| {
            library.texture_image(texture).map_or(Vec2::splat(DEFAULT_TILE_TEXELS), |image| {
                Vec2::new(image.width as f32, image.height as f32)
            })
        });
        let submesh = submeshes.entry(record).or_insert_with(|| SubMesh {
            texture,
            vertices: Vec::new(),
            indices: Vec::new(),
        });
        let first = submesh.vertices.len() as u32;
        // Corners as the tile is seen from above: north-west, north-east, south-east, south-west,
        // each with where it is across (east) and down (south) the tile.
        let corners = [
            (Vec3::new(west, GROUND_HEIGHT, north), Vec2::new(0.0, 0.0)),
            (Vec3::new(east, GROUND_HEIGHT, north), Vec2::new(1.0, 0.0)),
            (Vec3::new(east, GROUND_HEIGHT, south), Vec2::new(1.0, 1.0)),
            (Vec3::new(west, GROUND_HEIGHT, south), Vec2::new(0.0, 1.0)),
        ];
        for (position, across) in corners {
            submesh.vertices.push(MeshVertex {
                position,
                normal: Vec3::Y,
                uv_texels: tile_image_point(*tile, across) * size,
            });
        }
        // South-west, north-east, south-east and south-west, north-west, north-east: facing up.
        submesh.indices.extend([first + 3, first + 1, first + 2, first + 3, first, first + 1]);
    }
    let side = RMB_BLOCK_SIDE;
    ModelMesh {
        model_id,
        submeshes: submeshes.into_values().collect(),
        bounds_min: Vec3::new(0.0, GROUND_HEIGHT, 0.0),
        bounds_max: Vec3::new(side, GROUND_HEIGHT, side),
    }
}

/// The point of a tile's image (0 to 1 across and down it) seen at `point`
/// of the tile (0 to 1 across to the east and down to the south), once the
/// tile is turned as marked.
fn tile_image_point(tile: GroundTile, point: Vec2) -> Vec2 {
    let Vec2 { x: across, y: down } = point;
    match (tile.rotated, tile.flipped) {
        (false, false) => point,
        // A quarter turn anticlockwise.
        (true, false) => Vec2::new(1.0 - down, across),
        // Half round.
        (false, true) => Vec2::new(1.0 - across, 1.0 - down),
        // Both: three quarters round anticlockwise.
        (true, true) => Vec2::new(down, 1.0 - across),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swaps_textures_for_the_climate_as_daggerfall_unity_does() {
        let swap = |climate: u8, season, archive, record| {
            ClimateTextures::new(Climate::from_value(climate), season)
                .apply(TextureKey::new(archive, record))
                .archive
        };
        let (desert, mountain, swamp, woodlands) = (224, 226, 228, 231);
        use Season::{Summer, Winter};
        // A temperate castle wall (set 9) in each climate, and with snow in winter.
        assert_eq!(swap(desert, Summer, 309, 0), 9);
        assert_eq!(swap(mountain, Summer, 309, 0), 109);
        assert_eq!(swap(woodlands, Winter, 309, 0), 310);
        // Deserts have no winter; castles' later records have no snow.
        assert_eq!(swap(desert, Winter, 309, 0), 9);
        assert_eq!(swap(woodlands, Winter, 309, 4), 309);
        // Mountain woods have a temperate base, though their ground is the mountains'.
        assert_eq!(swap(230, Summer, 14, 0), 314);
        // Doors take the climate, without snow; the third door never changes.
        assert_eq!(swap(swamp, Winter, 374, 0), 474);
        assert_eq!(swap(swamp, Summer, 374, 3), 374);
        // Swamps keep marble floors, and their Mages Guilds have no snow.
        assert_eq!(swap(swamp, Summer, 341, 0), 341);
        assert_eq!(swap(swamp, Winter, 335, 0), 435);
        // Gables (75 to 77) are not swapped; city specials keep their climate but get snow.
        assert_eq!(swap(desert, Winter, 76, 0), 76);
        assert_eq!(swap(woodlands, Winter, 79, 0), 80);
        assert_eq!(swap(woodlands, Winter, 82, 2), 82);
        // Textures of no climate set, and the user's own, stay as they are.
        assert_eq!(swap(desert, Winter, 186, 0), 186);
        assert_eq!(swap(desert, Winter, 0xF001, 0), 0xF001);
    }

    #[test]
    fn grounds_and_trees_have_snow_in_winter_except_in_deserts() {
        let woodlands = Climate::DEFAULT;
        assert_eq!(
            (woodlands.ground_archive_in(Season::Winter), woodlands.nature_archive_in(Season::Winter)),
            (303, 505)
        );
        let desert = Climate::from_value(224);
        assert_eq!(
            (desert.ground_archive_in(Season::Winter), desert.nature_archive_in(Season::Winter)),
            (2, 503)
        );
        // The rainforest's trees have no snow, though its swamp ground does.
        let rainforest = Climate::from_value(227);
        assert_eq!(
            (rainforest.ground_archive_in(Season::Winter), rainforest.nature_archive_in(Season::Winter)),
            (403, 500)
        );
    }

    #[test]
    fn turns_ground_tiles_as_daggerfall_unity_does() {
        let tile = |rotated, flipped| GroundTile { record: 0, rotated, flipped };
        let top_right = Vec2::new(1.0, 0.0);
        assert_eq!(tile_image_point(tile(false, false), top_right), top_right);
        // Turned a quarter anticlockwise, the image's bottom-right corner shows at the top right.
        assert_eq!(tile_image_point(tile(true, false), top_right), Vec2::new(1.0, 1.0));
        // Half round, its bottom-left corner shows there.
        assert_eq!(tile_image_point(tile(false, true), top_right), Vec2::new(0.0, 1.0));
        // Both: its top-left corner.
        assert_eq!(tile_image_point(tile(true, true), top_right), Vec2::new(0.0, 0.0));
    }
}
