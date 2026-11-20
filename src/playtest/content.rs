//! The test's own content mod: what loose world data cannot carry, built
//! into a `.dfmod` as an export builds one.
//!
//! The user's imported models, a dungeon's own music and sky, and objects
//! at a size of their own reach the game only in a `.dfmod`: the models as
//! prefabs, the music and sky as files with scripts that play and show them,
//! and the scaled objects through a script that puts them in place (see
//! [`crate::export::ModExtras`]). When a test has any of these, its plan
//! carries a mod of them ([`TestPlan::content`](super::TestPlan::content)),
//! which is built with the Unity editor, as exporting a `.dfmod` is, and
//! installed beside the test mod. The world data stays loose, so moving
//! objects about between tests needs no new build: only a change to the
//! mod's own files does. A few builds are kept, so going back to a test
//! tried before reuses its build too.

use std::path::{Path, PathBuf};

use crate::export::unity::{self, BuildRequest, BuildTarget};
use crate::export::{ModExtras, ModFiles};
use crate::project::ModDetails;

/// The content mod's title, which gives its file name.
const TITLE: &str = "Dungeon Maker test content";
/// The content mod's identity, which stays the same from test to test.
const GUID: &str = "8b3e6c1d-27f4-4a95-b0d8-5e9a1f6c3d27";
/// How many builds of content mods are kept.
const KEPT_BUILDS: usize = 4;
/// The file in a kept build's folder whose time says when the build was last used.
const USED_FILE: &str = "used.txt";

/// The details the content mod is made with.
pub fn details() -> ModDetails {
    ModDetails {
        title: TITLE.to_string(),
        version: "1.0".to_string(),
        author: "Daggerfall Dungeon Maker".to_string(),
        contact: String::new(),
        description: "What a Daggerfall Dungeon Maker test needs besides its world data. \
                      It is only in the program's own copy of Daggerfall Unity."
            .to_string(),
        guid: GUID.to_string(),
    }
}

/// The content mod's file name, which its scripts are named after.
pub fn file_name() -> String {
    crate::export::file_safe_name(TITLE, "DungeonMakerTestContent")
}

/// The content mod of `extras`, or `None` if there is nothing in them.
pub(crate) fn of_extras(extras: ModExtras) -> Option<ModFiles> {
    if extras.is_empty() {
        return None;
    }
    let file_name = file_name();
    let files = extras.scripts(&file_name);
    with_manifest(file_name, files, extras.copied_files)
}

/// The content mod of an export's `mod_files`: its scripts and copied files,
/// without its world data (which the test writes loose) or its manifest.
/// `None` if there is nothing but world data.
pub fn of_export(mod_files: ModFiles) -> Option<ModFiles> {
    let world_data = format!("{}/", crate::export::WORLD_DATA_FOLDER);
    let manifest = mod_files.manifest_path();
    let files: Vec<_> = mod_files
        .files
        .into_iter()
        .filter(|file| !file.path.starts_with(&world_data) && file.path != manifest)
        .collect();
    if files.is_empty() && mod_files.copied_files.is_empty() {
        return None;
    }
    with_manifest(mod_files.file_name, files, mod_files.copied_files)
}

fn with_manifest(
    file_name: String,
    mut files: Vec<crate::export::ExportedFile>,
    copied_files: Vec<crate::export::CopiedFile>,
) -> Option<ModFiles> {
    files.push(crate::export::manifest_file(TITLE, &file_name, &details(), &files, &copied_files));
    Some(ModFiles {
        title: TITLE.to_string(),
        file_name,
        files,
        copied_files,
        dungeons: Vec::new(),
        cells: Vec::new(),
    })
}

