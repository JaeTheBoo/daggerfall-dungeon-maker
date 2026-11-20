//! The lights of Daggerfall's places, as Daggerfall Unity lights them, for
//! drawing a scene with the game's lighting.
//!
//! * Dungeons are dark (`PlayerAmbientLight.DungeonAmbientLight`) but for
//!   their light objects, each a point light three times its stored radius
//!   in reach (`RDBLayout.AddLight`, with the dungeon light prefab).
//! * Insides of buildings are dim (`InteriorAmbientLight`) but for their
//!   lamps: each flat of the lights archive (210) is a point light, its
//!   reach, brightness and colour set by its image (`DaggerfallInterior.AddLight`,
//!   with the interior light prefab).
//! * Settlements at night are dark (`ExteriorNightAmbientLight`) but for
//!   their street lamps, the same flats outdoors (`RMBLayout.AddLight`, with
//!   the city light prefab). Their gates shut at night too.
//!
//! Daggerfall Unity draws with Unity's lights; their fall-off is approximated.

use glam::Vec3;

use crate::arena2::{Arena2Library, TextureKey};
use crate::scene::{Scene, SceneObjectKind};

/// Most lights a scene is drawn with: those nearest the camera's point of interest.
pub const MAX_LIGHTS: usize = 128;

/// The archive of the images of lamps, candles and torches.
pub const LIGHTS_ARCHIVE: u16 = 210;

/// How a scene is lit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameLighting {
    /// Fully lit, as the editor draws it.
    #[default]
    Off,
    /// A dungeon's darkness and its lights.
    Dungeon,
    /// The inside of a building and its lamps.
    Interior,
    /// A settlement at night and its street lamps.
    Night,
}

impl GameLighting {
    /// The light every surface gets, whatever lamps are near (`PlayerAmbientLight`).
    pub fn ambient(self) -> Option<Vec3> {
        match self {
            Self::Off => None,
            Self::Dungeon => Some(Vec3::splat(0.12)),
            Self::Interior => Some(Vec3::splat(0.18)),
            Self::Night => Some(Vec3::splat(0.25)),
        }
    }
}

/// One point light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLight {
    pub position: Vec3,
    /// How far it reaches, in metres.
    pub range: f32,
    pub color: Vec3,
    pub intensity: f32,
}

/// The dungeon light prefab: white, at 0.8.
const DUNGEON_LIGHT_INTENSITY: f32 = 0.8;
/// A dungeon light reaches this many times its stored radius.
const DUNGEON_LIGHT_REACH: f32 = 3.0;
/// The interior light prefab: white, at 1, reaching 15 m.
const INTERIOR_LIGHT_RANGE: f32 = 15.0;
/// The city light prefab: white, at 1, reaching 18 m.
const CITY_LIGHT_RANGE: f32 = 18.0;

/// The lights of `scene` when it is drawn with `lighting`.
pub fn scene_lights(scene: &Scene, lighting: GameLighting, library: &mut Arena2Library) -> Vec<PointLight> {
    let mut lights = Vec::new();
    for object in scene.objects.iter().filter(|object| object.visible) {
        match (lighting, &object.kind) {
            (GameLighting::Dungeon, SceneObjectKind::Light { radius, .. }) => lights.push(PointLight {
                position: object.position,
                range: radius * DUNGEON_LIGHT_REACH,
                color: Vec3::ONE,
                intensity: DUNGEON_LIGHT_INTENSITY,
            }),
            (GameLighting::Interior, &SceneObjectKind::Flat { texture, .. })
                if texture.archive == LIGHTS_ARCHIVE =>
            {
                let height = library.billboard_world_size(texture).map_or(0.0, |size| size.y);
                lights.push(interior_lamp(texture, object.position, height));
            }
            (GameLighting::Night, &SceneObjectKind::Flat { texture, .. })
                if texture.archive == LIGHTS_ARCHIVE =>
            {
                let height = library.billboard_world_size(texture).map_or(0.0, |size| size.y);
                lights.push(PointLight {
                    position: object.position + Vec3::Y * (height / 2.0),
                    range: CITY_LIGHT_RANGE,
                    color: Vec3::ONE,
                    intensity: 1.0,
                });
            }
            _ => {}
        }
    }
    lights
}

