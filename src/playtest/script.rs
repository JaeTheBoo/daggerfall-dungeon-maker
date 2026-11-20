//! The program's own small mod for the test copy of Daggerfall Unity, which
//! starts the game straight into what is tested.
//!
//! Started with `-dungeonMakerTest <plan file>` (see [`super::TestPlan`]),
//! its script waits for the title screen, then begins a new game with
//! Daggerfall Unity's ready-made character at the plan's map pixel: inside
//! the dungeon there, or outside, moved as the plan says (to the dungeon's
//! entrance, to where travel arrives, or to a spot of the location, on the
//! ground found below it once the location is built). The new character's
//! quests (the tutorial, the main quest's first letter) are dropped, the
//! date moves to the plan's season (with clear weather) if it names one, and
//! no-clip is turned on as the console's `tcl` does, flying three times as
//! fast as the game's own levitating (the mouse wheel changes the speed, and
//! holding Run flies faster still), with mouse look on. A plan for
//! something on its own also makes the land flat and bare: the script gives
//! the game a terrain sampler of its own, with every height the same, and
//! leaves out the trees and rocks that are scattered over the land.
//!
//! Without the argument the script does nothing.
//!
//! A mod's scripts run only from a `.dfmod`, which only Unity can build, so
//! it is built once with the Unity editor an export uses (see
//! [`crate::export::unity`]) and kept for later tests ([`built_mod`]).

use std::path::{Path, PathBuf};

use crate::export::json::Manifest;
use crate::export::unity::{self, BuildRequest, BuildTarget};
use crate::export::{MANIFEST_EXTENSION, MOD_ASSET_ROOT};

/// The mod's name, which names its folder, manifest, script and `.dfmod`.
pub const MOD_NAME: &str = "DungeonMakerTest";

/// The command line argument naming the plan file.
pub const PLAN_ARGUMENT: &str = "-dungeonMakerTest";

/// The script.
pub const SCRIPT: &str = r##"// Written by Daggerfall Dungeon Maker for its test copy of Daggerfall Unity.
// Started with "-dungeonMakerTest <plan file>", it begins a new game with the
// ready-made character where the plan says, with no-clip on. Without that
// argument it does nothing.
using System;
using System.Collections.Generic;
using System.IO;
using Unity.Collections;
using Unity.Jobs;
using UnityEngine;
using DaggerfallConnect;
using DaggerfallConnect.Arena2;
using DaggerfallConnect.Utility;
using DaggerfallWorkshop;
using DaggerfallWorkshop.Game;
using DaggerfallWorkshop.Game.Entity;
using DaggerfallWorkshop.Game.Questing;
using DaggerfallWorkshop.Game.UserInterface;
using DaggerfallWorkshop.Game.UserInterfaceWindows;
using DaggerfallWorkshop.Game.Utility;
using DaggerfallWorkshop.Game.Utility.ModSupport;
using DaggerfallWorkshop.Game.Weather;
using DaggerfallWorkshop.Utility;

public class DungeonMakerTest : MonoBehaviour
{
    const string PlanArgument = "-dungeonMakerTest";
    // How long to wait for the title screen before starting anyway, in seconds.
    const float TitleWait = 30f;

    // How long after the game starts the player's place is written to the log, in seconds.
    const float ReportDelay = 8f;
    // How many frames the location must have been there before the player
    // is put at the plan's spot, so the game has finished putting them anywhere.
    const int SpotFrames = 5;

    // Flying speed with no-clip at first, and its limits, in metres a second
    // (the game's own levitating speed is 4).
    const float StartFlySpeed = 12f;
    const float SlowestFlySpeed = 2f;
    const float FastestFlySpeed = 150f;
    // How much one notch of the mouse wheel changes the flying speed, and how
    // many times faster holding Run flies.
    const float WheelStep = 1.25f;
    const float RunFactor = 3f;
    // How long after the wheel stops turning the new speed is shown, in seconds.
    const float SpeedShownAfter = 0.3f;

    static Dictionary<string, string> plan;
    float waitStarted;
    bool started;
    // When the game started (unscaled time), or -1 before it does; and whether the place was logged.
    float gameStarted = -1f;
    bool reported;
    // Whether the player is still to be put at the plan's spot, and for how
    // many frames the location has been there.
    bool spotPending;
    int framesWithLocation;
    // The flying speed chosen with the mouse wheel; the game's own levitating
    // speed, kept while no-clip is on to give back when it is turned off (or
    // -1 while it is off); and when the wheel last turned (-1 once its speed is shown).
    float flySpeed = StartFlySpeed;
    float usualLevitateSpeed = -1f;
    float wheelTurned = -1f;

