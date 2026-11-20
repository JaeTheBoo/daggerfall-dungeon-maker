//! Finding places for a test: an empty spot of the world map to set a
//! location down on its own, and a clear spot in a cell or settlement to put
//! the player at.

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use crate::arena2::{Arena2Library, MapPixel, WorldMap, MAP_HEIGHT, MAP_WIDTH};
use crate::scene::{SceneObject, SceneObjectKind};

/// How far, in map pixels, an empty spot is from every location of the
/// game: more than the land Daggerfall Unity draws round the player (at most
/// 4 map pixels away in its options), so no other location is in sight.
pub const CLEAR_RADIUS: i32 = 6;

/// `CLIMATE.PAK` value of the sea.
const OCEAN_CLIMATE: u8 = 223;

/// How far apart the spots tried for the player are, in metres (the editor's world units).
const SPOT_STEP: f32 = 1.0;
/// How far the player must keep from a model's box, in metres.
const PLAYER_CLEARANCE: f32 = 0.6;
/// Models whose top is this low (in metres above the ground) can be stood
/// on: the ground, paving. Any other model is in the way, even high above
/// the ground, as the game would stand the player on it (a walkway along a
/// wall's top, a bridge).
const LOW_MODEL_TOP: f32 = 0.3;
/// Side of the squares models are sorted into, to find those near a spot quickly.
const BUCKET_SIDE: f32 = 16.0;

/// The map pixel nearest `near` with no location within [`CLEAR_RADIUS`],
/// in a region (not the sea) and, if `climate` is given, with that
/// `CLIMATE.PAK` value. `None` if there is none.
pub fn empty_map_pixel(world: &WorldMap, climate: Option<u8>, near: MapPixel) -> Option<MapPixel> {
    let (width, height) = (MAP_WIDTH as usize, MAP_HEIGHT as usize);
    // How many locations are at or above and left of each pixel, for counting those in any box.
    let mut counts = vec![0u32; (width + 1) * (height + 1)];
    for location in &world.locations {
        if location.pixel.is_on_map() {
            counts[(location.pixel.y as usize + 1) * (width + 1) + location.pixel.x as usize + 1] += 1;
        }
    }
    for y in 1..=height {
        for x in 1..=width {
            counts[y * (width + 1) + x] += counts[(y - 1) * (width + 1) + x]
                + counts[y * (width + 1) + x - 1]
                - counts[(y - 1) * (width + 1) + x - 1];
        }
    }
    let locations_in = |left: i32, top: i32, right: i32, bottom: i32| {
        let at = |x: i32, y: i32| counts[y as usize * (width + 1) + x as usize];
        at(right + 1, bottom + 1) + at(left, top) - at(left, bottom + 1) - at(right + 1, top)
    };
    let mut best: Option<(i64, MapPixel)> = None;
    for y in CLEAR_RADIUS..MAP_HEIGHT - CLEAR_RADIUS {
        for x in CLEAR_RADIUS..MAP_WIDTH - CLEAR_RADIUS {
            let pixel = MapPixel::new(x, y);
            let distance = i64::from(x - near.x).pow(2) + i64::from(y - near.y).pow(2);
            if best.is_some_and(|(best_distance, _)| best_distance <= distance) {
                continue;
            }
            if locations_in(x - CLEAR_RADIUS, y - CLEAR_RADIUS, x + CLEAR_RADIUS, y + CLEAR_RADIUS) != 0
                || world.region_at(pixel).is_none()
            {
                continue;
            }
            let pixel_climate = world.climate_at(pixel);
            let fits = match climate {
                Some(climate) => pixel_climate == Some(climate),
                None => pixel_climate != Some(OCEAN_CLIMATE),
            };
            if fits {
                best = Some((distance, pixel));
            }
        }
    }
    best.map(|(_, pixel)| pixel)
}

/// The boxes of the models among `objects` that are in the way of a player
/// put on the ground (at height 0) with only the sky above: each model's
/// box in world space, leaving out those low enough to stand on, such as
/// the ground itself.
pub fn obstacles(objects: &[SceneObject], library: &mut Arena2Library) -> Vec<(Vec3, Vec3)> {
    let mut boxes = Vec::new();
    for object in objects.iter().filter(|object| object.visible) {
        let SceneObjectKind::Model { model_id, .. } = object.kind else { continue };
        let Some(mesh) = library.model(model_id) else { continue };
        let matrix = object.model_matrix();
        let (low, high) = (mesh.bounds_min, mesh.bounds_max);
        let mut minimum = Vec3::splat(f32::MAX);
        let mut maximum = Vec3::splat(f32::MIN);
        for corner in 0..8 {
            let point = Vec3::new(
                if corner & 1 == 0 { low.x } else { high.x },
                if corner & 2 == 0 { low.y } else { high.y },
                if corner & 4 == 0 { low.z } else { high.z },
            );
            let point = matrix.transform_point3(point);
            minimum = minimum.min(point);
            maximum = maximum.max(point);
        }
        if maximum.y > LOW_MODEL_TOP {
            boxes.push((minimum, maximum));
        }
    }
    boxes
}

