//! The program's own copy of Daggerfall Unity, which tests run in.
//!
//! The copy is made from the user's install the first time, and again when
//! that install changes (a new version of Daggerfall Unity), in the user's
//! local application data. It leaves out the user's mods and loose world
//! data, and it is in Daggerfall Unity's portable mode (a `Portable.txt`
//! beside the program): its settings, key bindings, saved games and log
//! are kept in its own `PortableAppdata` folder, never in the user's.
//!
//! Before each test its settings are made from the user's own (so the
//! screen, sound and keys are as the user has them), pointed at the
//! Daggerfall files the editor uses, with Daggerfall Unity's options
//! window skipped, mods on, and saved games and screenshots kept in the
//! copy. Then its world data is replaced with the test's, and the test mod
//! is put in its `Mods` folder.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// Daggerfall Unity's data folder inside its install folder (Windows and Linux).
const DATA_FOLDER: &str = "DaggerfallUnity_Data";
/// The program, by platform.
const WINDOWS_PROGRAM: &str = "DaggerfallUnity.exe";
const LINUX_PROGRAM: &str = "DaggerfallUnity.x86_64";
/// The file that puts Daggerfall Unity in portable mode, and the folder it then keeps its data in.
const PORTABLE_FILE: &str = "Portable.txt";
const PORTABLE_DATA_FOLDER: &str = "PortableAppdata";
/// The file in the copy that records which install it was made from.
const STAMP_FILE: &str = "DungeonMakerCopy.txt";
/// The file in the copy holding the test's plan.
pub const PLAN_FILE: &str = "DungeonMakerTest.txt";
/// Daggerfall Unity's settings file and key bindings, in its data folder.
const SETTINGS_FILE: &str = "settings.ini";
const KEY_BINDS_FILE: &str = "KeyBinds.txt";

/// The copy's folder: next to the Unity project exports build with.
pub fn copy_folder() -> PathBuf {
    let builder = crate::export::unity::default_build_project();
    builder.parent().map_or_else(|| builder.clone(), Path::to_path_buf).join("TestGame")
}

/// Where tests keep the test mod they built (see [`super::script::built_mod`]).
pub fn mod_cache_folder() -> PathBuf {
    copy_folder().with_file_name("TestMod")
}

/// Where tests keep the content mods they built (see [`super::content`]).
pub fn content_cache_folder() -> PathBuf {
    copy_folder().with_file_name("TestContent")
}

/// The program of the Daggerfall Unity in `folder`, if there is one.
fn program_in(folder: &Path) -> Option<PathBuf> {
    [WINDOWS_PROGRAM, LINUX_PROGRAM].iter().map(|name| folder.join(name)).find(|path| path.is_file())
}

/// The copy of Daggerfall Unity tests run in.
#[derive(Debug, Clone)]
pub struct GameCopy {
    pub folder: PathBuf,
    program: PathBuf,
}

impl GameCopy {
    /// The copy made from the Daggerfall Unity installed in `install`,
    /// copying it first if there is no copy of this install yet.
    pub fn prepare(install: &Path, step: &dyn Fn(String)) -> Result<Self, String> {
        if cfg!(target_os = "macos") {
            return Err("Testing in Daggerfall Unity is not available on macOS yet".to_string());
        }
        let source_program = program_in(install).ok_or_else(|| {
            format!(
                "Daggerfall Unity's program is not in '{}' (File > Set Daggerfall Unity folder)",
                install.display()
            )
        })?;
        let folder = copy_folder();
        let stamp = install_stamp(install, &source_program);
        let current = std::fs::read_to_string(folder.join(STAMP_FILE)).ok();
        if current.as_deref() != Some(stamp.as_str()) || program_in(&folder).is_none() {
            step("Copying Daggerfall Unity for testing (only the first time)…".to_string());
            if folder.exists() {
                std::fs::remove_dir_all(&folder).map_err(|error| {
                    format!("Could not clear the old test copy '{}': {error}", folder.display())
                })?;
            }
            copy_install(install, &folder)?;
            write(
                &folder.join(PORTABLE_FILE),
                "Daggerfall Unity keeps its settings and saved games in PortableAppdata.\n",
            )?;
            write(&folder.join(STAMP_FILE), &stamp)?;
        }
        let program = program_in(&folder).ok_or("The test copy of Daggerfall Unity has no program")?;
        Ok(Self { folder, program })
    }

    fn streaming_assets(&self) -> PathBuf {
        self.folder.join(DATA_FOLDER).join("StreamingAssets")
    }