    [Invoke(StateManager.StateTypes.Start, 0)]
    public static void Init(InitParams initParams)
    {
        plan = ReadPlan();
        if (plan != null)
        {
            if (Value("flat") == "true")
            {
                DaggerfallUnity.Instance.TerrainSampler = new DungeonMakerFlatLand(DaggerfallUnity.Instance.TerrainSampler);
                DaggerfallUnity.Instance.TerrainNature = new DungeonMakerNoNature();
            }
            GameObject test = new GameObject("Dungeon Maker test");
            DontDestroyOnLoad(test);
            test.AddComponent<DungeonMakerTest>();
        }
        initParams.Mod.IsReady = true;
    }

    // The plan's "name=value" lines, or null without the argument.
    static Dictionary<string, string> ReadPlan()
    {
        string[] arguments = Environment.GetCommandLineArgs();
        for (int i = 0; i + 1 < arguments.Length; i++)
        {
            if (arguments[i] != PlanArgument)
                continue;
            Dictionary<string, string> values = new Dictionary<string, string>();
            try
            {
                foreach (string line in File.ReadAllLines(arguments[i + 1]))
                {
                    int equals = line.IndexOf('=');
                    if (equals > 0 && !line.StartsWith("#"))
                        values[line.Substring(0, equals).Trim()] = line.Substring(equals + 1).Trim();
                }
            }
            catch (Exception exception)
            {
                Debug.LogErrorFormat("Dungeon Maker test: could not read the plan {0}: {1}", arguments[i + 1], exception.Message);
                return null;
            }
            return values;
        }
        return null;
    }

    static string Value(string name)
    {
        string value;
        return plan.TryGetValue(name, out value) ? value : "";
    }

    static int Int(string name)
    {
        int value;
        int.TryParse(Value(name), System.Globalization.NumberStyles.Integer, System.Globalization.CultureInfo.InvariantCulture, out value);
        return value;
    }

    static float Float(string name)
    {
        float value;
        float.TryParse(Value(name), System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out value);
        return value;
    }

    void Start()
    {
        waitStarted = Time.realtimeSinceStartup;
    }

    void Update()
    {
        if (started)
        {
            if (gameStarted >= 0f)
                Fly();
            if (spotPending)
                PlaceAtSpot();
            else if (!reported && gameStarted >= 0f && Time.realtimeSinceStartup - gameStarted > ReportDelay)
                ReportPlace();
            return;
        }
        // Starting once the title screen (or its video) is over the HUD closes
        // it; starting sooner would leave it to open over the game.
        bool titleShown = DaggerfallUI.UIManager != null && DaggerfallUI.UIManager.TopWindow != null
            && !(DaggerfallUI.UIManager.TopWindow is DaggerfallHUD);
        if (!titleShown && Time.realtimeSinceStartup - waitStarted < TitleWait)
            return;
        started = true;
        DaggerfallUnity.Settings.StartCellX = Int("map_x");
        DaggerfallUnity.Settings.StartCellY = Int("map_y");
        DaggerfallUnity.Settings.StartInDungeon = Value("arrival") == "inside";
        StartGameBehaviour.OnStartGame += OnStartGame;
        GameManager.Instance.StartGameBehaviour.StartMethod = StartGameBehaviour.StartMethods.NewCharacter;
    }

    // Called once the new game has begun, before the world around the player is built.
    void OnStartGame(object sender, EventArgs e)
    {
        StartGameBehaviour.OnStartGame -= OnStartGame;
        gameStarted = Time.realtimeSinceStartup;
        QuestMachine.Instance.PurgeAllQuests();
        SetSeason(Value("season"));
        StreamingWorld world = GameManager.Instance.StreamingWorld;
        string arrival = Value("arrival");
        if (arrival == "entrance")
        {
            world.SetAutoReposition(StreamingWorld.RepositionMethods.DungeonEntrance, Vector3.zero);
        }
        else if (arrival == "travel")
        {
            world.SetAutoReposition(StreamingWorld.RepositionMethods.RandomStartMarker, Vector3.zero);
        }
        else if (arrival == "spot")
        {
            // Done once the location is built (see PlaceAtSpot).
            spotPending = true;
        }
        TurnOnNoClip();
        TurnOnMouseLook();
        DaggerfallUI.AddHUDText("Dungeon Maker test: no-clip is on (the console's tcl turns it off)", 6f);
        DaggerfallUI.AddHUDText("The mouse wheel changes the flying speed; hold Run to fly faster", 6f);
    }

