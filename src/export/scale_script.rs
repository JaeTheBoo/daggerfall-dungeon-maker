//! The C# script a mod carries to show scaled objects at their size.
//!
//! Daggerfall's cells have no room for an object's size: every model and
//! flat is laid out at the size it was made. Worse, Daggerfall Unity joins a
//! cell's plain models into one mesh as it lays the cell out, so they cannot
//! be found and resized afterwards. So an export for a `.dfmod` leaves scaled
//! objects out of their cells, and the script made here puts them back, at
//! their size, once the dungeon is laid out: each one in its cell (found by
//! its block name and grid spot), where it was, turned as it was, with the
//! dungeon's textures for models as the game gives its own. Loose files have
//! no script, so there the objects stay in their cells at their usual size.
//!
//! Only objects without behaviour of their own can be scaled this way (see
//! [`crate::scene::SceneObject::scale_blocker`]): a copy made by the script is
//! plain, without the actions, door handling or loot the game would give it.
//! A dungeon is recognised by its region and name.

use glam::{Quat, Vec3};

use super::music_script::{identifier, string_literal, CURRENT_DUNGEON_METHOD};
use crate::scene::TAPESTRY_MODEL_IDS;

/// What a scaled object shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaledShape {
    Model(u32),
    Flat { archive: u16, record: u8 },
}

/// One scaled object the script puts in place.
#[derive(Debug, Clone, PartialEq)]
pub struct ScaledObject {
    /// Index of its dungeon in the script's list.
    pub dungeon: usize,
    /// The block (cell) it is in, as the dungeon names it, for example `N9000001.RDB`.
    pub block_name: String,
    /// The cell's spot in the dungeon's grid.
    pub grid_x: i32,
    pub grid_z: i32,
    pub shape: ScaledShape,
    /// Where it is in its cell, how it is turned, and its size along its own axes.
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

/// One dungeon with scaled objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaledDungeon {
    pub region_index: usize,
    /// The location's name, as the game knows it.
    pub location_name: String,
}

