//! Tests that parse a real Daggerfall installation.
//!
//! No game data is stored in this repository. Set the `DAGGERFALL_ARENA2`
//! environment variable to an `ARENA2` folder to run these tests; without it
//! every test returns immediately.

use std::path::PathBuf;

use daggerfall_dungeon_maker::arena2::{Arena2Library, RdbObjectKind};
use daggerfall_dungeon_maker::catalog::{AssetCatalog, ModelCategory};

/// Opens the library pointed to by `DAGGERFALL_ARENA2`, if set.
fn open_library() -> Option<Arena2Library> {
    let folder = PathBuf::from(std::env::var_os("DAGGERFALL_ARENA2")?);
    Some(Arena2Library::open(&folder).expect("ARENA2 folder should load"))
}

#[test]
fn lists_rdb_blocks() {
    let Some(library) = open_library() else { return };

    let block_names = library.rdb_block_names();
    println!("{} RDB blocks, first {:?}", block_names.len(), block_names.first());
    assert!(block_names.len() > 100);
    assert!(block_names.iter().all(|name| name.ends_with(".RDB")));
}

#[test]
fn lists_dungeons_whose_blocks_all_exist() {
    let Some(mut library) = open_library() else { return };

    let dungeons = library.dungeons();
    println!("{} dungeons", dungeons.len());
    assert!(dungeons.len() > 1000);
    assert!(dungeons.iter().any(|dungeon| dungeon.name == "Privateer's Hold"));

    let block_names = library.rdb_block_names();
    for dungeon in dungeons.iter() {
        for placement in &dungeon.blocks {
            assert!(
                block_names.contains(&placement.block_name),
                "{} uses missing {}",
                dungeon.name,
                placement.block_name
            );
        }
    }
}

#[test]
fn lists_every_location_with_its_outdoor_cells() {
    let Some(mut library) = open_library() else { return };

    let locations = library.locations();
    let dungeons = library.dungeons();
    let with_dungeon = locations.iter().filter(|location| location.has_dungeon).count();
    println!("{} locations, {with_dungeon} with a dungeon, {} dungeons", locations.len(), dungeons.len());
    assert!(locations.len() > 10_000);
    // Every dungeon lies under a location; a few locations share one dungeon's record.
    assert!(with_dungeon >= dungeons.len() * 99 / 100, "{with_dungeon} of {}", dungeons.len());

    let city = locations
        .iter()
        .find(|location| location.name == "Daggerfall" && location.region_name == "Daggerfall")
        .expect("the city of Daggerfall");
    println!(
        "Daggerfall: {:?}, {} × {} cells",
        city.location_type, city.exterior.width, city.exterior.height
    );
    assert_eq!(city.location_type, Some(daggerfall_dungeon_maker::arena2::LocationType::City));
    assert!(city.exterior.width * city.exterior.height >= 25);
    assert!(city.has_dungeon, "Castle Daggerfall lies under the city");

    // Every outdoor cell a location uses can be found (some through Daggerfall Unity's
    // substitutes) and read, with its 16 × 16 ground tiles.
    let mut unreadable = Vec::new();
    let mut checked = std::collections::HashSet::new();
    for location in locations.iter() {
        assert_eq!(location.exterior.block_names.len(), location.exterior.width * location.exterior.height);
        for name in &location.exterior.block_names {
            if !checked.insert(name.clone()) {
                continue;
            }
            match library.rmb_block(name) {
                Ok(block) => assert_eq!(block.ground.len(), 256, "{name}"),
                Err(error) => unreadable.push(format!("{name} ({}): {error}", location.name)),
            }
        }
    }
    println!("{} different outdoor cells used", checked.len());
    assert!(unreadable.is_empty(), "{unreadable:?}");
}

#[test]
fn outdoor_cells_stand_on_their_climates_ground() {
    use daggerfall_dungeon_maker::arena2::{Climate, Season, RMB_BLOCK_SIDE};
    use daggerfall_dungeon_maker::exterior::cell_scene;
    use daggerfall_dungeon_maker::scene::SceneObjectKind;
    let Some(mut library) = open_library() else { return };

    let block = library.rmb_block("TVRNAL01.RMB").expect("a tavern cell");
    for (climate, season) in [
        (Climate::DEFAULT, Season::Summer),
        (Climate::DEFAULT, Season::Winter),
        (Climate::from_value(224), Season::Winter),
    ] {
        let scene = cell_scene(&block, climate, season, 3, &mut library);
        assert!(scene.objects.iter().all(|object| object.group_index == 3));
        // The ground comes first: 256 tiles of two triangles each, the size of the cell.
        let SceneObjectKind::Model { model_id, .. } = scene.objects[0].kind else { panic!("ground first") };
        let ground = library.model(model_id).expect("the ground model");
        assert_eq!(ground.triangle_count(), 512);
        let ground_archive = climate.ground_archive_in(season);
        assert!(ground.submeshes.iter().all(|submesh| submesh.texture.archive == ground_archive));
        assert!(
            (ground.size().x - RMB_BLOCK_SIDE).abs() < 0.001
                && (ground.size().z - RMB_BLOCK_SIDE).abs() < 0.001
        );
        // Every tile's texture can be drawn.
        for submesh in &ground.submeshes {
            assert!(library.texture_image(submesh.texture).is_some(), "{:?}", submesh.texture);
        }
        // Then the buildings and scenery, and the flats, trees in the climate's own archive.
        assert_eq!(scene.objects.len(), 1 + block.models.len() + block.flats.len());
        let nature = scene.objects.iter().filter(|object| {
            matches!(object.kind, SceneObjectKind::Flat { texture, .. } if (500..=511).contains(&texture.archive))
        });
        assert!(nature.clone().count() > 0);
        let nature_archive = climate.nature_archive_in(season);
        assert!(nature.clone().all(|object| matches!(object.kind,
            SceneObjectKind::Flat { texture, .. } if texture.archive == nature_archive)));
    }

    // Sentinel's cells mark some tiles with a record past the archive's, drawn as plain ground.
    let sentinel = library.rmb_block("SENT1.RMB").expect("a cell of Sentinel");
    assert!(sentinel.ground.iter().any(|tile| tile.record == 63));
    let scene = cell_scene(&sentinel, Climate::from_value(224), Season::Summer, 0, &mut library);
    let SceneObjectKind::Model { model_id, .. } = scene.objects[0].kind else { panic!("ground first") };
    let ground = library.model(model_id).expect("the ground model");
    for submesh in &ground.submeshes {
        assert!(library.texture_image(submesh.texture).is_some(), "{:?}", submesh.texture);
    }
}

#[test]
fn climate_swaps_find_their_textures_in_every_outdoor_cell() {
    use daggerfall_dungeon_maker::arena2::{Climate, Season};
    use daggerfall_dungeon_maker::exterior::ClimateTextures;
    use std::collections::{BTreeMap, BTreeSet};
    let Some(mut library) = open_library() else { return };

    // Every texture of every model in every outdoor cell.
    let mut textures = BTreeSet::new();
    for name in library.rmb_block_names() {
        for model in library.rmb_block(&name).expect("an outdoor cell").models {
            if let Some(mesh) = library.model(model.model_id) {
                textures.extend(mesh.submeshes.iter().map(|submesh| submesh.texture));
            }
        }
    }
    // The textures the climates swap them for, in summer and winter, that the game data lacks (none).
    let mut missing: BTreeMap<String, BTreeSet<(u16, u8)>> = BTreeMap::new();
    let mut swapped = 0;
    for climate in Climate::ALL {
        for season in Season::ALL {
            let swaps = ClimateTextures::new(climate, season);
            for &texture in &textures {
                let new = swaps.apply(texture);
                if new == texture {
                    continue;
                }
                swapped += 1;
                if library.texture_image(new).is_none() {
                    missing
                        .entry(format!("{} {}", climate.name, season.name()))
                        .or_default()
                        .insert((new.archive, new.record));
                }
            }
        }
    }
    println!("{} textures used outdoors, {swapped} swaps", textures.len());
    for (climate, textures) in &missing {
        println!(
            "{climate}: {} missing, e.g. {:?}",
            textures.len(),
            textures.iter().take(8).collect::<Vec<_>>()
        );
    }
    assert!(swapped > 1000);
    // Every swap finds its texture in the game data.
    assert!(missing.is_empty(), "swaps that find no texture: {missing:?}");
}

#[test]
fn dungeons_have_types() {
    let Some(mut library) = open_library() else { return };

    let dungeons = library.dungeons();
    let typed = dungeons.iter().filter(|dungeon| dungeon.dungeon_type.is_some()).count();
    println!("{typed} of {} dungeons have a type", dungeons.len());
    assert!(typed * 100 >= dungeons.len() * 99, "most dungeons should have a type");
    for dungeon_type in daggerfall_dungeon_maker::encounters::DungeonType::all() {
        let examples: Vec<&str> = dungeons
            .iter()
            .filter(|dungeon| dungeon.dungeon_type == Some(dungeon_type))
            .map(|dungeon| dungeon.name.as_str())
            .take(3)
            .collect();
        println!("{}: {}", dungeon_type.name(), examples.join(", "));
        assert!(!examples.is_empty(), "no dungeon is a {}", dungeon_type.name());
    }
}

#[test]
fn fixed_enemy_markers_name_known_enemies() {
    let Some(library) = open_library() else { return };

    let mut markers = Vec::new();
    for name in library.rdb_block_names() {
        let block = library.rdb_block(&name).expect("block should parse");
        let scene = daggerfall_dungeon_maker::scene::Scene::from_rdb_block(&block);
        markers.extend(
            scene.objects.iter().filter_map(|object| object.fixed_enemy()).map(|enemy| (name.clone(), enemy)),
        );
    }
    let unknown: Vec<_> = markers
        .iter()
        .filter(|(_, enemy)| enemy.enemy().is_none() && !enemy.is_switched_off())
        .map(|(name, enemy)| format!("{name}: {}", enemy.mobile_id))
        .collect();
    println!("{} fixed enemy markers, {} name an unknown enemy", markers.len(), unknown.len());
    for (name, enemy) in markers.iter().take(8) {
        println!(
            "  {name}: {} (passive: {}, gender: {:?})",
            enemy.description(),
            enemy.passive,
            enemy.human_gender()
        );
    }
    assert!(!markers.is_empty());
    assert!(unknown.is_empty(), "unknown enemies: {unknown:?}");
}

#[test]
fn water_cells_have_underwater_enemy_markers() {
    let Some(library) = open_library() else { return };

    let mut cells_with_water = Vec::new();
    let mut underwater_markers = 0;
    let mut random_markers = 0;
    for name in library.rdb_block_names() {
        let block = library.rdb_block(&name).expect("block should parse");
        let scene = daggerfall_dungeon_maker::scene::Scene::from_rdb_block(&block);
        let markers: Vec<_> = scene.objects.iter().filter(|object| object.is_random_enemy_marker()).collect();
        random_markers += markers.len();
        if let Some(level) = scene.water_level() {
            let under = markers.iter().filter(|marker| scene.is_under_water(marker)).count();
            underwater_markers += under;
            cells_with_water.push(format!(
                "{name} (water at {level:.1} m, {under} of {} random markers under)",
                markers.len()
            ));
        }
    }
    println!(
        "{} cells have water; {underwater_markers} of {random_markers} random enemy markers are under it",
        cells_with_water.len()
    );
    for cell in cells_with_water.iter().take(6) {
        println!("  {cell}");
    }
    assert!(!cells_with_water.is_empty());
    assert!(underwater_markers > 0);
    // Water cells are the "W" interiors.
    assert!(cells_with_water.iter().any(|cell| cell.starts_with('W')));
}

#[test]
fn lever_action_links_to_a_door() {
    use daggerfall_dungeon_maker::scene::{Scene, SceneObjectKind};
    let Some(mut library) = open_library() else { return };

    let scene = Scene::from_rdb_block(&library.rdb_block("N0000051.RDB").expect("block should parse"));
    let lever = scene
        .objects
        .iter()
        .find(|object| matches!(object.kind, SceneObjectKind::Model { model_id: 61028, .. }))
        .expect("N0000051 has a lever");
    let SceneObjectKind::Model { data, .. } = lever.kind else { unreachable!() };
    let action = data.action.expect("the lever has an action");
    println!("lever action: {action:?}, trigger {}, lock index {}", data.trigger(), data.lock_index());
    let target =
        scene.object_by_id(lever.next.expect("the lever links to an object")).expect("the target exists");
    let SceneObjectKind::Model { model_id, .. } = target.kind else {
        panic!("the lever should link to a model")
    };
    println!("lever opens model {model_id}");
    let door = library.model(model_id).expect("target model loads");
    assert!(door.door_kind().is_some(), "the lever should link to a door");
}