    // While no-clip is on, flies at the speed chosen with the mouse wheel,
    // and faster while Run is held; turning no-clip off gives back the game's own speed.
    void Fly()
    {
        LevitateMotor levitate = GameManager.Instance.PlayerMotor.GetComponent<LevitateMotor>();
        if (levitate == null || GameManager.IsGamePaused)
            return;
        if (!GameManager.Instance.PlayerEntity.NoClipMode)
        {
            if (usualLevitateSpeed >= 0f)
            {
                levitate.LevitateMoveSpeed = usualLevitateSpeed;
                usualLevitateSpeed = -1f;
            }
            return;
        }
        if (usualLevitateSpeed < 0f)
            usualLevitateSpeed = levitate.LevitateMoveSpeed;
        float wheel = Input.mouseScrollDelta.y;
        if (wheel != 0f)
        {
            flySpeed = Mathf.Clamp(flySpeed * Mathf.Pow(WheelStep, wheel), SlowestFlySpeed, FastestFlySpeed);
            wheelTurned = Time.realtimeSinceStartup;
        }
        else if (wheelTurned >= 0f && Time.realtimeSinceStartup - wheelTurned > SpeedShownAfter)
        {
            wheelTurned = -1f;
            DaggerfallUI.AddHUDText(string.Format("Flying speed {0:0} (the game's own is 4)", flySpeed), 1.5f);
        }
        levitate.LevitateMoveSpeed = InputManager.Instance.HasAction(InputManager.Actions.Run) ? flySpeed * RunFactor : flySpeed;
    }

    // Puts the player at the plan's spot of the location, on the ground
    // there, facing north, once the location has been built for a few frames.
    void PlaceAtSpot()
    {
        DaggerfallLocation location = GameManager.Instance.StreamingWorld.CurrentPlayerLocationObject;
        if (location == null || GameManager.Instance.StreamingWorld.IsRepositioningPlayer)
        {
            framesWithLocation = 0;
            return;
        }
        if (++framesWithLocation < SpotFrames)
            return;
        spotPending = false;
        Vector3 spot = location.transform.position + LocationOrigin() + new Vector3(Float("spot_x"), 0, Float("spot_z"));
        // The spot has only sky above it, so the first thing below is the ground.
        RaycastHit hit;
        Vector3 ground = spot;
        if (Physics.Raycast(spot + Vector3.up * 500f, Vector3.down, out hit, 1000f, ~0, QueryTriggerInteraction.Ignore))
            ground = hit.point;
        CharacterController controller = GameManager.Instance.PlayerController;
        GameManager.Instance.PlayerObject.transform.position = ground + Vector3.up * (controller.height * 0.65f);
        GameManager.Instance.AcrobatMotor.ClearFallingDamage();
        if (GameManager.Instance.PlayerMouseLook != null)
            GameManager.Instance.PlayerMouseLook.SetFacing(Vector3.forward);
    }

    // Writes where the test put the player, and whether no-clip is on, to the log, for checking.
    void ReportPlace()
    {
        reported = true;
        Vector3 player = GameManager.Instance.PlayerObject.transform.position;
        DaggerfallLocation location = GameManager.Instance.StreamingWorld.CurrentPlayerLocationObject;
        string place = GameManager.Instance.PlayerEnterExit.IsPlayerInsideDungeon ? "inside the dungeon" : "outside";
        if (location != null && !GameManager.Instance.PlayerEnterExit.IsPlayerInsideDungeon)
            place = string.Format("{0} from the location's corner", player - location.transform.position - LocationOrigin());
        LevitateMotor levitate = GameManager.Instance.PlayerMotor.GetComponent<LevitateMotor>();
        Debug.LogFormat("Dungeon Maker test: the player is at {0}, {1}; no-clip {2}, levitating {3}, layer {4}",
            player, place, GameManager.Instance.PlayerEntity.NoClipMode, levitate != null && levitate.IsLevitating,
            LayerMask.LayerToName(GameManager.Instance.PlayerController.gameObject.layer));
        PlayerMouseLook look = GameManager.Instance.PlayerMouseLook;
        System.Reflection.FieldInfo forced = typeof(PlayerMouseLook).GetField("forceHideCursor",
            System.Reflection.BindingFlags.Instance | System.Reflection.BindingFlags.NonPublic);
        bool stopped = look == null || (forced != null && (bool)forced.GetValue(look));
        IUserInterfaceWindow top = DaggerfallUI.UIManager.TopWindow;
        Debug.LogFormat("Dungeon Maker test: mouse look {0}, cursor shown {1}, cursor {2}, game paused {3}, focused {4}, top window {5}",
            !stopped && look.enableMouseLook && look.lockCursor && !look.cursorActive, look != null && look.cursorActive,
            Cursor.lockState, GameManager.IsGamePaused, Application.isFocused, top != null ? top.GetType().Name : "none");
    }