/// The spot nearest `centre` (x and z in metres, the editor's world units)
/// inside the area from `area_minimum` to `area_maximum` that is clear of
/// every one of `obstacles` (see [`obstacles`]) by a player's width; the
/// centre itself if no spot is clear.
pub fn clear_spot(obstacles: &[(Vec3, Vec3)], centre: Vec2, area_minimum: Vec2, area_maximum: Vec2) -> Vec2 {
    let bucket =
        |point: Vec2| ((point.x / BUCKET_SIDE).floor() as i32, (point.y / BUCKET_SIDE).floor() as i32);
    // The obstacles reaching into each bucket, grown by the player's clearance.
    let mut buckets: HashMap<(i32, i32), Vec<(Vec2, Vec2)>> = HashMap::new();
    for (minimum, maximum) in obstacles {
        let low = Vec2::new(minimum.x, minimum.z) - PLAYER_CLEARANCE;
        let high = Vec2::new(maximum.x, maximum.z) + PLAYER_CLEARANCE;
        let ((left, bottom), (right, top)) = (bucket(low), bucket(high));
        for x in left..=right {
            for y in bottom..=top {
                buckets.entry((x, y)).or_default().push((low, high));
            }
        }
    }
    let is_clear = |point: Vec2| {
        buckets.get(&bucket(point)).is_none_or(|boxes| {
            boxes
                .iter()
                .all(|(low, high)| point.x < low.x || point.x > high.x || point.y < low.y || point.y > high.y)
        })
    };
    let inside = |point: Vec2| {
        point.x >= area_minimum.x
            && point.x <= area_maximum.x
            && point.y >= area_minimum.y
            && point.y <= area_maximum.y
    };
    let reach = (area_maximum - area_minimum).max_element();
    let rings = (reach / SPOT_STEP).ceil() as i32 + 1;
    for ring in 0..=rings {
        // The clear spots on this square ring round the centre; the nearest wins.
        let mut nearest: Option<Vec2> = None;
        for step_x in -ring..=ring {
            for step_y in -ring..=ring {
                if step_x.abs() != ring && step_y.abs() != ring {
                    continue;
                }
                let point = centre + Vec2::new(step_x as f32, step_y as f32) * SPOT_STEP;
                if inside(point)
                    && is_clear(point)
                    && nearest
                        .is_none_or(|best| point.distance_squared(centre) < best.distance_squared(centre))
                {
                    nearest = Some(point);
                }
            }
        }
        if let Some(point) = nearest {
            return point;
        }
    }
    centre
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_centre_is_kept_when_it_is_clear() {
        let spot = clear_spot(&[], Vec2::new(50.0, 50.0), Vec2::ZERO, Vec2::splat(100.0));
        assert_eq!(spot, Vec2::new(50.0, 50.0));
    }

    #[test]
    fn a_building_on_the_centre_moves_the_spot_just_outside_it() {
        let building = (Vec3::new(40.0, 0.0, 45.0), Vec3::new(60.0, 8.0, 55.0));
        let spot = clear_spot(&[building], Vec2::new(50.0, 50.0), Vec2::ZERO, Vec2::splat(100.0));
        // The nearest clear spots are past the building's north and south sides.
        assert_eq!(spot.x, 50.0);
        assert!(spot.y < 45.0 - PLAYER_CLEARANCE || spot.y > 55.0 + PLAYER_CLEARANCE, "{spot}");
        assert!((spot.y - 50.0).abs() <= 5.0 + PLAYER_CLEARANCE + SPOT_STEP, "{spot}");
    }

    #[test]
    fn spots_outside_the_area_are_not_used() {
        // A wall over the whole area apart from its east edge.
        let wall = (Vec3::new(-10.0, 0.0, -10.0), Vec3::new(97.0, 5.0, 110.0));
        let spot = clear_spot(&[wall], Vec2::new(50.0, 50.0), Vec2::ZERO, Vec2::splat(100.0));
        assert!(spot.x > 97.0 + PLAYER_CLEARANCE && spot.x <= 100.0, "{spot}");
    }
}