/// The content mod `content` built for this computer's platform, building it
/// with the Unity editor `unity_editor` first unless a kept build of the same
/// files is in `cache`.
pub fn built(
    cache: &Path,
    unity_editor: Option<&Path>,
    content: &ModFiles,
    step: &dyn Fn(String),
) -> Result<PathBuf, String> {
    let version = format!("{:016x}", version(content));
    let folder = cache.join(&version);
    let target = BuildTarget::current();
    let mod_folder = folder.join(&content.file_name);
    let bundle = mod_folder.join(target.name()).join(format!("{}.dfmod", content.file_name.to_lowercase()));
    if !bundle.is_file() {
        let unity_editor = unity_editor.ok_or(
            "This test has imported models, its own music or sky, or scaled objects, which only a .dfmod \
             carries, and building one needs the Unity editor (Unity 2019.4, as for building a .dfmod in \
             File > Export). Install Unity 2019.4.40f1 with Unity Hub, or choose it in the export window.",
        )?;
        if folder.exists() {
            std::fs::remove_dir_all(&folder)
                .map_err(|error| format!("Could not clear '{}': {error}", folder.display()))?;
        }
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("Could not create '{}': {error}", folder.display()))?;
        crate::export::write_mod(content, &folder)?;
        step(
            "Building the test's models, music, sky and scaled objects with Unity (a minute or two; \
             the build is kept for the next test of the same things)…"
                .to_string(),
        );
        unity::build(
            &BuildRequest {
                unity_editor: unity_editor.to_path_buf(),
                build_project: unity::default_build_project(),
                mod_folder: mod_folder.clone(),
                file_name: content.file_name.clone(),
                targets: vec![target],
                mods_folder: None,
            },
            step,
        )?;
        if !bundle.is_file() {
            return Err(format!("Unity finished but the test's content is not at '{}'", bundle.display()));
        }
    }
    // Marks the build as just used, and lets go of the builds used longest ago.
    let _ = std::fs::write(folder.join(USED_FILE), "");
    forget_old_builds(cache);
    Ok(bundle)
}

/// What tells builds of `content` apart: its files' contents, and the
/// files it copies, by where they are, their size and when they changed.
fn version(content: &ModFiles) -> u64 {
    let mut parts = Vec::new();
    for file in &content.files {
        parts.push(file.path.clone());
        parts.push(file.contents.clone());
    }
    for file in &content.copied_files {
        let (size, changed) = std::fs::metadata(&file.source)
            .map(|metadata| {
                let changed = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |since| since.as_nanos());
                (metadata.len(), changed)
            })
            .unwrap_or((0, 0));
        parts.push(format!("{} {} {size} {changed}", file.path, file.source.display()));
    }
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    super::script::stable_hash(&parts)
}

/// Removes all but the [`KEPT_BUILDS`] builds in `cache` used last.
fn forget_old_builds(cache: &Path) {
    let Ok(entries) = std::fs::read_dir(cache) else { return };
    let mut builds: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| {
            let used = std::fs::metadata(entry.path().join(USED_FILE))
                .or_else(|_| entry.metadata())
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (used, entry.path())
        })
        .collect();
    builds.sort_by_key(|(used, _)| std::cmp::Reverse(*used));
    for (_, folder) in builds.into_iter().skip(KEPT_BUILDS) {
        let _ = std::fs::remove_dir_all(folder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::ExportedFile;

    #[test]
    fn an_export_gives_its_scripts_and_files_but_not_its_world_data() {
        let file = |path: &str| ExportedFile { path: path.to_string(), contents: "x".to_string() };
        let export = ModFiles {
            title: TITLE.to_string(),
            file_name: file_name(),
            files: vec![
                file("WorldData/N9000001.RDB.json"),
                file(&format!("{}DungeonMusic.cs.txt", file_name())),
                file(&format!("{}.dfmod.json", file_name())),
            ],
            copied_files: Vec::new(),
            dungeons: Vec::new(),
            cells: Vec::new(),
        };
        let content = of_export(export).expect("a script needs a .dfmod");
        let paths: Vec<&str> = content.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, [format!("{}DungeonMusic.cs.txt", file_name()), content.manifest_path()]);
        let manifest = &content.files[1].contents;
        assert!(manifest.contains("DungeonMusic.cs.txt") && !manifest.contains("WorldData"), "{manifest}");
    }

    #[test]
    fn world_data_alone_needs_no_content_mod() {
        let export = ModFiles {
            title: TITLE.to_string(),
            file_name: file_name(),
            files: vec![ExportedFile { path: "WorldData/a.json".to_string(), contents: String::new() }],
            copied_files: Vec::new(),
            dungeons: Vec::new(),
            cells: Vec::new(),
        };
        assert!(of_export(export).is_none());
        assert!(of_extras(ModExtras::default()).is_none());
    }
}