    /// Writes the copy's settings: the user's own (from the install in
    /// `install`), changed to use the Daggerfall files in `daggerfall_folder`
    /// (the folder holding `ARENA2`, if known) and to start straight away.
    pub fn write_settings(&self, install: &Path, daggerfall_folder: Option<&Path>) -> Result<(), String> {
        let data = self.folder.join(PORTABLE_DATA_FOLDER);
        create_folder(&data)?;
        let users_data = user_data_folder(install);
        let mut settings = users_data
            .as_ref()
            .and_then(|folder| std::fs::read_to_string(folder.join(SETTINGS_FILE)).ok())
            .unwrap_or_default();
        if let Some(folder) = daggerfall_folder {
            settings =
                set_ini_value(&settings, "Daggerfall", "MyDaggerfallPath", &folder.display().to_string());
        }
        // Saved games and screenshots stay in the copy, wherever the user keeps theirs.
        settings = set_ini_value(&settings, "Daggerfall", "MyDaggerfallUnitySavePath", "");
        settings = set_ini_value(&settings, "Daggerfall", "MyDaggerfallUnityScreenshotsPath", "");
        settings = set_ini_value(&settings, "GUI", "ShowOptionsAtStart", "False");
        settings = set_ini_value(&settings, "Enhancements", "LypyL_ModSystem", "True");
        write(&data.join(SETTINGS_FILE), &settings)?;
        if let Some(key_binds) =
            users_data.map(|folder| folder.join(KEY_BINDS_FILE)).filter(|path| path.is_file())
        {
            copy(&key_binds, &data.join(KEY_BINDS_FILE))?;
        }
        Ok(())
    }

    /// Makes the `.dfmod`s `bundles` (the test mod, and the test's content
    /// mod if it has one) the only mods in the copy's `Mods` folder.
    pub fn install_mods(&self, bundles: &[PathBuf]) -> Result<(), String> {
        let mods = self.streaming_assets().join("Mods");
        create_folder(&mods)?;
        if let Ok(entries) = std::fs::read_dir(&mods) {
            for entry in entries.flatten() {
                if entry.path().extension().is_some_and(|extension| extension.eq_ignore_ascii_case("dfmod")) {
                    std::fs::remove_file(entry.path())
                        .map_err(|error| format!("Could not remove '{}': {error}", entry.path().display()))?;
                }
            }
        }
        for bundle in bundles {
            let name = bundle.file_name().ok_or("A test mod has no file name")?;
            copy(bundle, &mods.join(name))?;
        }
        Ok(())
    }

    /// Replaces the copy's loose world data with `files` (names inside `WorldData`, and contents).
    pub fn write_world_data(&self, files: &[(String, String)]) -> Result<(), String> {
        let world_data = self.streaming_assets().join(crate::export::WORLD_DATA_FOLDER);
        if world_data.exists() {
            std::fs::remove_dir_all(&world_data)
                .map_err(|error| format!("Could not clear '{}': {error}", world_data.display()))?;
        }
        create_folder(&world_data)?;
        for (name, contents) in files {
            write(&world_data.join(name), contents)?;
        }
        Ok(())
    }

    /// Writes the plan `text` and starts the copy with it.
    pub fn launch(&self, plan: &str) -> Result<Child, String> {
        let plan_path = self.folder.join(PLAN_FILE);
        write(&plan_path, plan)?;
        Command::new(&self.program)
            .arg(super::script::PLAN_ARGUMENT)
            .arg(&plan_path)
            .current_dir(&self.folder)
            .spawn()
            .map_err(|error| format!("Could not start '{}': {error}", self.program.display()))
    }
}

/// What identifies the install in `install` (with its program `program`):
/// its folder, and the size and time of its program and game code, which
/// change with each version of Daggerfall Unity.
fn install_stamp(install: &Path, program: &Path) -> String {
    let describe = |path: &Path| match std::fs::metadata(path) {
        Ok(metadata) => {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |since| since.as_secs());
            format!("{} {}", metadata.len(), modified)
        }
        Err(_) => "missing".to_string(),
    };
    let code = install.join(DATA_FOLDER).join("Managed").join("Assembly-CSharp.dll");
    format!("{}\n{}\n{}\n", install.display(), describe(program), describe(&code))
}

/// Copies the install in `install` to `destination`, leaving out the
/// user's mods, loose world data and portable data.
fn copy_install(install: &Path, destination: &Path) -> Result<(), String> {
    let streaming_assets = install.join(DATA_FOLDER).join("StreamingAssets");
    let left_out = [
        streaming_assets.join("Mods"),
        streaming_assets.join(crate::export::WORLD_DATA_FOLDER),
        install.join(PORTABLE_DATA_FOLDER),
        install.join(PORTABLE_FILE),
    ];
    copy_folder_contents(install, destination, &left_out)
}