    // Mouse look on, with the cursor hidden, as a game usually starts. The
    // intro video stops mouse look while it plays and only lets go of it if it
    // ends by itself, which it does not when the test starts over it.
    static void TurnOnMouseLook()
    {
        PlayerMouseLook look = GameManager.Instance.PlayerMouseLook;
        if (look == null)
            return;
        look.ForceHideCursor(false);
        look.cursorActive = false;
        look.lockCursor = true;
        look.enableMouseLook = true;
    }

    // Moves the date to the middle of summer or of winter ("summer" or
    // "winter"), with clear weather; anything else keeps the game's start date.
    static void SetSeason(string season)
    {
        if (season != "summer" && season != "winter")
            return;
        DaggerfallDateTime now = DaggerfallUnity.Instance.WorldTime.Now;
        // Months 5 to 7 are summer, 11 to 1 winter.
        now.Month = season == "summer" ? 6 : 0;
        // The time the player was last updated moves too, so no time seems to pass.
        GameManager.Instance.PlayerEntity.LastGameMinutes = now.ToClassicDaggerfallTime();
        // Clear weather in every climate, which the game takes the weather from on arriving.
        WeatherManager weather = GameManager.Instance.WeatherManager;
        for (int i = 0; i < weather.PlayerWeather.ClimateWeathers.Length; i++)
            weather.PlayerWeather.ClimateWeathers[i] = (byte)WeatherType.Sunny;
        weather.SetWeather(WeatherType.Sunny);
    }

    // Where the location at the plan's map pixel starts (its south-west
    // corner), in the map pixel's own space, as the game lays it out.
    static Vector3 LocationOrigin()
    {
        ContentReader reader = DaggerfallUnity.Instance.ContentReader;
        ContentReader.MapSummary summary;
        DFLocation location;
        if (!reader.HasLocation(Int("map_x"), Int("map_y"), out summary)
            || !reader.GetLocation(summary.RegionIndex, summary.MapIndex, out location))
            return Vector3.zero;
        DFPosition tile = TerrainHelper.GetLocationTerrainTileOrigin(location);
        return new Vector3(tile.X * RMBLayout.RMBTileSide, 0, tile.Y * RMBLayout.RMBTileSide);
    }

    // As the console's "tcl": no collisions, and levitating.
    static void TurnOnNoClip()
    {
        PlayerEntity player = GameManager.Instance.PlayerEntity;
        if (player == null)
            return;
        player.NoClipMode = true;
        LevitateMotor levitate = GameManager.Instance.PlayerMotor.GetComponent<LevitateMotor>();
        if (levitate != null)
            levitate.IsLevitating = true;
        GameManager.Instance.PlayerController.gameObject.layer = LayerMask.NameToLayer("NoclipLayer");
    }
}

// Land at one height everywhere, a little above the beach so it is grass and earth.
public class DungeonMakerFlatLand : TerrainSampler
{
    readonly float height;

    public DungeonMakerFlatLand(ITerrainSampler usual)
    {
        HeightmapDimension = usual.HeightmapDimension;
        MaxTerrainHeight = usual.MaxTerrainHeight;
        MeanTerrainHeightScale = usual.MeanTerrainHeightScale;
        OceanElevation = usual.OceanElevation;
        BeachElevation = usual.BeachElevation;
        height = Mathf.Clamp01((usual.BeachElevation + 40f) / usual.MaxTerrainHeight);
    }

    public override int Version
    {
        get { return 1; }
    }

    public override void GenerateSamples(ref MapPixelData mapPixel)
    {
        float[,] samples = new float[HeightmapDimension, HeightmapDimension];
        for (int y = 0; y < HeightmapDimension; y++)
            for (int x = 0; x < HeightmapDimension; x++)
                samples[y, x] = height;
        mapPixel.heightmapSamples = samples;
    }

    public override JobHandle ScheduleGenerateSamplesJob(ref MapPixelData mapPixel)
    {
        NativeArray<float> samples = mapPixel.heightmapData;
        for (int i = 0; i < samples.Length; i++)
            samples[i] = height;
        return new JobHandle();
    }
}

// No trees or rocks scattered over the land.
public class DungeonMakerNoNature : ITerrainNature
{
    public bool NatureMeshUsed
    {
        get { return false; }
    }

    public void LayoutNature(DaggerfallTerrain dfTerrain, DaggerfallBillboardBatch dfBillboardBatch, float terrainScale, int terrainDist)
    {
    }
}
"##;