/// The script for the mod `mod_file_name`, putting `objects` in `dungeons`.
pub fn scale_script(mod_file_name: &str, dungeons: &[ScaledDungeon], objects: &[ScaledObject]) -> String {
    let class_name = format!("{}ScaledObjects", identifier(mod_file_name));
    let list = |items: Vec<String>| items.join(", ");
    let float = |value: f32| format!("{value}f");
    let regions = list(dungeons.iter().map(|dungeon| dungeon.region_index.to_string()).collect());
    let names = list(dungeons.iter().map(|dungeon| string_literal(&dungeon.location_name)).collect());
    let object_dungeons = list(objects.iter().map(|object| object.dungeon.to_string()).collect());
    let blocks = list(objects.iter().map(|object| string_literal(&object.block_name)).collect());
    let grid_x = list(objects.iter().map(|object| object.grid_x.to_string()).collect());
    let grid_z = list(objects.iter().map(|object| object.grid_z.to_string()).collect());
    let shape = |pick: fn(ScaledShape) -> i64| {
        list(objects.iter().map(|object| pick(object.shape).to_string()).collect())
    };
    let models = shape(|shape| match shape {
        ScaledShape::Model(model_id) => i64::from(model_id),
        ScaledShape::Flat { .. } => -1,
    });
    let archives = shape(|shape| match shape {
        ScaledShape::Model(_) => -1,
        ScaledShape::Flat { archive, .. } => i64::from(archive),
    });
    let records = shape(|shape| match shape {
        ScaledShape::Model(_) => -1,
        ScaledShape::Flat { record, .. } => i64::from(record),
    });
    let positions = list(objects.iter().flat_map(|object| object.position.to_array()).map(float).collect());
    let rotations = list(objects.iter().flat_map(|object| object.rotation.to_array()).map(float).collect());
    let scales = list(objects.iter().flat_map(|object| object.scale.to_array()).map(float).collect());
    let current_dungeon = CURRENT_DUNGEON_METHOD;
    let (first_tapestry, last_tapestry) = (TAPESTRY_MODEL_IDS.start(), TAPESTRY_MODEL_IDS.end());
    format!(
        r#"// Written by Daggerfall Dungeon Maker for the mod "{mod_file_name}".
// Puts the mod's dungeons' scaled objects in place, at their size. Cells
// cannot store an object's size, so the export left these objects out of
// their cells; this script adds them once the dungeon is laid out.
using System;
using UnityEngine;
using DaggerfallWorkshop;
using DaggerfallWorkshop.Game;
using DaggerfallWorkshop.Utility;
using DaggerfallWorkshop.Game.Utility.ModSupport;

public class {class_name} : MonoBehaviour
{{
    // The dungeons, by region and name.
    static readonly int[] Regions = {{ {regions} }};
    static readonly string[] LocationNames = {{ {names} }};

    // Each scaled object: its dungeon (an index into the lists above), and the
    // block it is in, by name and grid spot.
    static readonly int[] ObjectDungeons = {{ {object_dungeons} }};
    static readonly string[] ObjectBlocks = {{ {blocks} }};
    static readonly int[] ObjectGridX = {{ {grid_x} }};
    static readonly int[] ObjectGridZ = {{ {grid_z} }};
    // What it shows: a model id, or -1 for a flat, with the flat's texture archive and record.
    static readonly int[] ObjectModels = {{ {models} }};
    static readonly int[] ObjectArchives = {{ {archives} }};
    static readonly int[] ObjectRecords = {{ {records} }};
    // Where it is in its block (x, y, z), how it is turned (x, y, z, w) and its size (x, y, z).
    static readonly float[] Positions = {{ {positions} }};
    static readonly float[] Rotations = {{ {rotations} }};
    static readonly float[] Scales = {{ {scales} }};

    // Tapestries and banners, which the game gives no collision so the player
    // cannot get stuck behind them.
    const int FirstTapestry = {first_tapestry};
    const int LastTapestry = {last_tapestry};

    static Mod mod;
    // The laid out dungeon the objects were put in, so each is put in once.
    DaggerfallDungeon placedIn;

    [Invoke(StateManager.StateTypes.Start, 0)]
    public static void Init(InitParams initParams)
    {{
        mod = initParams.Mod;
        GameObject scaler = new GameObject(mod.Title + " scaled objects");
        DontDestroyOnLoad(scaler);
        scaler.AddComponent<{class_name}>();
        mod.IsReady = true;
        Debug.LogFormat("{{0}}: scaled objects script started for {{1}} object(s)", mod.Title, ObjectDungeons.Length);
    }}

    void LateUpdate()
    {{
        int dungeon = CurrentDungeon();
        if (dungeon < 0)
        {{
            placedIn = null;
            return;
        }}
        // A new layout (entering, or loading a save) needs the objects again;
        // the old one took its copies with it.
        DaggerfallDungeon laidOut = GameManager.Instance.PlayerEnterExit.Dungeon;
        if (laidOut == null || laidOut == placedIn)
            return;
        placedIn = laidOut;
        Place(dungeon, laidOut);
    }}

{current_dungeon}
    void Place(int dungeon, DaggerfallDungeon laidOut)
    {{
        int placed = 0;
        int missing = 0;
        for (int i = 0; i < ObjectDungeons.Length; i++)
        {{
            if (ObjectDungeons[i] != dungeon)
                continue;
            Transform block = FindBlock(laidOut, ObjectBlocks[i], ObjectGridX[i], ObjectGridZ[i]);
            if (block == null)
            {{
                missing++;
                continue;
            }}
            GameObject made;
            if (ObjectModels[i] >= 0)
            {{
                bool tapestry = ObjectModels[i] >= FirstTapestry && ObjectModels[i] <= LastTapestry;
                made = GameObjectHelper.CreateDaggerfallMeshGameObject((uint)ObjectModels[i], block, false, null, tapestry);
                DaggerfallMesh mesh = made.GetComponent<DaggerfallMesh>();
                if (mesh != null)
                    mesh.SetDungeonTextures(laidOut.DungeonTextureTable);
            }}
            else
            {{
                made = GameObjectHelper.CreateDaggerfallBillboardGameObject(ObjectArchives[i], ObjectRecords[i], block);
            }}
            if (made == null)
            {{
                missing++;
                continue;
            }}
            made.transform.localPosition = new Vector3(Positions[3 * i], Positions[3 * i + 1], Positions[3 * i + 2]);
            made.transform.localRotation = new Quaternion(Rotations[4 * i], Rotations[4 * i + 1], Rotations[4 * i + 2], Rotations[4 * i + 3]);
            made.transform.localScale = new Vector3(Scales[3 * i], Scales[3 * i + 1], Scales[3 * i + 2]);
            placed++;
        }}
        Debug.LogFormat("{{0}}: put {{1}} scaled object(s) in {{2}}", mod.Title, placed, LocationNames[dungeon]);
        if (missing > 0)
            Debug.LogWarningFormat("{{0}}: {{1}} scaled object(s) in {{2}} could not be put in place: their cell was not found", mod.Title, missing, LocationNames[dungeon]);
    }}

    // The laid out block named `blockName` at grid spot (x, z) of the dungeon.
    // The game names each block's object after the block, and puts it at its
    // grid spot times the size of a block.
    static Transform FindBlock(DaggerfallDungeon laidOut, string blockName, int x, int z)
    {{
        string name = "DaggerfallBlock [" + blockName + "]";
        Vector3 spot = new Vector3(x * RDBLayout.RDBSide, 0, z * RDBLayout.RDBSide);
        Transform found = null;
        float nearest = float.MaxValue;
        foreach (Transform child in laidOut.transform)
        {{
            if (!string.Equals(child.name, name, StringComparison.OrdinalIgnoreCase))
                continue;
            float distance = (child.localPosition - spot).sqrMagnitude;
            if (distance < nearest)
            {{
                found = child;
                nearest = distance;
            }}
        }}
        return found;
    }}
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_lists_each_object() {
        let dungeons = [ScaledDungeon { region_index: 17, location_name: "Big \"Cave\"".to_string() }];
        let objects = [
            ScaledObject {
                dungeon: 0,
                block_name: "N9000001.RDB".to_string(),
                grid_x: -1,
                grid_z: 2,
                shape: ScaledShape::Model(1234),
                position: Vec3::new(1.5, -2.0, 3.25),
                rotation: Quat::IDENTITY,
                scale: Vec3::new(2.0, 2.0, 0.5),
            },
            ScaledObject {
                dungeon: 0,
                block_name: "N9000002.RDB".to_string(),
                grid_x: 0,
                grid_z: 0,
                shape: ScaledShape::Flat { archive: 210, record: 3 },
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::new(3.0, 1.5, 1.0),
            },
        ];
        let script = scale_script("My Mod", &dungeons, &objects);
        assert!(script.contains("public class My_ModScaledObjects : MonoBehaviour"));
        assert!(script.contains(r#"LocationNames = { "Big \"Cave\"" }"#));
        assert!(script.contains(r#"ObjectBlocks = { "N9000001.RDB", "N9000002.RDB" }"#));
        assert!(script.contains("ObjectGridX = { -1, 0 }"));
        assert!(script.contains("ObjectModels = { 1234, -1 }"));
        assert!(script.contains("ObjectArchives = { -1, 210 }"));
        assert!(script.contains("ObjectRecords = { -1, 3 }"));
        assert!(script.contains("Positions = { 1.5f, -2f, 3.25f, 0f, 0f, 0f }"));
        assert!(script.contains("Rotations = { 0f, 0f, 0f, 1f, 0f, 0f, 0f, 1f }"));
        assert!(script.contains("Scales = { 2f, 2f, 0.5f, 3f, 1.5f, 1f }"));
        assert!(script.contains("SetDungeonTextures(laidOut.DungeonTextureTable)"));
        assert!(script.contains("const int FirstTapestry = 42500;"));
        assert!(script.contains("const int LastTapestry = 42571;"));
    }
}