#[test]
fn parses_every_rdb_block_and_its_models() {
    let Some(mut library) = open_library() else { return };

    let mut object_count = 0;
    let mut missing_models = 0;
    for name in library.rdb_block_names() {
        let block = library.rdb_block(&name).unwrap_or_else(|error| panic!("{name}: {error}"));
        object_count += block.objects.len();
        for object in &block.objects {
            if let RdbObjectKind::Model(model) = &object.kind {
                if library.model(model.model_id).is_none() {
                    missing_models += 1;
                }
            }
        }
    }
    println!("{object_count} objects, {missing_models} missing model references");
    assert!(object_count > 10_000);
    assert_eq!(missing_models, 0);
}

#[test]
fn object_lists_are_the_quarters_of_a_cell() {
    use daggerfall_dungeon_maker::arena2::native_to_world;
    use daggerfall_dungeon_maker::scene::{group_label, quarter_of, Scene};
    let Some(library) = open_library() else { return };

    // Each of a cell's own lists holds the objects of one square of it, and the
    // lists come in the order of those squares (west to east, then south to
    // north), whether the cell's grid is four lists wide or, in five cells, eight.
    let mut checked = 0;
    for name in library.rdb_block_names() {
        let block = library.rdb_block(&name).unwrap();
        // Daggerfall Unity's corrections move some objects into other squares
        // (the exit of W0000009, for one) without moving them to other lists.
        if daggerfall_dungeon_maker::arena2::unity_fix_description(&name).is_some() {
            let scene = Scene::from_rdb_block(&block);
            for object in &scene.objects {
                assert_eq!(
                    object.group_index,
                    quarter_of(object.position),
                    "{name}: objects go by their square"
                );
            }
            continue;
        }
        let mut quarter_of_list = std::collections::BTreeMap::new();
        for object in &block.objects {
            let quarter = quarter_of(native_to_world(object.position));
            let known = *quarter_of_list.entry(object.group_index).or_insert(quarter);
            assert_eq!(known, quarter, "{name}: list {} spans several squares", object.group_index);
            checked += 1;
        }
        let quarters: Vec<usize> = quarter_of_list.values().copied().collect();
        assert!(
            quarters.windows(2).all(|pair| pair[0] < pair[1]),
            "{name}: lists out of order: {quarter_of_list:?}"
        );
        // The editor lists the cell's objects by those squares, in the same order.
        let scene = Scene::from_rdb_block(&block);
        for (object, read) in scene.objects.iter().zip(&block.objects) {
            assert_eq!(object.group_index, quarter_of_list[&read.group_index]);
        }
    }
    println!("{checked} objects are in their square's list");
    assert_eq!(group_label(0), "South-west quarter");
    assert_eq!(group_label(5), "North-east quarter");
    assert_eq!(group_label(2), "Past the east edge, south (list 2)");
}

#[test]
fn describes_every_model() {
    let Some(mut library) = open_library() else { return };

    let started = std::time::Instant::now();
    let catalog = AssetCatalog::build(&mut library);
    println!("catalog built in {:?}", started.elapsed());

    assert_eq!(library.texture_archive_name(74), Some("Door Set"));
    let exit_door = catalog.model_description(70300).expect("the exit door model is described");
    println!("70300: {}", exit_door.summary());
    assert_eq!(exit_door.category, ModelCategory::ExitDoor);

    let described = catalog.all_model_ids.iter().filter(|&&id| catalog.model_description(id).is_some());
    assert!(described.count() > catalog.all_model_ids.len() * 9 / 10);
    let mut category_counts = std::collections::BTreeMap::new();
    for (position, model) in catalog.room_models.iter().enumerate() {
        let description = catalog.model_description(model.model_id).expect("room models are described");
        *category_counts.entry(description.category.label()).or_insert(0) += 1;
        if position % 12 == 0 {
            println!("{}: {}", model.model_id, description.summary());
        }
    }
    println!("room model categories: {category_counts:?}");

    // Exterior models come from the outdoor blocks: buildings, walls and scenery
    // that dungeon cells never use, and the dungeon entrances seen from outside.
    println!(
        "{} exterior models, {} dungeon models",
        catalog.exterior_models.len(),
        catalog.room_models.len()
    );
    assert!(catalog.exterior_models.len() > 100);
    let outdoors_only =
        catalog.exterior_models.iter().filter(|model| catalog.room_model(model.model_id).is_none()).count();
    assert!(outdoors_only > catalog.exterior_models.len() / 2, "{outdoors_only}");
    assert!(catalog.exterior_models.iter().any(|model| {
        catalog
            .model_description(model.model_id)
            .is_some_and(|description| description.category == ModelCategory::DungeonEntrance)
    }));
    let ranked = catalog.exterior_models.windows(2).all(|pair| pair[0].room_count >= pair[1].room_count);
    assert!(ranked, "exterior models are listed most widely used first");
}

#[test]
fn decodes_room_textures() {
    let Some(mut library) = open_library() else { return };

    let block = library.rdb_block("N0000000.RDB").expect("block exists");
    let mut decoded = 0;
    for object in &block.objects {
        if let RdbObjectKind::Model(model) = &object.kind {
            let mesh = library.model(model.model_id).expect("model exists");
            for submesh in &mesh.submeshes {
                let image = library
                    .texture_image(submesh.texture)
                    .unwrap_or_else(|| panic!("{} should decode", submesh.texture));
                assert_eq!(image.pixels.len(), (image.width * image.height * 4) as usize);
                decoded += 1;
            }
        }
    }
    println!("decoded {decoded} textures");
    assert!(decoded > 0);
}

#[test]
fn dungeons_sit_on_the_world_map_in_their_region() {
    let Some(mut library) = open_library() else { return };

    let world = library.world_map().expect("MAPS.BSA should give a world map");
    assert!(world.has_regions(), "POLITIC.PAK should be read");
    println!("{} locations", world.locations.len());
    assert!(world.locations.len() > 10_000);

    let dungeons = library.dungeons();
    let mut in_region = 0;
    let mut location_types = std::collections::BTreeMap::new();
    for dungeon in dungeons.iter() {
        let pixel = dungeon.map_pixel.unwrap_or_else(|| panic!("{} has no map position", dungeon.name));
        assert!(pixel.is_on_map(), "{} is off the map at {pixel}", dungeon.name);
        assert!(
            world.locations_at(pixel).any(|location| location.name == dungeon.name),
            "{} is not listed at {pixel}",
            dungeon.name
        );
        if world.region_at(pixel) == Some(dungeon.region_index) {
            in_region += 1;
        }
        let type_name = dungeon.location_type.map_or("unknown", |location_type| location_type.name());
        *location_types.entry(type_name).or_insert(0) += 1;
    }
    println!("{in_region} of {} dungeons are on a pixel of their region", dungeons.len());
    println!("location types of dungeons: {location_types:?}");
    assert!(in_region * 100 >= dungeons.len() * 95);
}

#[test]
fn dungeon_entrances_load_and_have_an_entrance_door() {
    use daggerfall_dungeon_maker::arena2::DoorKind;
    use daggerfall_dungeon_maker::buildings::door_boxes;
    use std::collections::BTreeMap;
    let Some(mut library) = open_library() else { return };

    let dungeons = library.dungeons();
    let mut designs: BTreeMap<_, Vec<&str>> = BTreeMap::new();
    for dungeon in dungeons.iter() {
        let exterior =
            dungeon.exterior.as_ref().unwrap_or_else(|| panic!("{} has no exterior", dungeon.name));
        designs
            .entry((exterior.width, exterior.height, exterior.block_names.clone()))
            .or_default()
            .push(&dungeon.name);
    }
    println!("{} dungeons have {} different entrances", dungeons.len(), designs.len());
    let mut without_door = Vec::new();
    let mut boxed_doors = 0;
    for ((width, height, block_names), users) in &designs {
        let mut has_entrance = false;
        for name in block_names {
            let block = library.rmb_block(name).unwrap_or_else(|error| panic!("{name}: {error}"));
            for model in &block.models {
                let Some(mesh) = library.model(model.model_id) else { continue };
                // The red boxes the views draw round dungeon doors.
                let doors = door_boxes(&mesh, DoorKind::DungeonEntrance).len();
                boxed_doors += doors;
                has_entrance |= doors > 0;
                assert_eq!(
                    doors > 0,
                    mesh.submeshes
                        .iter()
                        .any(|submesh| DoorKind::from_texture(submesh.texture)
                            == Some(DoorKind::DungeonEntrance))
                );
            }
        }
        if !has_entrance {
            without_door.push(format!("{width}x{height} {block_names:?} ({})", users[0]));
        }
    }
    println!(
        "{boxed_doors} dungeon doors; {} entrances without one: {:?}",
        without_door.len(),
        without_door.iter().take(10).collect::<Vec<_>>()
    );
    assert!(without_door.len() * 20 <= designs.len());
}

#[test]
fn doors_and_pieces_join_in_daggerfalls_own_cells() {
    use daggerfall_dungeon_maker::connectors::{fit, open_faces, DoorSlots};
    use daggerfall_dungeon_maker::scene::{Scene, SceneObjectKind};
    let Some(mut library) = open_library() else { return };

    let mut door_slots = DoorSlots::default();
    let (mut joined, mut pieces) = (0, 0);
    for name in library.rdb_block_names() {
        let scene = Scene::from_rdb_block(&library.rdb_block(&name).unwrap());
        door_slots.learn(&scene, &mut library);
        let mut faces = Vec::new();
        for (index, object) in scene.objects.iter().enumerate() {
            let SceneObjectKind::Model { model_id, .. } = object.kind else { continue };
            let Some(mesh) = library.model(model_id) else { continue };
            for face in open_faces(&mesh).iter().filter(|face| !face.is_plug) {
                faces.push((index, face.transformed(object.rotation(), object.position)));
            }
        }
        let mut with_faces = std::collections::HashSet::new();
        let mut matched = std::collections::HashSet::new();
        for (first, face) in &faces {
            with_faces.insert(*first);
            // A face that meets another piece's face of the same size, centre to centre.
            let meets = faces.iter().any(|(second, other)| {
                first != second
                    && face.center.distance(other.center) < 0.1
                    && fit(face, other).is_some_and(|m| m < 0.1)
            });
            if meets {
                matched.insert(*first);
            }
        }
        pieces += with_faces.len();
        joined += matched.len();
    }
    println!(
        "{joined} of {pieces} pieces with openings meet a neighbour exactly; {} pieces have doorways",
        door_slots.piece_count()
    );
    assert!(door_slots.piece_count() > 200);
    assert!(joined * 4 > pieces, "at least a quarter of the pieces should meet a neighbour face to face");
}