/// The mod's manifest, listing its files where Unity builds them.
fn manifest() -> String {
    let folder = format!("{MOD_ASSET_ROOT}/{MOD_NAME}");
    let manifest = Manifest {
        mod_title: "Dungeon Maker test".to_string(),
        mod_version: "1.0".to_string(),
        mod_author: "Daggerfall Dungeon Maker".to_string(),
        contact_info: String::new(),
        dfunity_version: crate::export::DAGGERFALL_UNITY_VERSION.to_string(),
        mod_description: "Starts tests of Daggerfall Dungeon Maker's cells, dungeons and settlements. \
                          It is only in the program's own copy of Daggerfall Unity."
            .to_string(),
        guid: "5d1f7a52-3c4e-4b8a-9e61-0d2c7f4a9b13".to_string(),
        files: vec![
            format!("{folder}/{MOD_NAME}.cs.txt"),
            format!("{folder}/{MOD_NAME}{MANIFEST_EXTENSION}"),
        ],
        dependencies: Vec::new(),
    };
    crate::export::to_json(&manifest)
}

/// File name of the built mod.
pub fn bundle_name() -> String {
    format!("{}.dfmod", MOD_NAME.to_lowercase())
}

/// The mod built for this computer's platform, building it with the Unity
/// editor `unity_editor` first if this version of it has not been built
/// yet. Builds are kept in `cache`, one folder per version of the script.
pub fn built_mod(
    cache: &Path,
    unity_editor: Option<&Path>,
    step: &dyn Fn(String),
) -> Result<PathBuf, String> {
    let manifest = manifest();
    let version = format!("{:016x}", stable_hash(&[SCRIPT, &manifest]));
    let mod_folder = cache.join(&version).join(MOD_NAME);
    let target = BuildTarget::current();
    let bundle = mod_folder.join(target.name()).join(bundle_name());
    if bundle.is_file() {
        return Ok(bundle);
    }
    let unity_editor = unity_editor.ok_or(
        "Testing needs the Unity editor once, to build the small mod that starts the test (Unity 2019.4, \
         as for building a .dfmod in File > Export). Install Unity 2019.4.40f1 with Unity Hub, or choose \
         it in the export window.",
    )?;
    std::fs::create_dir_all(&mod_folder)
        .map_err(|error| format!("Could not create '{}': {error}", mod_folder.display()))?;
    let write = |name: String, contents: &str| {
        let path = mod_folder.join(name);
        std::fs::write(&path, contents)
            .map_err(|error| format!("Could not write '{}': {error}", path.display()))
    };
    write(format!("{MOD_NAME}{MANIFEST_EXTENSION}"), &manifest)?;
    write(format!("{MOD_NAME}.cs.txt"), SCRIPT)?;
    step("Building the test mod with Unity (only the first time; this takes a minute or two)…".to_string());
    unity::build(
        &BuildRequest {
            unity_editor: unity_editor.to_path_buf(),
            build_project: unity::default_build_project(),
            mod_folder: mod_folder.clone(),
            file_name: MOD_NAME.to_string(),
            targets: vec![target],
            mods_folder: None,
        },
        step,
    )?;
    if !bundle.is_file() {
        return Err(format!("Unity finished but the test mod is not at '{}'", bundle.display()));
    }
    // Builds of earlier versions of the script are no longer used.
    if let Ok(entries) = std::fs::read_dir(cache) {
        for entry in entries.flatten() {
            if entry.file_name() != version.as_str() && entry.path().is_dir() {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    Ok(bundle)
}

/// A hash of `parts` that stays the same from one run (and build) of the
/// program to the next (FNV-1a).
pub(super) fn stable_hash(parts: &[&str]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.bytes().chain(std::iter::once(0)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_lists_the_script_and_itself() {
        let manifest = manifest();
        assert!(
            manifest.contains("Assets/DungeonMaker/DungeonMakerTest/DungeonMakerTest.cs.txt"),
            "{manifest}"
        );
        assert!(
            manifest.contains("Assets/DungeonMaker/DungeonMakerTest/DungeonMakerTest.dfmod.json"),
            "{manifest}"
        );
    }

    #[test]
    fn the_script_reads_the_argument_the_program_passes() {
        assert!(SCRIPT.contains(&format!("\"{PLAN_ARGUMENT}\"")));
    }

    #[test]
    fn versions_are_told_apart_by_content() {
        assert_eq!(stable_hash(&["a", "b"]), stable_hash(&["a", "b"]));
        assert_ne!(stable_hash(&["ab", ""]), stable_hash(&["a", "b"]));
    }
}