fn copy_folder_contents(source: &Path, destination: &Path, left_out: &[PathBuf]) -> Result<(), String> {
    create_folder(destination)?;
    let entries = std::fs::read_dir(source)
        .map_err(|error| format!("Could not read '{}': {error}", source.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("Could not read '{}': {error}", source.display()))?;
        let path = entry.path();
        if left_out.contains(&path) {
            continue;
        }
        let target = destination.join(entry.file_name());
        let kind =
            entry.file_type().map_err(|error| format!("Could not read '{}': {error}", path.display()))?;
        if kind.is_dir() {
            copy_folder_contents(&path, &target, left_out)?;
        } else {
            copy(&path, &target)?;
        }
    }
    Ok(())
}

/// The folder the Daggerfall Unity installed in `install` keeps the user's
/// settings in: its own portable data folder if it is in portable mode,
/// else Unity's usual place for it.
fn user_data_folder(install: &Path) -> Option<PathBuf> {
    if install.join(PORTABLE_FILE).is_file() {
        return Some(install.join(PORTABLE_DATA_FOLDER));
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)?;
    let company = Path::new("Daggerfall Workshop").join("Daggerfall Unity");
    Some(if cfg!(windows) {
        home.join("AppData").join("LocalLow").join(company)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("unity3d")
            .join(company)
    })
}

/// `ini` with `key` in `[section]` set to `value`: the line changed if
/// there is one, else added at the end of the section (which is added at
/// the end if it is missing). Line endings are kept.
pub fn set_ini_value(ini: &str, section: &str, key: &str, value: &str) -> String {
    let newline = if ini.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = ini.lines().map(str::to_string).collect();
    let header = format!("[{section}]");
    let new_line = format!("{key} = {value}");
    let Some(start) = lines.iter().position(|line| line.trim().eq_ignore_ascii_case(&header)) else {
        if lines.last().is_some_and(|line| !line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(header);
        lines.push(new_line);
        return lines.join(newline) + newline;
    };
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map_or(lines.len(), |offset| start + 1 + offset);
    let existing = lines[start + 1..end]
        .iter()
        .position(|line| line.split_once('=').is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case(key)));
    match existing {
        Some(offset) => lines[start + 1 + offset] = new_line,
        None => {
            // After the section's last setting, before any blank lines.
            let mut insert_at = end;
            while insert_at > start + 1 && lines[insert_at - 1].trim().is_empty() {
                insert_at -= 1;
            }
            lines.insert(insert_at, new_line);
        }
    }
    lines.join(newline) + newline
}

fn create_folder(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|error| format!("Could not create '{}': {error}", path.display()))
}

fn write(path: &Path, contents: &str) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|error| format!("Could not write '{}': {error}", path.display()))
}

fn copy(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::copy(source, destination).map(|_| ()).map_err(|error| {
        format!("Could not copy '{}' to '{}': {error}", source.display(), destination.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_a_setting_in_its_section_only() {
        let ini = "[Daggerfall]\r\nMyDaggerfallPath = C:\\Old\r\n\r\n[GUI]\r\nShowOptionsAtStart = True\r\nOther = 1\r\n";
        let changed = set_ini_value(ini, "GUI", "ShowOptionsAtStart", "False");
        assert_eq!(
            changed,
            "[Daggerfall]\r\nMyDaggerfallPath = C:\\Old\r\n\r\n[GUI]\r\nShowOptionsAtStart = False\r\nOther = 1\r\n"
        );
        let path = set_ini_value(&changed, "Daggerfall", "MyDaggerfallPath", "D:\\DAGGER");
        assert!(path.starts_with("[Daggerfall]\r\nMyDaggerfallPath = D:\\DAGGER\r\n\r\n[GUI]"), "{path}");
    }

    #[test]
    fn adds_missing_settings_and_sections() {
        let ini = "[Daggerfall]\nMyDaggerfallPath = X\n\n[Video]\nFullscreen = True\n";
        let added = set_ini_value(ini, "Daggerfall", "MyDaggerfallUnitySavePath", "");
        assert_eq!(
            added,
            "[Daggerfall]\nMyDaggerfallPath = X\nMyDaggerfallUnitySavePath = \n\n[Video]\nFullscreen = True\n"
        );
        let section = set_ini_value(&added, "Enhancements", "LypyL_ModSystem", "True");
        assert!(
            section.ends_with("Fullscreen = True\n\n[Enhancements]\nLypyL_ModSystem = True\n"),
            "{section}"
        );
        assert_eq!(
            set_ini_value("", "GUI", "ShowOptionsAtStart", "False"),
            "[GUI]\nShowOptionsAtStart = False\n"
        );
    }
}