#[test]
fn exported_blocks_match_daggerfalls_own() {
    use std::collections::HashMap;

    use daggerfall_dungeon_maker::export::json::Block;
    use daggerfall_dungeon_maker::scene::Scene;
    let Some(mut library) = open_library() else { return };

    let tags = library.model_tags();
    assert_eq!(tags.get(&55004).map(String::as_str), Some("DOR"), "a common door is tagged as one");
    for name in library.rdb_block_names() {
        let block = library.rdb_block(&name).expect("block should parse");
        let scene = Scene::from_rdb_block(&block);
        let value = serde_json::to_value(Block::from_cell(&name, 0, &scene.objects, &tags)).unwrap();
        let models = value["RdbBlock"]["ModelReferenceList"].as_array().unwrap();
        let exported: HashMap<i64, &serde_json::Value> = value["RdbBlock"]["ObjectRootList"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|root| root["RdbObjects"].as_array())
            .flatten()
            .map(|object| (object["Position"].as_i64().unwrap(), object))
            .collect();
        assert_eq!(exported.len(), block.objects.len(), "{name}: every object is exported");
        // Exported objects are numbered 16, 32, … in block order.
        let position_of = |offset: i32| {
            block
                .objects
                .iter()
                .position(|object| object.offset == offset)
                .map_or(-1, |index| (index as i64 + 1) * 16)
        };
        for (index, original) in block.objects.iter().enumerate() {
            let object = exported[&((index as i64 + 1) * 16)];
            let place =
                [&object["XPos"], &object["YPos"], &object["ZPos"]].map(|value| value.as_i64().unwrap());
            let original_place =
                [original.position.x, original.position.y, original.position.z].map(i64::from);
            assert_eq!(place, original_place, "{name}");
            let resources = &object["Resources"];
            match &original.kind {
                RdbObjectKind::Model(model) => {
                    let resource = &resources["ModelResource"];
                    let reference = &models[resource["ModelIndex"].as_u64().unwrap() as usize];
                    assert_eq!(reference["ModelIdNum"], model.model_id, "{name}");
                    let rotation =
                        ["XRotation", "YRotation", "ZRotation"].map(|key| resource[key].as_i64().unwrap());
                    let original_rotation =
                        [model.rotation.x, model.rotation.y, model.rotation.z].map(i64::from);
                    assert_eq!(rotation, original_rotation, "{name}");
                    assert_eq!(resource["TriggerFlag_StartingLock"], model.trigger_and_lock, "{name}");
                    assert_eq!(resource["SoundIndex"], model.sound, "{name}");
                    let action = &resource["ActionResource"];
                    match model.action {
                        Some(original_action) => {
                            assert_eq!(action["Flags"], original_action.action_type, "{name}");
                            assert_eq!(action["Axis"], original_action.axis, "{name}");
                            assert_eq!(action["Duration"], original_action.duration, "{name}");
                            assert_eq!(action["Magnitude"], original_action.magnitude, "{name}");
                            let next = position_of(original_action.next_offset);
                            assert_eq!(action["NextObjectOffset"], next, "{name}");
                        }
                        None => assert_eq!(action["Flags"], 0, "{name}"),
                    }
                }
                RdbObjectKind::Flat(flat) => {
                    let resource = &resources["FlatResource"];
                    assert_eq!(resource["TextureArchive"], flat.texture.archive, "{name}");
                    assert_eq!(resource["TextureRecord"], flat.texture.record, "{name}");
                    assert_eq!(resource["Flags"], flat.flags, "{name}");
                    assert_eq!(resource["FactionOrMobileId"], flat.faction_or_mobile_id, "{name}");
                    assert_eq!(resource["Action"], flat.action, "{name}");
                    assert_eq!(resource["NextObjectOffset"], position_of(flat.next_offset), "{name}");
                }
                RdbObjectKind::Light(light) => {
                    let resource = &resources["LightResource"];
                    assert_eq!(resource["Radius"], light.radius, "{name}");
                    assert_eq!(resource["Unknown1"], light.unknown_1, "{name}");
                    assert_eq!(resource["Unknown2"], light.unknown_2, "{name}");
                }
            }
        }
    }
}