/// The light of a lamp inside a building, drawn with `texture` and centred
/// on `center` with a height of `height`: Daggerfall Unity's settings for
/// its image, over the interior light prefab's.
fn interior_lamp(texture: TextureKey, center: Vec3, height: f32) -> PointLight {
    let mut light =
        PointLight { position: center, range: INTERIOR_LIGHT_RANGE, color: Vec3::ONE, intensity: 1.0 };
    let warm = |r: f32, g: f32, b: f32| Vec3::new(r, g, b);
    let (offset, range, intensity, color): (f32, Option<f32>, Option<f32>, Option<Vec3>) =
        match texture.record {
            0 => (-0.1, Some(20.0), Some(1.1), Some(warm(0.95, 0.91, 0.63))),
            2 => (0.1, Some(INTERIOR_LIGHT_RANGE / 3.0), Some(0.6), Some(warm(1.0, 0.99, 0.82))),
            3 => (0.1, Some(INTERIOR_LIGHT_RANGE / 3.0), None, None),
            4 => (0.0, Some(INTERIOR_LIGHT_RANGE / 3.0), None, None),
            5 => (0.15, Some(7.5), Some(0.33), Some(warm(1.0, 0.89, 0.61))),
            6 => (0.6, Some(15.0), Some(0.75), Some(warm(1.0, 0.93, 0.62))),
            8 => (0.0, None, None, Some(warm(0.68, 1.0, 0.94))),
            9 => (0.4, Some(15.0), Some(0.65), Some(warm(1.0, 0.92, 0.6))),
            11 => (-0.4, Some(5.0), Some(0.5), None),
            13 => (-0.35, Some(INTERIOR_LIGHT_RANGE * 1.2), Some(1.1), Some(warm(0.93, 0.84, 0.49))),
            14 | 15 => (height / 2.0, None, None, None),
            17 => (0.2, None, Some(0.8), Some(warm(1.0, 0.97, 0.87))),
            20 => (0.6, Some(12.0), Some(0.75), Some(warm(1.0, 0.92, 0.72))),
            21 => (height / 2.4, Some(INTERIOR_LIGHT_RANGE / 3.0), Some(0.5), Some(warm(1.0, 0.95, 0.67))),
            22 => (-0.5, None, Some(1.5), Some(warm(1.0, 0.95, 0.78))),
            24 => (-1.85, None, Some(1.4), Some(warm(1.0, 0.98, 0.64))),
            25 => (-1.0, None, Some(1.4), Some(warm(1.0, 0.98, 0.64))),
            26 => (0.0, None, Some(1.4), Some(warm(1.0, 0.98, 0.64))),
            27 => (-0.02, None, Some(1.4), Some(warm(1.0, 0.98, 0.64))),
            _ => (0.0, None, None, None),
        };
    light.position.y += offset;
    light.range = range.unwrap_or(light.range);
    light.intensity = intensity.unwrap_or(light.intensity);
    light.color = color.unwrap_or(light.color);
    light
}

/// The `limit` lights of `lights` nearest to `point`.
pub fn nearest_lights(mut lights: Vec<PointLight>, point: Vec3, limit: usize) -> Vec<PointLight> {
    if lights.len() > limit {
        lights.sort_by(|a, b| {
            let reach = |light: &PointLight| light.position.distance(point) - light.range;
            reach(a).total_cmp(&reach(b))
        });
        lights.truncate(limit);
    }
    lights
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lamps_take_their_images_settings() {
        let chandelier = interior_lamp(TextureKey::new(LIGHTS_ARCHIVE, 0), Vec3::ZERO, 1.0);
        assert_eq!((chandelier.range, chandelier.intensity), (20.0, 1.1));
        assert!((chandelier.position.y + 0.1).abs() < 1e-6);
        let candle = interior_lamp(TextureKey::new(LIGHTS_ARCHIVE, 3), Vec3::ZERO, 1.0);
        assert_eq!((candle.range, candle.intensity), (5.0, 1.0), "a third of the prefab's reach");
        let plain = interior_lamp(TextureKey::new(LIGHTS_ARCHIVE, 1), Vec3::ZERO, 1.0);
        assert_eq!((plain.range, plain.intensity, plain.color), (15.0, 1.0, Vec3::ONE), "the prefab's own");
        assert_eq!(GameLighting::Off.ambient(), None);
    }

    #[test]
    fn keeps_the_lights_nearest_the_point() {
        let light =
            |x: f32| PointLight { position: Vec3::X * x, range: 1.0, color: Vec3::ONE, intensity: 1.0 };
        let kept = nearest_lights(vec![light(9.0), light(1.0), light(5.0)], Vec3::ZERO, 2);
        assert_eq!(kept.iter().map(|light| light.position.x).collect::<Vec<_>>(), [1.0, 5.0]);
    }
}