#[test]
fn exports_a_project_dungeon_as_a_new_location() {
    use daggerfall_dungeon_maker::arena2::MapPixel;
    use daggerfall_dungeon_maker::export::{build_mod, check_project};
    use daggerfall_dungeon_maker::project::{
        CellCategory, DungeonLocation, ModDetails, Project, ProjectDungeonRoom, ProjectEntry, ProjectRoom,
    };
    use daggerfall_dungeon_maker::scene::Scene;
    let Some(mut library) = open_library() else { return };

    let dungeons = library.dungeons();
    let original =
        dungeons.iter().find(|dungeon| dungeon.name == "Privateer's Hold").expect("Privateer's Hold");
    let world = library.world_map().unwrap();
    let region = original.region_index;
    // A free spot of the same region near the original.
    let home = original.map_pixel.unwrap();
    let pixel = (-20..=20)
        .flat_map(|dx| (-20..=20).map(move |dy| MapPixel::new(home.x + dx, home.y + dy)))
        .find(|&pixel| world.region_at(pixel) == Some(region) && world.locations_at(pixel).next().is_none())
        .expect("a free spot nearby");

    let mut project = Project::default();
    let room_id = project.new_id();
    let start = original.blocks.iter().find(|block| block.is_starting_block).unwrap();
    let scene = Scene::from_rdb_block(&library.rdb_block(&start.block_name).unwrap());
    project.add(
        ProjectEntry::Room(ProjectRoom {
            id: room_id,
            name: "My start".to_string(),
            based_on: Some(start.block_name.clone()),
            // The cell's category, not the block it was copied from, gives its block name's letter.
            category: CellCategory::Water,
            objects: scene.objects,
        }),
        None,
    );
    let dungeon_id = project.add_empty_dungeon("Test Hold of Export");
    let dungeon = project.dungeon_mut(dungeon_id).unwrap();
    dungeon.dungeon_type = original.dungeon_type;
    dungeon.rooms = ProjectDungeonRoom::layout_of(original);
    for room in &mut dungeon.rooms {
        if room.is_starting_block {
            room.room = Some(room_id);
        }
    }
    dungeon.location = DungeonLocation { map_pixel: Some(pixel), ..DungeonLocation::copied_from(original) };

    let checks = check_project(&project, &mut library);
    assert!(checks[0].is_ready(), "{:?}", checks[0].problems);
    let details = ModDetails { title: "Export Test".to_string(), ..ModDetails::default() };
    let mod_files =
        build_mod(&project, &mut library, &details, "Fallback", &[dungeon_id], true).expect("the mod builds");
    let paths: Vec<&str> = mod_files.files.iter().map(|file| file.path.as_str()).collect();
    println!("{paths:#?}");
    assert_eq!(mod_files.file_name, "Export_Test");
    assert_eq!(mod_files.cells.len(), 1);
    let block_name = &mod_files.cells[0].block_name;
    assert!(block_name.starts_with('W') && block_name.ends_with(".RDB"), "{block_name}");
    assert!(paths.contains(&format!("WorldData/{block_name}.json").as_str()));
    let location_file = format!("WorldData/locationnew-Test_Hold_of_Export-{region}.json");
    let location = mod_files.files.iter().find(|file| file.path == location_file).expect("the location file");
    let location: serde_json::Value = serde_json::from_str(&location.contents).unwrap();
    let blocks = location["Dungeon"]["Blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), original.blocks.len());
    let start_block = blocks.iter().find(|block| block["IsStartingBlock"] == true).unwrap();
    assert_eq!(start_block["BlockName"], block_name.as_str());
    let exterior_blocks = location["Exterior"]["ExteriorData"]["BlockNames"].as_array().unwrap();
    assert_eq!(exterior_blocks.len(), original.exterior.as_ref().unwrap().block_names.len());
    assert_eq!(location["RegionIndex"], region);
    println!("climate {}", location["Climate"]);
    assert!((223..=232).contains(&location["Climate"]["WorldClimate"].as_i64().unwrap()));
    let manifest = mod_files.files.iter().find(|file| file.path == "Export_Test.dfmod.json").unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest.contents).unwrap();
    assert_eq!(manifest["Files"].as_array().unwrap().len(), mod_files.files.len());
    assert!(manifest["Files"][0].as_str().unwrap().starts_with("Assets/DungeonMaker/Export_Test/"));

    // Written to disk: the world data, the manifest and a readme.
    let folder = std::env::temp_dir().join(format!("ddm-export-{}", std::process::id()));
    let mod_folder =
        daggerfall_dungeon_maker::export::write_mod(&mod_files, &folder).expect("the mod is written");
    let written = |path: &str| mod_folder.join(path).is_file();
    let all_written = mod_files.files.iter().all(|file| written(&file.path)) && written("README.txt");
    std::fs::remove_dir_all(&folder).unwrap();
    assert!(all_written);

    // A scaled plain model: a .dfmod export leaves it out of the cell and has a
    // script put it back at its size; loose files keep it in the cell.
    let tags = library.model_tags();
    let model_count = |files: &daggerfall_dungeon_maker::export::ModFiles| {
        let block =
            files.files.iter().find(|file| file.path == format!("WorldData/{block_name}.json")).unwrap();
        let block: serde_json::Value = serde_json::from_str(&block.contents).unwrap();
        block["RdbBlock"]["ObjectRootList"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|root| root["RdbObjects"].as_array())
            .flatten()
            .filter(|object| object["Type"] == "Model")
            .count()
    };
    let unscaled_models = model_count(&mod_files);
    let room = project.room_mut(room_id).expect("the room is there");
    let index = (0..room.objects.len())
        .find(|&index| {
            matches!(room.objects[index].kind, daggerfall_dungeon_maker::scene::SceneObjectKind::Model { .. })
                && Scene::scale_blocker(&room.objects, index, &tags).is_none()
        })
        .expect("a model that can be scaled");
    room.objects[index].scale = Some(glam::Vec3::new(2.0, 0.5, 1.5));
    let checks = check_project(&project, &mut library);
    assert_eq!(checks[0].scaled_objects, 1);
    let scaled =
        build_mod(&project, &mut library, &details, "Fallback", &[dungeon_id], true).expect("the mod builds");
    assert_eq!(model_count(&scaled), unscaled_models - 1);
    assert_eq!(scaled.dungeons[0].scaled_objects, 1);
    let script = scaled
        .files
        .iter()
        .find(|file| file.path == "Export_TestScaledObjects.cs.txt")
        .expect("the scale script");
    assert!(script.contents.contains(&format!("{{ \"{block_name}\" }}")), "{}", script.contents);
    assert!(script.contents.contains("Scales = { 2f, 0.5f, 1.5f }"), "{}", script.contents);
    let loose = build_mod(&project, &mut library, &details, "Fallback", &[dungeon_id], false)
        .expect("the mod builds");
    assert_eq!(model_count(&loose), unscaled_models);
    // Every other object keeps its position (saved games know objects by it).
    let positions = |files: &daggerfall_dungeon_maker::export::ModFiles| {
        let block =
            files.files.iter().find(|file| file.path == format!("WorldData/{block_name}.json")).unwrap();
        let block: serde_json::Value = serde_json::from_str(&block.contents).unwrap();
        block["RdbBlock"]["ObjectRootList"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|root| root["RdbObjects"].as_array())
            .flatten()
            .map(|object| {
                let place = [&object["XPos"], &object["YPos"], &object["ZPos"]].map(|value| value.as_i64());
                (object["Position"].as_i64().unwrap(), place)
            })
            .collect::<std::collections::HashMap<_, _>>()
    };
    let (all, kept) = (positions(&loose), positions(&scaled));
    assert_eq!(kept.len() + 1, all.len());
    assert!(kept.iter().all(|(position, place)| all.get(position) == Some(place)), "positions moved");
    assert!(!loose.files.iter().any(|file| file.path.ends_with("ScaledObjects.cs.txt")));
    let room = project.room_mut(room_id).expect("the room is there");
    room.objects[index].scale = None;

    // A model of the user's own in the cell: a .dfmod carries its files and the
    // cell tags it so the game gives it a collider; loose files cannot carry it.
    {
        use daggerfall_dungeon_maker::custom_models::{CustomModelLibrary, FIRST_CUSTOM_MODEL_ID};
        use daggerfall_dungeon_maker::scene::{ModelData, SceneObjectKind};
        let models_folder = std::env::temp_dir().join(format!("ddm-export-models-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&models_folder);
        std::fs::create_dir_all(&models_folder).unwrap();
        let obj = models_folder.join("pillar.obj");
        std::fs::write(&obj, "v 0 0 0\nv 1 0 0\nv 0 2 0\nf 1 2 3\n").unwrap();
        let (mut custom_models, _) = CustomModelLibrary::open(models_folder.join("store"));
        let model_id = custom_models.import(&obj).expect("the model imports").model.id;
        assert_eq!(model_id, FIRST_CUSTOM_MODEL_ID);

        let room = project.room_mut(room_id).expect("the room is there");
        let mut pillar = room.objects[index].clone();
        pillar.kind = SceneObjectKind::Model { model_id, data: ModelData::default() };
        pillar.id = room.objects.iter().map(|object| object.id).max().unwrap() + 1;
        pillar.next = None;
        room.objects.push(pillar);
        // Not imported on this computer: the dungeon cannot be exported.
        let checks = check_project(&project, &mut library);
        assert!(
            checks[0].problems.iter().any(|problem| problem.contains("not imported")),
            "{:?}",
            checks[0].problems
        );

        custom_models.register(&mut library);
        let checks = check_project(&project, &mut library);
        assert!(checks[0].is_ready(), "{:?}", checks[0].problems);
        assert_eq!(checks[0].custom_models, 1);
        let with_model = build_mod(&project, &mut library, &details, "Fallback", &[dungeon_id], true)
            .expect("the mod builds");
        let copied: Vec<&str> = with_model.copied_files.iter().map(|file| file.path.as_str()).collect();
        assert!(copied.contains(&format!("Models/{model_id}_model.json").as_str()), "{copied:?}");
        assert!(copied.contains(&format!("Models/{model_id}_texture_0.png").as_str()), "{copied:?}");
        let block =
            with_model.files.iter().find(|file| file.path == format!("WorldData/{block_name}.json")).unwrap();
        let block: serde_json::Value = serde_json::from_str(&block.contents).unwrap();
        let reference = block["RdbBlock"]["ModelReferenceList"]
            .as_array()
            .unwrap()
            .iter()
            .find(|reference| reference["ModelIdNum"] == model_id)
            .expect("the cell refers to the model");
        assert_eq!(reference["Description"], "COL");
        let loose = build_mod(&project, &mut library, &details, "Fallback", &[dungeon_id], false)
            .expect("the mod builds");
        assert!(loose.copied_files.iter().all(|file| !file.path.starts_with("Models/")));

        project.room_mut(room_id).expect("the room is there").objects.pop();
        let _ = std::fs::remove_dir_all(&models_folder);
    }

    // Its name may not repeat a location of its region.
    project.dungeon_mut(dungeon_id).unwrap().name = original.name.clone();
    let checks = check_project(&project, &mut library);
    let problems = &checks[0].problems;
    assert!(problems.iter().any(|problem| problem.contains("already has a location")), "{problems:?}");
}

#[test]
fn reads_what_identifies_a_location_as_daggerfall_unity_does() {
    let Some(mut library) = open_library() else { return };

    // Daggerfall Unity's own reading of this location, saved by a published mod
    // (drcarademono/beautiful-villages-locations, location-0-2.json).
    let dungeons = library.dungeons();
    let ruins = dungeons
        .iter()
        .find(|dungeon| dungeon.region_index == 0 && dungeon.name == "Ruins of The Caorchom Farmstead")
        .expect("the ruins are in the Alik'r Desert");
    let classic = ruins.classic.expect("its game record is read");
    assert_eq!(classic.location_index, 2);
    assert_eq!(classic.map_id, 2_484_750);
    assert_eq!((classic.longitude, classic.latitude), (76_584, 14_376));
    assert_eq!((classic.location_id, classic.dungeon_location_id), (26_219, 26_220));
    assert_eq!((classic.world_x, classic.world_y), (19_609_600, 3_684_353));
    assert!(!classic.discovered);
    assert_eq!(classic.key, 0);
    let world = library.world_map().unwrap();
    assert_eq!(world.location_climate_at(ruins.map_pixel.unwrap()), Some(224));
}

#[test]
fn exports_a_dungeon_replacing_one_of_daggerfalls() {
    use daggerfall_dungeon_maker::export::{build_mod, check_project};
    use daggerfall_dungeon_maker::project::{
        DungeonLocation, DungeonReference, ModDetails, Project, ProjectDungeonRoom,
    };
    let Some(mut library) = open_library() else { return };

    let dungeons = library.dungeons();
    let original =
        dungeons.iter().find(|dungeon| dungeon.name == "Privateer's Hold").expect("Privateer's Hold");
    let classic = original.classic.expect("its game record is read");
    let mut project = Project::default();
    let dungeon_id = project.add_empty_dungeon("My Hold");
    let dungeon = project.dungeon_mut(dungeon_id).unwrap();
    dungeon.dungeon_type = original.dungeon_type;
    dungeon.rooms = ProjectDungeonRoom::layout_of(original);
    dungeon.location = DungeonLocation {
        map_pixel: original.map_pixel,
        replaces: Some(DungeonReference::to(original)),
        ..DungeonLocation::copied_from(original)
    };

    let checks = check_project(&project, &mut library);
    assert!(checks[0].is_ready(), "{:?}", checks[0].problems);
    let mod_files = build_mod(&project, &mut library, &ModDetails::default(), "Replace", &[dungeon_id], true)
        .expect("the mod builds");
    let path = format!("WorldData/location-{}-{}.json", original.region_index, classic.location_index);
    let file = mod_files.files.iter().find(|file| file.path == path).expect("the replacement file");
    let location: serde_json::Value = serde_json::from_str(&file.contents).unwrap();
    assert_eq!(location["Name"], "Privateer's Hold", "a replacement keeps the replaced name");
    assert_eq!(location["LocationIndex"], classic.location_index);
    assert_eq!(location["MapTableData"]["MapId"], classic.map_id);
    assert_eq!(location["Exterior"]["RecordElement"]["Header"]["LocationId"], classic.location_id);
    assert_eq!(location["Dungeon"]["RecordElement"]["Header"]["LocationId"], classic.dungeon_location_id);
    assert_eq!(mod_files.dungeons[0].replaces.as_deref(), Some("Privateer's Hold"));
    assert!(!mod_files.files.iter().any(|file| file.path.contains("locationnew-")));

    // With music of its own, the file goes into the mod with a script that plays it there.
    let music = std::env::temp_dir().join(format!("ddm-music-{}.ogg", std::process::id()));
    std::fs::write(&music, b"OggS").unwrap();
    let location = &mut project.dungeon_mut(dungeon_id).unwrap().location;
    location.custom_music = true;
    location.custom_music_file = music.display().to_string();
    let with_music =
        build_mod(&project, &mut library, &ModDetails::default(), "Replace", &[dungeon_id], true)
            .expect("the mod builds");
    assert_eq!(with_music.copied_files.len(), 1);
    assert_eq!(with_music.copied_files[0].path, "Music/replace_music_0.ogg");
    let script =
        with_music.files.iter().find(|file| file.path.ends_with(".cs.txt")).expect("the music script");
    assert!(script.contents.contains(&format!("{{ {} }}", original.region_index)), "{}", script.contents);
    assert!(script.contents.contains(r#"{ "Privateer's Hold" }"#));
    assert!(script.contents.contains(r#"{ "replace_music_0.ogg" }"#));
    let manifest = with_music.files.iter().find(|file| file.path.ends_with(".dfmod.json")).unwrap();
    assert!(manifest.contents.contains("Music/replace_music_0.ogg") && manifest.contents.contains(".cs.txt"));
    std::fs::remove_file(&music).unwrap();
    // A music file that is not there stops the export.
    let checks = check_project(&project, &mut library);
    assert!(
        checks[0].problems.iter().any(|problem| problem.contains("was not found")),
        "{:?}",
        checks[0].problems
    );
    project.dungeon_mut(dungeon_id).unwrap().location.custom_music = false;

    // With a skybox, the image goes into the mod with a script that shows it there.
    let sky = std::env::temp_dir().join(format!("ddm-sky-{}.png", std::process::id()));
    std::fs::write(&sky, b"\x89PNG").unwrap();
    let location = &mut project.dungeon_mut(dungeon_id).unwrap().location;
    location.skybox = true;
    location.skybox_file = sky.display().to_string();
    let with_sky = build_mod(&project, &mut library, &ModDetails::default(), "Replace", &[dungeon_id], true)
        .expect("the mod builds");
    assert_eq!(with_sky.copied_files.len(), 1);
    assert_eq!(with_sky.copied_files[0].path, "Sky/replace_sky_0.png");
    let script =
        with_sky.files.iter().find(|file| file.path == "ReplaceDungeonSky.cs.txt").expect("the sky script");
    assert!(script.contents.contains(r#"{ "Privateer's Hold" }"#));
    assert!(script.contents.contains(r#"{ "replace_sky_0.mat" }"#));
    assert!(script.contents.contains(r#"{ "replace_sky_0.png" }"#));
    assert!(!with_sky.files.iter().any(|file| file.path.ends_with("DungeonMusic.cs.txt")));
    // Written out, the image is in the mod's Sky folder.
    let out = std::env::temp_dir().join(format!("ddm-sky-export-{}", std::process::id()));
    let mod_folder =
        daggerfall_dungeon_maker::export::write_mod(&with_sky, &out).expect("the mod is written");
    assert!(mod_folder.join("Sky").join("replace_sky_0.png").is_file());
    std::fs::remove_dir_all(&out).unwrap();
    // An image that is not there stops the export.
    std::fs::remove_file(&sky).unwrap();
    let checks = check_project(&project, &mut library);
    assert!(
        checks[0]
            .problems
            .iter()
            .any(|problem| problem.contains("skybox image") && problem.contains("was not found")),
        "{:?}",
        checks[0].problems
    );
    project.dungeon_mut(dungeon_id).unwrap().location.skybox = false;

    // Another dungeon cannot take the same place.
    let other_id = project.add_empty_dungeon("Second Hold");
    let replaced = project.dungeon(dungeon_id).unwrap().location.clone();
    let other = project.dungeon_mut(other_id).unwrap();
    other.dungeon_type = original.dungeon_type;
    other.rooms = ProjectDungeonRoom::layout_of(original);
    other.location = replaced;
    let checks = check_project(&project, &mut library);
    let problems = &checks[1].problems;
    assert!(problems.iter().any(|problem| problem.contains("already at its spot")), "{problems:?}");
}

#[test]
fn a_castle_daggerfall_copy_keeps_its_foyer_doors_ids() {
    use daggerfall_dungeon_maker::export::{build_mod, check_project, CASTLE_DAGGERFALL_MAP_ID};
    use daggerfall_dungeon_maker::project::{
        CellCategory, DungeonLocation, DungeonReference, ModDetails, Project, ProjectDungeonRoom,
        ProjectEntry, ProjectRoom,
    };
    use daggerfall_dungeon_maker::scene::Scene;
    let Some(mut library) = open_library() else { return };

    let dungeons = library.dungeons();
    let castle = dungeons
        .iter()
        .find(|dungeon| dungeon.classic.is_some_and(|classic| classic.map_id == CASTLE_DAGGERFALL_MAP_ID))
        .expect("Castle Daggerfall");
    // Its start cell, S0000160, holds the two magically held foyer doors.
    let start = castle.blocks.iter().find(|block| block.is_starting_block).unwrap();
    assert_eq!(start.block_name, "S0000160.RDB");
    let mut project = Project::default();
    let room_id = project.new_id();
    let scene = Scene::from_rdb_block(&library.rdb_block(&start.block_name).unwrap());
    project.add(
        ProjectEntry::Room(ProjectRoom {
            id: room_id,
            name: "My foyer".to_string(),
            based_on: Some(start.block_name.clone()),
            category: CellCategory::Special,
            objects: scene.objects,
        }),
        None,
    );
    let dungeon_id = project.add_empty_dungeon("My Castle");
    let dungeon = project.dungeon_mut(dungeon_id).unwrap();
    dungeon.dungeon_type = castle.dungeon_type;
    dungeon.rooms = ProjectDungeonRoom::layout_of(castle);
    for room in &mut dungeon.rooms {
        if room.is_starting_block {
            room.room = Some(room_id);
        }
    }
    dungeon.location = DungeonLocation {
        map_pixel: castle.map_pixel,
        replaces: Some(DungeonReference::to(castle)),
        ..DungeonLocation::copied_from(castle)
    };
    let checks = check_project(&project, &mut library);
    assert!(checks[0].is_ready(), "{:?}", checks[0].problems);
    assert!(!checks[0].notes.iter().any(|note| note.contains("foyer")), "{:?}", checks[0].notes);

    // The copy keeps S0000160's block position and the doors' offsets, so the
    // game's ids for them (block position plus offset) stay 29331574 and 29331622.
    let mod_files = build_mod(&project, &mut library, &ModDetails::default(), "Castle", &[dungeon_id], false)
        .expect("the mod builds");
    let block_name = &mod_files.cells[0].block_name;
    let block =
        mod_files.files.iter().find(|file| file.path == format!("WorldData/{block_name}.json")).unwrap();
    let block: serde_json::Value = serde_json::from_str(&block.contents).unwrap();
    let block_position = block["Position"].as_i64().unwrap();
    assert_eq!(Some(block_position), library.block_position("S0000160.RDB"));
    let ids: Vec<i64> = block["RdbBlock"]["ObjectRootList"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|root| root["RdbObjects"].as_array())
        .flatten()
        .map(|object| block_position + object["Position"].as_i64().unwrap())
        .collect();
    assert!(ids.contains(&29_331_574) && ids.contains(&29_331_622));
    let unique: std::collections::HashSet<&i64> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "every object keeps its own id");

    // Moved, the doors cannot be found, and the export says the fix will not work.
    let room = project.room_mut(room_id).unwrap();
    for object in &mut room.objects {
        if matches!(
            object.kind,
            daggerfall_dungeon_maker::scene::SceneObjectKind::Model { model_id: 55004, .. }
        ) {
            object.position.x += 1.0;
        }
    }
    let checks = check_project(&project, &mut library);
    assert!(checks[0].notes.iter().any(|note| note.contains("foyer")), "{:?}", checks[0].notes);
}

#[test]
fn finds_doors_the_game_switches_off_and_dungeon_textures() {
    use daggerfall_dungeon_maker::doors::{dungeon_switched_off, DoorCell};
    use daggerfall_dungeon_maker::dungeon_textures::{uses_dungeon_textures, DungeonTextures, TextureMode};
    use daggerfall_dungeon_maker::export::{cell_origin, daggerfall_dungeon_identity, dungeon_cells};
    use daggerfall_dungeon_maker::project::{Project, ProjectDungeonRoom};
    let Some(mut library) = open_library() else { return };

    let dungeons = library.dungeons();
    let world = library.world_map();
    let tags = library.model_tags();
    let tower = dungeons.iter().find(|dungeon| dungeon.name == "Bechur Tower").expect("Bechur Tower");
    let layout = ProjectDungeonRoom::layout_of(tower);
    let project = Project::default();
    let cells = dungeon_cells(&layout, &project, &mut library);
    let door_cells: Vec<DoorCell<'_>> = cells
        .iter()
        .map(|(placement, objects)| DoorCell {
            objects,
            origin: cell_origin(placement),
            is_start: placement.is_starting_block,
        })
        .collect();
    let off = dungeon_switched_off(&door_cells, &tags, &mut library);
    assert_eq!(off.len(), 5, "{off:?}");

    // Its textures are Daggerfall's classic ones for its ids and climate, and
    // every texture its models swap to is in the game data.
    let identity = daggerfall_dungeon_identity(tower, world.as_deref()).expect("its ids");
    let textures = DungeonTextures::for_dungeon(&identity, TextureMode::Classic);
    assert_ne!(textures.table, daggerfall_dungeon_maker::dungeon_textures::STANDARD_TABLE);
    for (_, objects) in &cells {
        for object in objects.iter().filter(|object| uses_dungeon_textures(object)) {
            let daggerfall_dungeon_maker::scene::SceneObjectKind::Model { model_id, .. } = object.kind else {
                continue;
            };
            let Some(mesh) = library.model(model_id) else { continue };
            for submesh in &mesh.submeshes {
                let key = textures.apply(submesh.texture);
                assert!(library.texture_image(key).is_some(), "{key} for model {model_id}");
            }
        }
    }
}

#[test]
fn finds_openings_that_lead_to_empty_space() {
    use std::collections::HashMap;

    use daggerfall_dungeon_maker::connectors::unclosed_faces;
    use daggerfall_dungeon_maker::scene::SceneObject;
    let Some(mut library) = open_library() else { return };

    let mut cache = HashMap::new();

    // A corridor on its own leads to empty space at both ends.
    let corridor = SceneObject {
        id: 1,
        name: String::new(),
        group_index: 0,
        kind: daggerfall_dungeon_maker::scene::SceneObjectKind::Model {
            model_id: 67001,
            data: Default::default(),
        },
        position: glam::Vec3::new(25.6, 0.0, 25.6),
        rotation_degrees: glam::Vec3::ZERO,
        visible: true,
        next: None,
        scale: None,
        prefab: None,
        folder: None,
    };
    let found = unclosed_faces(std::slice::from_ref(&corridor), |_| true, &mut library, &mut cache);
    let normals: Vec<glam::Vec3> = found.iter().map(|face| face.face.normal.round()).collect();
    assert_eq!(found.len(), 2, "{normals:?}");
    assert!(normals.contains(&glam::Vec3::Z) && normals.contains(&-glam::Vec3::Z), "{normals:?}");

    // Another corridor joined to one end closes it; the far ends stay open.
    // (Each corridor's far end is all that is left.)
    let joined = SceneObject {
        id: 2,
        position: corridor.position + glam::Vec3::new(0.0, 0.0, 12.8),
        ..corridor.clone()
    };
    let found = unclosed_faces(&[corridor, joined], |_| true, &mut library, &mut cache);
    assert_eq!(found.len(), 2);
}

#[test]
fn reads_daggerfalls_dungeon_songs() {
    use daggerfall_dungeon_maker::arena2::parse_hmi;
    use daggerfall_dungeon_maker::music::{song_for_seed, DUNGEON_SONGS};
    let Some(mut library) = open_library() else { return };

    for song in DUNGEON_SONGS {
        let data = library.midi_song(song.file).expect("the song is in MIDI.BSA");
        let parsed = parse_hmi(&data).expect("the song reads");
        let duration = parsed.duration();
        let notes = parsed.timeline().iter().filter(|(_, message)| message[0] & 0xF0 == 0x90).count();
        println!("{}: {notes} note events, {duration:.0} s", song.label());
        assert!(notes > 50, "{} has notes", song.file);
        assert!((10.0..900.0).contains(&duration), "{} lasts {duration} s", song.file);
    }

    // Daggerfall's dungeons between them play every dungeon song.
    let dungeons = library.dungeons();
    let mut played = std::collections::BTreeMap::new();
    for dungeon in dungeons.iter() {
        if let Some(classic) = dungeon.classic {
            *played
                .entry(song_for_seed(classic.dungeon_music_seed, dungeon.region_index).name)
                .or_insert(0) += 1;
        }
    }
    println!("{played:?}");
    assert_eq!(played.len(), DUNGEON_SONGS.len());
}

#[test]
fn corrects_the_cells_daggerfall_unity_corrects() {
    use daggerfall_dungeon_maker::arena2::{unity_fix_description, RdbObject};
    let Some(library) = open_library() else { return };
    let block = |name: &str| library.rdb_block(name).expect("the cell reads");
    let in_list = |objects: &[RdbObject], list: usize| -> Vec<RdbObject> {
        objects.iter().filter(|object| object.group_index == list).cloned().collect()
    };
    let is_model_at = |object: &RdbObject, x: i32, y: i32, z: i32| {
        matches!(object.kind, RdbObjectKind::Model(_)) && object.position == glam::IVec3::new(x, y, z)
    };
    let marker = |object: &RdbObject, record: u8| matches!(&object.kind, RdbObjectKind::Flat(flat) if flat.texture.archive == 199 && flat.texture.record == record);

    for name in ["N0000022.RDB", "N0000023.RDB"] {
        assert!(in_list(&block(name).objects, 2).is_empty(), "{name} loses its bad door");
    }
    // The door and the lever that are moved.
    let door = in_list(&block("N0000035.RDB").objects, 0)
        .into_iter()
        .filter(|object| object.position.y == -300)
        .count();
    assert_eq!(door, 1, "N0000035.RDB has its door at the corrected height");
    let lever = in_list(&block("N0000052.RDB").objects, 0)
        .into_iter()
        .find(|object| object.position.x == 543)
        .expect("N0000052.RDB has its lever in its corrected place");
    assert!(
        matches!(&lever.kind, RdbObjectKind::Model(model) if model.action.is_some()),
        "the lever has an action"
    );
    assert!(in_list(&block("N0000071.RDB").objects, 4).iter().any(|object| object.position.x == 640));

    // W0000009: the exit, the start marker and the new wall all move to the corner.
    let w9 = block("W0000009.RDB");
    let wall = in_list(&w9.objects, 8)
        .into_iter()
        .find(|object| matches!(&object.kind, RdbObjectKind::Model(model) if model.model_id == 72100))
        .expect("W0000009.RDB gets a brick wall");
    let start =
        in_list(&w9.objects, 4).into_iter().find(|object| marker(object, 10)).expect("a start marker");
    assert_eq!((start.position.x, start.position.z + 128), (wall.position.x, wall.position.z));
    let exit = in_list(&w9.objects, 8).into_iter().next().expect("the exit");
    assert_eq!((exit.position.x, exit.position.z + 2), (wall.position.x, wall.position.z));
    println!("W0000009 corner {:?}, exit {:?}", start.position, exit.position);

    assert!(in_list(&block("W0000018.RDB").objects, 6)
        .iter()
        .any(|object| is_model_at(object, 1152, -1792, 1920)));
    assert!(in_list(&block("W0000020.RDB").objects, 5)
        .iter()
        .any(|object| is_model_at(object, 1152, -384, 1408)));

    assert!(unity_fix_description("w0000020.rdb").is_some());
    assert!(unity_fix_description("N0000004.RDB").is_none());
}

#[test]
fn names_the_sounds_texts_spells_and_factions_cells_refer_to() {
    use std::collections::BTreeMap;
    let Some(library) = open_library() else { return };
    let references = library.references();
    let sounds = references.sounds.as_ref().expect("DAGGER.SND reads");
    let texts = references.texts.as_ref().expect("TEXT.RSC reads");
    assert!(texts.get(8605).is_some_and(|text| text.contains("Blind God")));
    assert_eq!(references.spells.get(&1).map(String::as_str), Some("Fenrik's Door Jam"));
    assert!(references.factions.len() > 300, "{} factions", references.factions.len());

    // What Daggerfall's own cells refer to.
    let mut sound_uses: BTreeMap<u8, usize> = BTreeMap::new();
    let (mut texts_shown, mut texts_missing, mut spells_cast, mut spells_missing) = (0, 0, 0, 0);
    let (mut faction_people, mut faction_missing) = (0, Vec::new());
    for name in library.rdb_block_names() {
        let Ok(block) = library.rdb_block(&name) else { continue };
        for object in &block.objects {
            match &object.kind {
                RdbObjectKind::Model(model) => {
                    let Some(action) = model.action else { continue };
                    if model.sound > 0 {
                        *sound_uses.entry(model.sound).or_default() += 1;
                    }
                    match action.action_type {
                        0x0B => match texts.get(8600 + u16::from(model.sound)) {
                            Some(_) => texts_shown += 1,
                            None => texts_missing += 1,
                        },
                        0x09 => match references.spells.get(&model.sound) {
                            Some(_) => spells_cast += 1,
                            None => spells_missing += 1,
                        },
                        _ => {}
                    }
                }
                RdbObjectKind::Flat(flat)
                    if daggerfall_dungeon_maker::catalog::PEOPLE_FLAT_ARCHIVES
                        .contains(&flat.texture.archive)
                        && flat.faction_or_mobile_id != 0 =>
                {
                    match references.factions.get(&flat.faction_or_mobile_id) {
                        Some(_) => faction_people += 1,
                        None => faction_missing.push(flat.faction_or_mobile_id),
                    }
                }
                _ => {}
            }
        }
    }
    let unnamed: Vec<u8> =
        sound_uses.keys().copied().filter(|&id| sounds.name(u32::from(id)).is_none()).collect();
    for (id, uses) in &sound_uses {
        println!("sound {id}: {uses} uses, {:?}", sounds.name(u32::from(*id)));
    }
    println!("sounds with no sound data: {unnamed:?}");
    println!("show text: {texts_shown} found, {texts_missing} missing; cast spell: {spells_cast} found, {spells_missing} missing");
    println!("people with factions: {faction_people} found, missing {faction_missing:?}");
    assert!(texts_shown > 0 && spells_cast > 0 && faction_people > 0);
}

#[test]
fn export_check_needs_a_marker_to_arrive_at_and_notes_missing_quest_markers() {
    use daggerfall_dungeon_maker::arena2::MapPixel;
    use daggerfall_dungeon_maker::export::check_project;
    use daggerfall_dungeon_maker::project::{
        CellCategory, DungeonLocation, Project, ProjectDungeonRoom, ProjectEntry, ProjectRoom,
    };
    use daggerfall_dungeon_maker::scene::{
        Scene, ENTER_MARKER_RECORD, QUEST_ITEM_MARKER_RECORD, QUEST_MARKER_RECORD, START_MARKER_RECORD,
    };
    let Some(mut library) = open_library() else { return };

    let dungeons = library.dungeons();
    let original =
        dungeons.iter().find(|dungeon| dungeon.name == "Privateer's Hold").expect("Privateer's Hold");
    let world = library.world_map().unwrap();
    let home = original.map_pixel.unwrap();
    let pixel = (-20..=20)
        .flat_map(|dx| (-20..=20).map(move |dy| MapPixel::new(home.x + dx, home.y + dy)))
        .find(|&pixel| {
            world.region_at(pixel) == Some(original.region_index)
                && world.locations_at(pixel).next().is_none()
        })
        .expect("a free spot nearby");
    let start = original.blocks.iter().find(|block| block.is_starting_block).unwrap();
    let objects = Scene::from_rdb_block(&library.rdb_block(&start.block_name).unwrap()).objects;
    let check = |keep: &dyn Fn(&daggerfall_dungeon_maker::scene::SceneObject) -> bool,
                 library: &mut daggerfall_dungeon_maker::arena2::Arena2Library| {
        let mut project = Project::default();
        let room_id = project.new_id();
        let objects = objects.iter().filter(|object| keep(object)).cloned().collect();
        let room = ProjectRoom {
            id: room_id,
            name: "Start".to_string(),
            based_on: None,
            category: CellCategory::Interior,
            objects,
        };
        project.add(ProjectEntry::Room(room), None);
        let dungeon_id = project.add_empty_dungeon("Test Hold of Markers");
        let dungeon = project.dungeon_mut(dungeon_id).unwrap();
        dungeon.dungeon_type = original.dungeon_type;
        dungeon.rooms = vec![ProjectDungeonRoom {
            block_name: String::new(),
            grid_x: 0,
            grid_z: 0,
            is_starting_block: true,
            room: Some(room_id),
        }];
        dungeon.location =
            DungeonLocation { map_pixel: Some(pixel), ..DungeonLocation::copied_from(original) };
        check_project(&project, library).remove(0)
    };
    let mentions = |lines: &[String], text: &str| lines.iter().any(|line| line.contains(text));

    let has_quest_markers = objects.iter().any(|object| {
        object.is_marker_record(QUEST_MARKER_RECORD) || object.is_marker_record(QUEST_ITEM_MARKER_RECORD)
    });
    assert!(has_quest_markers, "Privateer's Hold's start cell has quest markers");
    let whole = check(&|_| true, &mut library);
    assert!(whole.is_ready(), "{:?}", whole.problems);
    assert!(!mentions(&whole.notes, "quest marker"), "{:?}", whole.notes);

    // Without its quest markers quests skip it, which is a note, not a problem.
    let no_quests = check(
        &|object| {
            !object.is_marker_record(QUEST_MARKER_RECORD)
                && !object.is_marker_record(QUEST_ITEM_MARKER_RECORD)
        },
        &mut library,
    );
    assert!(no_quests.is_ready(), "{:?}", no_quests.problems);
    assert!(mentions(&no_quests.notes, "quest marker"), "{:?}", no_quests.notes);

    // Without a start or an enter marker the game cannot load it.
    let nowhere = check(
        &|object| {
            !object.is_marker_record(START_MARKER_RECORD) && !object.is_marker_record(ENTER_MARKER_RECORD)
        },
        &mut library,
    );
    assert!(mentions(&nowhere.problems, "no start marker or enter marker"), "{:?}", nowhere.problems);
}

#[test]
fn reads_every_buildings_inside_and_entrances() {
    use daggerfall_dungeon_maker::arena2::DoorKind;
    use daggerfall_dungeon_maker::buildings::{
        arrival_exit, door_boxes, dungeon_textured_entrances, entrance_centers, interior_exits,
        interior_layout, landing_markers,
    };
    let Some(mut library) = open_library() else { return };
    let (mut buildings, mut with_inside, mut models, mut people, mut doors) = (0, 0, 0, 0, 0);
    let mut without_entrance = Vec::new();
    for name in library.rmb_block_names() {
        let block = library.rmb_block(&name).expect("the outdoor cell reads");
        for (record, building) in block.buildings.iter().enumerate() {
            buildings += 1;
            models += building.interior.models.len();
            people += building.interior.people.len();
            doors += building.interior.doors.len();
            if !building.has_interior() {
                continue;
            }
            with_inside += 1;
            let entrances: usize = building
                .models
                .clone()
                .filter_map(|model| library.model(block.models[model].model_id))
                .map(|mesh| door_boxes(&mesh, DoorKind::Building).len())
                .sum();
            if entrances == 0 {
                without_entrance.push(format!("{name} #{record}"));
            }
        }
    }
    println!(
        "{buildings} buildings, {with_inside} with an inside: {models} models, {people} people, {doors} doors; \
         {} without an entrance: {:?}",
        without_entrance.len(),
        &without_entrance[..without_entrance.len().min(10)]
    );
    // As counted straight from BLOCKS.BSA.
    assert_eq!((buildings, models, people, doors), (9005, 226_268, 14_174, 25_922));
    assert!(without_entrance.len() * 100 < with_inside, "almost every inside has an entrance");

    // Every inside has exits. Walking in, Daggerfall Unity stands the inside in
    // the space of the door's model, so each door lands on an exit there.
    let (mut exits, mut without_exit, mut doors_in, mut far_doors) = (0, Vec::new(), 0, Vec::new());
    let (mut raised, mut rerouted) = (Vec::new(), Vec::new());
    for name in library.rmb_block_names() {
        let block = library.rmb_block(&name).expect("the outdoor cell reads");
        for (record, building) in block.buildings.iter().enumerate() {
            if !building.has_interior() {
                continue;
            }
            let layout = interior_layout(&block, record, &mut library);
            if layout.raised {
                raised.push(format!("{name} #{}", record + 1));
            }
            let inside_exits = interior_exits(&layout.scene, &mut library);
            rerouted.extend(
                dungeon_textured_entrances(&block, record, &mut library)
                    .iter()
                    .map(|door| format!("{name} #{}: {:.1?}", record + 1, door.center())),
            );
            exits += inside_exits.len();
            if inside_exits.is_empty() {
                without_exit.push(format!("{name} #{record}"));
            }
            let markers = landing_markers(&building.interior);
            for door in entrance_centers(&block, record, &mut library) {
                doors_in += 1;
                let nearest = inside_exits
                    .iter()
                    .map(|exit| exit.center().distance(door))
                    .fold(f32::INFINITY, f32::min);
                assert!(arrival_exit(door, &markers, &inside_exits).is_some() || inside_exits.is_empty());
                // Most are within a metre; some insides' doors are set back a little.
                if nearest > 5.0 {
                    far_doors.push(format!("{name} #{record}: {nearest:.1} m"));
                }
            }
        }
    }
    println!(
        "{exits} exits; {} insides without one: {:?}; {} of {doors_in} doors more than 5 m from an exit: {:?}",
        without_exit.len(),
        &without_exit[..without_exit.len().min(10)],
        far_doors.len(),
        &far_doors[..far_doors.len().min(20)]
    );
    assert!(without_exit.is_empty(), "every inside has an exit");

    // The insides that store their rooms by their floor, raised to fit.
    println!("raised: {raised:?}");
    assert_eq!(
        raised,
        [
            "BOOKAS00.RMB #9",
            "CUSTAA05.RMB #1",
            "CUSTAA05.RMB #2",
            "CUSTAA05.RMB #5",
            "CUSTAA05.RMB #6",
            "CUSTAA05.RMB #7",
            "LIBRAM00.RMB #8"
        ]
    );
    // Doors drawn as dungeon doors that lead into their building's inside:
    // the tops of Castle Daggerfall's two big towers and the three small towers' doors.
    println!("dungeon-textured entrances: {rerouted:?}");
    assert_eq!(rerouted.len(), 8);
    assert!(rerouted.iter().all(|door| door.starts_with("CUSTAA05.RMB")));
    assert!(far_doors.len() * 100 < doors_in, "doors land on an exit inside");

    // A tavern's inside: its people and markers, laid out.
    let block = library.rmb_block("TVRNAL01.RMB").unwrap();
    let (record, tavern) =
        block.buildings.iter().enumerate().find(|(_, building)| building.data.building_type == 15).unwrap();
    let scene = interior_layout(&block, record, &mut library).scene;
    assert_eq!(
        scene.objects.len(),
        tavern.interior.models.len()
            + tavern.interior.doors.len()
            + tavern.interior.flats.len()
            + tavern.interior.people.len()
    );
    assert!(!tavern.interior.people.is_empty(), "the tavern has people");
    let (minimum, maximum) = scene.bounds(&mut library).unwrap();
    println!("tavern inside: {} objects, {:?} to {:?}", scene.objects.len(), minimum, maximum);
    assert!((maximum - minimum).max_element() < 120.0, "the inside is the size of a building");
}

#[test]
fn names_settlements_buildings_as_daggerfall_unity_does() {
    use daggerfall_dungeon_maker::buildings::{
        settlement_buildings, BuildingNamer, BANK, GUILD_HALL, PALACE, TEMPLE,
    };
    use daggerfall_dungeon_maker::unity_install::{find_install_folder, UnityText};
    let Some(mut library) = open_library() else { return };
    let locations = library.locations();
    let daggerfall = locations.iter().find(|location| location.name == "Daggerfall").expect("Daggerfall");
    assert!(daggerfall.exterior.port_town, "Daggerfall is a port");
    let blocks: Vec<_> =
        daggerfall.exterior.block_names.iter().map(|name| library.rmb_block(name).ok()).collect();
    let cells: Vec<_> = blocks.iter().map(Option::as_ref).collect();
    let buildings: Vec<_> = settlement_buildings(daggerfall, &cells).into_iter().flatten().collect();
    // Every named building of the city's list goes to a building of its cells.
    let named =
        daggerfall.exterior.buildings.iter().filter(|building| {
            daggerfall_dungeon_maker::buildings::is_named_building(building.building_type)
        });
    for listed in named {
        assert!(buildings.iter().any(|building| building.name_seed == listed.name_seed
            && building.building_type == listed.building_type));
    }

    // Names need Daggerfall Unity's word lists: from DAGGERFALL_UNITY, or found where it is usually unzipped.
    let hints: Vec<std::path::PathBuf> =
        std::env::var_os("DAGGERFALL_UNITY").map(Into::into).into_iter().collect();
    let Some(unity) = find_install_folder(&hints).and_then(|folder| UnityText::load(&folder).ok()) else {
        println!("Daggerfall Unity not found; names not checked");
        return;
    };
    let references = library.references();
    let namer = BuildingNamer { unity: Some(&unity), references: &references };
    let name_of = |building_type: u8| {
        buildings
            .iter()
            .filter(|building| building.building_type == building_type)
            .filter_map(|building| namer.name(building, &daggerfall.name, daggerfall.region_index))
            .collect::<Vec<_>>()
    };
    println!(
        "palace {:?}, banks {:?}, guilds {:?}, temples {:?}",
        name_of(PALACE),
        name_of(BANK),
        name_of(GUILD_HALL),
        name_of(TEMPLE)
    );
    assert!(name_of(PALACE).iter().any(|name| name == "Castle Daggerfall"));
    assert!(name_of(BANK).iter().all(|name| name == "The Bank of Daggerfall"));
    assert!(name_of(GUILD_HALL).iter().any(|name| name == "The Mages Guild"));
    assert!(name_of(TEMPLE).iter().any(|name| name.starts_with("The Temple of")));
    let unnamed = buildings
        .iter()
        .filter(|building| daggerfall_dungeon_maker::buildings::is_named_building(building.building_type))
        .filter(|building| namer.name(building, &daggerfall.name, daggerfall.region_index).is_none())
        .count();
    assert_eq!(unnamed, 0, "every named building of Daggerfall gets a name");
}

#[test]
fn tells_furniture_town_maps_and_who_people_are() {
    use daggerfall_dungeon_maker::arena2::{AUTOMAP_SIDE, WORLD_SCALE};
    use daggerfall_dungeon_maker::buildings::{furniture_use, interior_layout, FurnitureUse};
    use daggerfall_dungeon_maker::people::{outdoor_gender, person_details, PersonName, PersonPlace};
    use daggerfall_dungeon_maker::scene::SceneObjectKind;
    use daggerfall_dungeon_maker::unity_install::{find_install_folder, UnityText};
    use std::collections::BTreeMap;
    let Some(mut library) = open_library() else { return };
    let references = library.references();
    assert!(references.flats.len() > 200, "FLATS.CFG reads: {} images", references.flats.len());

    // Furniture of every inside, and every cell's town map, north up: a
    // building's first model stands on a pixel of its type (plus 1) in most cells.
    let (mut furniture, mut on_its_pixel, mut buildings) = (BTreeMap::new(), 0, 0);
    for name in library.rmb_block_names() {
        let block = library.rmb_block(&name).expect("the outdoor cell reads");
        assert_eq!(block.automap.len(), AUTOMAP_SIDE * AUTOMAP_SIDE, "{name} has its town map");
        for (record, building) in block.buildings.iter().enumerate() {
            if let Some(model) = block.models.get(building.models.start) {
                let pixel = |value: f32| (value / WORLD_SCALE / 64.0).floor() as usize;
                let (x, z) = (pixel(model.position.x), pixel(model.position.z));
                if x < AUTOMAP_SIDE && z < AUTOMAP_SIDE {
                    buildings += 1;
                    let byte = block.automap[(AUTOMAP_SIDE - 1 - z) * AUTOMAP_SIDE + x];
                    on_its_pixel += usize::from(byte == building.data.building_type + 1);
                }
            }
            if !building.has_interior() {
                continue;
            }
            let layout = interior_layout(&block, record, &mut library);
            for &index in &layout.props {
                let SceneObjectKind::Model { model_id, .. } = layout.scene.objects[index].kind else {
                    continue;
                };
                if let Some(kind) = furniture_use(model_id, building.data.building_type) {
                    *furniture.entry(kind.name()).or_insert(0) += 1;
                }
            }
        }
    }
    println!("furniture {furniture:?}; {on_its_pixel} of {buildings} buildings on their town map pixel");
    for kind in
        [FurnitureUse::ShopShelf, FurnitureUse::Bookshelf, FurnitureUse::Container, FurnitureUse::Ladder]
    {
        assert!(furniture.get(kind.name()).is_some_and(|&count| count > 0), "some {}", kind.name());
    }
    assert!(on_its_pixel * 4 > buildings * 3, "town maps are north up");

    // People outdoors: their genders from FLATS.CFG, and their names in a
    // settlement with some when Daggerfall Unity's word lists are found.
    let hints: Vec<std::path::PathBuf> =
        std::env::var_os("DAGGERFALL_UNITY").map(Into::into).into_iter().collect();
    let unity = find_install_folder(&hints).and_then(|folder| UnityText::load(&folder).ok());
    let is_person = |flat: &&daggerfall_dungeon_maker::arena2::RmbFlat| {
        flat.faction_id != 0 && flat.texture.archive != 199
    };
    let mut with_people = std::collections::HashSet::new();
    for name in library.rmb_block_names() {
        let block = library.rmb_block(&name).expect("the outdoor cell reads");
        if block.flats.iter().any(|flat| is_person(&flat)) {
            with_people.insert(name.to_uppercase());
        }
    }
    let locations = library.locations();
    let settlement = locations
        .iter()
        .find(|location| {
            location.exterior.block_names.iter().any(|name| with_people.contains(&name.to_uppercase()))
        })
        .expect("a settlement has people outdoors");
    let place = PersonPlace {
        region_index: settlement.region_index,
        index_in_region: settlement.index_in_region,
        building_key: 0,
    };
    let (mut genders, mut names) = (BTreeMap::new(), Vec::new());
    for name in &settlement.exterior.block_names {
        let block = library.rmb_block(name).expect("the settlement's cells read");
        for flat in block.flats.iter().filter(is_person) {
            let gender = outdoor_gender(flat.texture, flat.flags, &references.flats);
            *genders.entry(gender.name()).or_insert(0) += 1;
            let person = person_details(
                flat.texture,
                flat.faction_id,
                gender,
                flat.record_offset,
                Ok(place),
                unity.as_ref(),
                &references,
            );
            names.push(person.name);
        }
    }
    println!(
        "{} cells have people outdoors; in {}: {genders:?}; {:?}",
        with_people.len(),
        settlement.name,
        &names[..names.len().min(8)]
    );
    assert!(!genders.is_empty());

    // The people inside Daggerfall's buildings, named for where they stand.
    let daggerfall = locations.iter().find(|location| location.name == "Daggerfall").expect("Daggerfall");
    let width = daggerfall.exterior.width.max(1);
    let mut inside = Vec::new();
    for (cell, name) in daggerfall.exterior.block_names.iter().enumerate() {
        let block = library.rmb_block(name).expect("Daggerfall's cells read");
        for (record, building) in block.buildings.iter().enumerate() {
            let place = PersonPlace {
                region_index: daggerfall.region_index,
                index_in_region: daggerfall.index_in_region,
                building_key: daggerfall_dungeon_maker::people::building_key(
                    cell % width,
                    cell / width,
                    record,
                ),
            };
            for person in &building.interior.people {
                let gender = daggerfall_dungeon_maker::people::Gender::from_flags(person.flags);
                let details = person_details(
                    person.texture,
                    person.faction_id,
                    gender,
                    person.record_offset,
                    Ok(place),
                    unity.as_ref(),
                    &references,
                );
                inside.push((gender.name(), details.name));
            }
        }
    }
    println!("{} people inside Daggerfall's buildings: {:?}", inside.len(), &inside[..inside.len().min(6)]);
    assert!(inside.len() > 100);
    if unity.is_some() {
        assert!(inside.iter().all(|(_, name)| matches!(name, PersonName::Named(_) | PersonName::MadeUp(_))));
    }
    if unity.is_some() {
        assert!(names.iter().all(|name| matches!(name, PersonName::Named(_) | PersonName::MadeUp(_))));
    }
}

#[test]
fn knows_factions_groups_rulers_and_dungeon_people() {
    use daggerfall_dungeon_maker::arena2::{
        guild_group_name, ruler_title_name, social_group_name, INDIVIDUAL_FACTION_KIND, PROVINCE_FACTION_KIND,
    };
    use daggerfall_dungeon_maker::people::{
        is_dungeon_person, person_details, Gender, PersonName, PersonPlace, PersonRace, Race,
    };
    use daggerfall_dungeon_maker::unity_install::{find_install_folder, UnityText};
    let Some(mut library) = open_library() else { return };
    let references = library.references();

    // Daggerfall's province: its ruler's title, groups, race and court.
    let daggerfall = references
        .factions
        .values()
        .find(|faction| faction.kind == PROVINCE_FACTION_KIND && faction.name == "Daggerfall")
        .expect("Daggerfall's province");
    assert_eq!(ruler_title_name(daggerfall.ruler), "King");
    assert_eq!(social_group_name(daggerfall.social_group), "Nobility");
    assert_eq!(guild_group_name(daggerfall.guild_group), "Region");
    assert_eq!(daggerfall.race, 3, "Breton");
    let court: Vec<&str> = daggerfall
        .children
        .iter()
        .filter_map(|child| references.factions.get(child))
        .filter(|child| child.kind == INDIVIDUAL_FACTION_KIND)
        .map(|child| child.name.as_str())
        .collect();
    assert!(court.contains(&"King Gothryd"), "{court:?}");

    // The people of Daggerfall's dungeons: who they are, named for where they stand.
    let hints: Vec<std::path::PathBuf> =
        std::env::var_os("DAGGERFALL_UNITY").map(Into::into).into_iter().collect();
    let unity = find_install_folder(&hints).and_then(|folder| UnityText::load(&folder).ok());
    let dungeons = library.dungeons();
    let (mut people, mut named, mut made_up) = (0, Vec::new(), Vec::new());
    let mut by_race = std::collections::BTreeMap::new();
    for dungeon in dungeons.iter().filter(|dungeon| dungeon.classic.is_some()) {
        let classic = dungeon.classic.unwrap();
        let place = PersonPlace {
            region_index: dungeon.region_index,
            index_in_region: classic.location_index,
            building_key: 0,
        };
        for placement in &dungeon.blocks {
            let Ok(block) = library.rdb_block(&placement.block_name) else { continue };
            for object in &block.objects {
                let daggerfall_dungeon_maker::arena2::RdbObjectKind::Flat(flat) = &object.kind else {
                    continue;
                };
                if !is_dungeon_person(flat.texture) {
                    continue;
                }
                people += 1;
                assert!(flat.resource_offset > 0);
                let person = person_details(
                    flat.texture,
                    flat.faction_or_mobile_id,
                    Gender::from_flags(flat.flags as u8),
                    flat.resource_offset,
                    Ok(place),
                    unity.as_ref(),
                    &references,
                );
                let race = match person.race {
                    PersonRace::Faction(race) | PersonRace::Region(race) => race.name(),
                    PersonRace::RegionUnknown => "unknown",
                };
                *by_race.entry(race).or_insert(0) += 1;
                match person.name {
                    PersonName::Named(name) => named.push(name),
                    PersonName::MadeUp(name) => made_up.push(name),
                    PersonName::Unknown(_) => {}
                }
            }
        }
    }
    named.sort();
    named.dedup();
    println!(
        "{people} people in Daggerfall's dungeons; races {by_race:?}; named {:?}; made up {:?}",
        &named[..named.len().min(10)],
        &made_up[..made_up.len().min(6)]
    );
    assert!(named.iter().any(|name| name == "King Eadwyre"), "Wayrest's king stands in its castle");
    assert!(by_race.contains_key(Race::Breton.name()));
    if unity.is_some() {
        assert!(!made_up.is_empty());
    }
}

#[test]
fn lights_gates_and_treasure_as_the_game_has_them() {
    use daggerfall_dungeon_maker::buildings::interior_layout;
    use daggerfall_dungeon_maker::encounters::DungeonType;
    use daggerfall_dungeon_maker::lighting::{scene_lights, GameLighting, LIGHTS_ARCHIVE};
    use daggerfall_dungeon_maker::loot::loot_table;
    use daggerfall_dungeon_maker::scene::{Scene, SceneObjectKind};
    let Some(mut library) = open_library() else { return };

    // Settlements' gates: models 446 (open) and 447 (shut).
    let mut gates = std::collections::BTreeMap::new();
    let mut lamps_outdoors = 0;
    for name in library.rmb_block_names() {
        let block = library.rmb_block(&name).expect("the outdoor cell reads");
        for model in block.models.iter().filter(|model| matches!(model.model_id, 446 | 447)) {
            *gates.entry(model.model_id).or_insert(0) += 1;
        }
        lamps_outdoors += block.flats.iter().filter(|flat| flat.texture.archive == LIGHTS_ARCHIVE).count();
    }
    println!("gates {gates:?}; {lamps_outdoors} lamps outdoors");
    assert!(gates.values().sum::<usize>() > 0, "settlements have gates");
    assert!(lamps_outdoors > 0);

    // A tavern's lamps light its inside, each one a light.
    let block = library.rmb_block("TVRNAL01.RMB").unwrap();
    let (record, _) =
        block.buildings.iter().enumerate().find(|(_, building)| building.data.building_type == 15).unwrap();
    let scene = interior_layout(&block, record, &mut library).scene;
    let lamps = scene
        .objects
        .iter()
        .filter(|object| matches!(object.kind, SceneObjectKind::Flat { texture, .. } if texture.archive == LIGHTS_ARCHIVE))
        .count();
    let lights = scene_lights(&scene, GameLighting::Interior, &mut library);
    println!(
        "the tavern's {lamps} lamps: {:?}",
        lights.iter().map(|light| (light.range, light.intensity)).collect::<Vec<_>>()
    );
    assert!(lamps > 0 && lights.len() == lamps);
    assert!(scene_lights(&scene, GameLighting::Off, &mut library).is_empty());

    // A dungeon cell's lights, each reaching three times its radius.
    let cell = library.rdb_block("S0000160.RDB").unwrap();
    let scene = Scene::from_rdb_block(&cell);
    let radii: Vec<f32> = scene
        .objects
        .iter()
        .filter_map(|object| match object.kind {
            SceneObjectKind::Light { radius, .. } => Some(radius),
            _ => None,
        })
        .collect();
    let lights = scene_lights(&scene, GameLighting::Dungeon, &mut library);
    assert!(!radii.is_empty() && lights.len() == radii.len());
    assert!(lights.iter().zip(&radii).all(|(light, radius)| (light.range - radius * 3.0).abs() < 1e-4));

    // Every dungeon of the game has a treasure table.
    for dungeon in library.dungeons().iter() {
        if let Some(dungeon_type) = dungeon.dungeon_type {
            assert!(loot_table(dungeon_type).is_some(), "{}", dungeon.name);
        }
    }
    println!(
        "Daggerfall's dungeon: {}",
        loot_table(DungeonType::from_name("Human Stronghold").unwrap()).unwrap().describe()
    );
}

#[test]
fn plans_tests_on_empty_spots_and_where_things_are() {
    use daggerfall_dungeon_maker::arena2::{Climate, LocationType};
    use daggerfall_dungeon_maker::playtest::{self, places, Arrival, Setting};
    use daggerfall_dungeon_maker::project::CellCategory;
    use daggerfall_dungeon_maker::scene::Scene;

    let Some(mut library) = open_library() else { return };
    let world = library.world_map().expect("MAPS.BSA");
    let clear_of_locations = |pixel: daggerfall_dungeon_maker::arena2::MapPixel| {
        world.locations.iter().all(|location| {
            (location.pixel.x - pixel.x).abs() > places::CLEAR_RADIUS
                || (location.pixel.y - pixel.y).abs() > places::CLEAR_RADIUS
        })
    };

    // An outdoor cell on its own: a one-cell location in the climate asked for, on flat land.
    let woodlands = Climate::DEFAULT.value;
    let plan =
        playtest::outdoor_cell("CUSTAA05.RMB", woodlands, glam::Vec2::new(51.2, 55.2), &mut library).unwrap();
    assert!(plan.flat_land && clear_of_locations(plan.map_pixel), "{}", plan.map_pixel);
    assert_eq!(world.climate_at(plan.map_pixel), Some(woodlands));
    assert_eq!(plan.arrival, Arrival::Spot(glam::Vec2::new(51.2, 55.2)));
    let (name, location) = &plan.world_data[0];
    assert!(
        name.starts_with("locationnew-CUSTAA05-") && location.contains("\"HasDungeon\": false"),
        "{name}"
    );

    // A dungeon cell on its own without anywhere to put the player gets a start marker.
    let cell = library.rdb_block("N0000004.RDB").unwrap();
    let mut objects = Scene::from_rdb_block(&cell).objects;
    objects.retain(|object| !object.is_marker_record(8) && !object.is_marker_record(10));
    let plan = playtest::dungeon_cell(
        "N0000004.RDB",
        &objects,
        CellCategory::Interior,
        Some("N0000004.RDB"),
        &mut library,
    )
    .unwrap();
    assert_eq!(plan.arrival, Arrival::InsideDungeon);
    assert!(clear_of_locations(plan.map_pixel));
    assert!(plan.notes.iter().any(|note| note.contains("start marker")), "{:?}", plan.notes);
    assert!(plan.world_data.iter().any(|(name, _)| name == "N9999900.RDB.json"));

    // Daggerfall's own dungeons need no world data where they are, and a copy on their own.
    let dungeons = library.dungeons();
    let hold = dungeons.iter().find(|dungeon| dungeon.name == "Privateer's Hold").unwrap().clone();
    let plan = playtest::daggerfall_dungeon(&hold, Setting::WhereItIs, &mut library).unwrap();
    assert!(plan.world_data.is_empty() && !plan.flat_land && plan.arrival == Arrival::DungeonEntrance);
    let plan = playtest::daggerfall_dungeon(&hold, Setting::OnItsOwn, &mut library).unwrap();
    assert!(plan.world_data.len() == 1 && plan.flat_land && clear_of_locations(plan.map_pixel));

    // A settlement where it is arrives as travel does; on its own it keeps its kind.
    let locations = library.locations();
    let city = locations
        .iter()
        .filter(|location| location.name == "Daggerfall")
        .max_by_key(|location| location.exterior.block_names.len())
        .unwrap()
        .clone();
    let plan =
        playtest::settlement(&city, None, Setting::WhereItIs, woodlands, glam::Vec2::ZERO, &mut library)
            .unwrap();
    assert_eq!((plan.map_pixel, plan.arrival), (city.map_pixel, Arrival::Travel));
    let plan =
        playtest::settlement(&city, None, Setting::OnItsOwn, woodlands, glam::Vec2::ZERO, &mut library)
            .unwrap();
    assert_eq!(city.location_type, Some(LocationType::City));
    assert!(plan.world_data[0].1.contains("\"LocationType\": \"TownCity\""));
    println!("{}", plan.plan_file());
}

#[test]
fn tests_carry_imported_models_music_skies_and_scaled_objects() {
    use daggerfall_dungeon_maker::arena2::MapPixel;
    use daggerfall_dungeon_maker::custom_models::CustomModelLibrary;
    use daggerfall_dungeon_maker::music::{song_for_seed, DUNGEON_SONGS};
    use daggerfall_dungeon_maker::playtest::{self, Setting};
    use daggerfall_dungeon_maker::project::{
        CellCategory, DungeonLocation, Project, ProjectDungeonRoom, ProjectEntry, ProjectRoom,
    };
    use daggerfall_dungeon_maker::scene::{ModelData, Scene, SceneObjectKind};

    let Some(mut library) = open_library() else { return };
    let tags = library.model_tags();
    let folder = std::env::temp_dir().join(format!("ddm-test-content-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let obj = folder.join("pillar.obj");
    std::fs::write(&obj, "v 0 0 0\nv 1 0 0\nv 0 2 0\nf 1 2 3\n").unwrap();
    let (mut custom_models, _) = CustomModelLibrary::open(folder.join("store"));
    let model_id = custom_models.import(&obj).expect("the model imports").model.id;
    custom_models.register(&mut library);

    // A dungeon cell with an imported model and a scaled plain model: the
    // model stays in the cell and its files go into the test's content mod,
    // whose script puts the scaled model in place.
    let mut objects = Scene::from_rdb_block(&library.rdb_block("N0000004.RDB").unwrap()).objects;
    let scaled = (0..objects.len())
        .find(|&index| {
            matches!(objects[index].kind, SceneObjectKind::Model { .. })
                && Scene::scale_blocker(&objects, index, &tags).is_none()
        })
        .expect("a plain model");
    objects[scaled].scale = Some(glam::Vec3::splat(2.0));
    let mut pillar = objects[scaled].clone();
    pillar.kind = SceneObjectKind::Model { model_id, data: ModelData::default() };
    pillar.scale = None;
    pillar.id = objects.iter().map(|object| object.id).max().unwrap() + 1;
    pillar.next = None;
    objects.push(pillar);
    let plan = playtest::dungeon_cell(
        "N0000004.RDB",
        &objects,
        CellCategory::Interior,
        Some("N0000004.RDB"),
        &mut library,
    )
    .unwrap();
    let content = plan.content.as_ref().expect("a content mod");
    let copied: Vec<&str> = content.copied_files.iter().map(|file| file.path.as_str()).collect();
    assert!(copied.contains(&format!("Models/{model_id}_model.json").as_str()), "{copied:?}");
    let script = content.files.iter().find(|file| file.path.ends_with("ScaledObjects.cs.txt")).unwrap();
    assert!(script.contents.contains("\"Test of N0000004\"") && script.contents.contains("\"N9999900.RDB\""));
    let manifest = content.files.iter().find(|file| file.path == content.manifest_path()).unwrap();
    assert!(manifest.contents.contains(&format!("Models/{model_id}_model.json")));
    assert!(!manifest.contents.contains("WorldData"), "the world data stays loose");
    let (_, block) = plan.world_data.iter().find(|(name, _)| name == "N9999900.RDB.json").unwrap();
    assert!(block.contains(&format!("\"{model_id}\"")), "the imported model is in the cell");
    assert!(plan.notes.is_empty(), "{:?}", plan.notes);

    // A project dungeon on its own with its own music and sky and a chosen
    // song: the content mod plays and shows them there, and the song plays.
    let music = folder.join("theme.ogg");
    std::fs::write(&music, b"OggS").unwrap();
    let sky = folder.join("sky.png");
    std::fs::write(&sky, b"\x89PNG").unwrap();
    let dungeons = library.dungeons();
    let original = dungeons.iter().find(|dungeon| dungeon.name == "Privateer's Hold").unwrap();
    let start = original.blocks.iter().find(|block| block.is_starting_block).unwrap();
    let mut project = Project::default();
    let room_id = project.new_id();
    project.add(
        ProjectEntry::Room(ProjectRoom {
            id: room_id,
            name: "Start".to_string(),
            based_on: Some(start.block_name.clone()),
            category: CellCategory::Interior,
            objects: Scene::from_rdb_block(&library.rdb_block(&start.block_name).unwrap()).objects,
        }),
        None,
    );
    let dungeon_id = project.add_empty_dungeon("Hold of Tests");
    let song = DUNGEON_SONGS[3];
    let dungeon = project.dungeon_mut(dungeon_id).unwrap();
    dungeon.dungeon_type = original.dungeon_type;
    dungeon.rooms = ProjectDungeonRoom::layout_of(original);
    for room in &mut dungeon.rooms {
        if room.is_starting_block {
            room.room = Some(room_id);
        }
    }
    dungeon.location = DungeonLocation {
        music: Some(song),
        custom_music: true,
        custom_music_file: music.display().to_string(),
        skybox: true,
        skybox_file: sky.display().to_string(),
        ..DungeonLocation::copied_from(original)
    };
    let plan = playtest::project_dungeon(&project, dungeon_id, Setting::OnItsOwn, &mut library).unwrap();
    let content = plan.content.as_ref().expect("a content mod");
    let paths: Vec<&str> = content.files.iter().map(|file| file.path.as_str()).collect();
    let copied: Vec<&str> = content.copied_files.iter().map(|file| file.path.as_str()).collect();
    assert!(paths.iter().any(|path| path.ends_with("DungeonMusic.cs.txt")), "{paths:?}");
    assert!(paths.iter().any(|path| path.ends_with("DungeonSky.cs.txt")), "{paths:?}");
    assert!(copied.contains(&"Music/dungeon_maker_test_content_music_0.ogg"), "{copied:?}");
    assert!(copied.contains(&"Sky/dungeon_maker_test_content_sky_0.png"), "{copied:?}");
    assert!(content
        .files
        .iter()
        .all(|file| !file.path.ends_with(".cs.txt") || file.contents.contains("\"Hold of Tests\"")));
    let (_, location) = plan.world_data.iter().find(|(name, _)| name.starts_with("locationnew-")).unwrap();
    let location: serde_json::Value = serde_json::from_str(location).unwrap();
    let seed = location["Dungeon"]["RecordElement"]["Header"]["Unknown2"].as_u64().unwrap() as u32;
    let region = location["RegionIndex"].as_u64().unwrap() as usize;
    assert_eq!(song_for_seed(seed, region), song);

    // Without its music file, the test goes without it and says so.
    std::fs::remove_file(&music).unwrap();
    let plan = playtest::project_dungeon(&project, dungeon_id, Setting::OnItsOwn, &mut library).unwrap();
    assert!(plan.notes.iter().any(|note| note.contains("was not found")), "{:?}", plan.notes);
    let content = plan.content.as_ref().expect("the sky still needs a content mod");
    assert!(!content.files.iter().any(|file| file.path.ends_with("DungeonMusic.cs.txt")));
    project.dungeon_mut(dungeon_id).unwrap().location.custom_music = false;

    // Where it is, the export's scripts and files come too, without its world data.
    let world = library.world_map().unwrap();
    let home = original.map_pixel.unwrap();
    let pixel = (-20..=20)
        .flat_map(|dx| (-20..=20).map(move |dy| MapPixel::new(home.x + dx, home.y + dy)))
        .find(|&pixel| {
            world.region_at(pixel) == Some(original.region_index)
                && world.locations_at(pixel).next().is_none()
        })
        .expect("a free spot nearby");
    project.dungeon_mut(dungeon_id).unwrap().location.map_pixel = Some(pixel);
    let plan = playtest::project_dungeon(&project, dungeon_id, Setting::WhereItIs, &mut library).unwrap();
    let content = plan.content.as_ref().expect("a content mod");
    assert!(content.files.iter().any(|file| file.path.ends_with("DungeonSky.cs.txt")));
    assert!(content.files.iter().all(|file| !file.path.starts_with("WorldData/")));
    assert!(plan.world_data.iter().any(|(name, _)| name.starts_with("locationnew-Hold_of_Tests-")));
    std::fs::remove_dir_all(&folder).unwrap();
}
