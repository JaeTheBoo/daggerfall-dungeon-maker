//! Importing Blender's own `.blend` files. A `.blend` file is Blender's memory
//! as it saved it, which changes with each version of Blender, so only Blender
//! reads it reliably: Blender itself, run without a window, exports the file as
//! glTF (one GLB file), which the editor then imports like any glTF model. This
//! is how Unity imports `.blend` files too.
//!
//! The export takes what the file's view layer shows, with modifiers applied
//! (as it looks in Blender), without cameras, lights or animation. Scripts
//! saved in the file are not run.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long Blender may take to convert a file before it is stopped.
const TIME_LIMIT: Duration = Duration::from_secs(600);
/// How often the conversion checks whether Blender has finished.
const CHECK_INTERVAL: Duration = Duration::from_millis(100);
/// Windows' flag for starting a console program without opening a console window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The Python script Blender runs once it has opened the file: it exports the
/// file as one GLB file, at the path given after `--`. Only options the
/// installed Blender's glTF exporter has are passed, as they differ between
/// versions.
const EXPORT_SCRIPT: &str = r#"# Exports the open .blend file as one GLB file for Daggerfall Dungeon Maker.
import sys

import bpy

output = sys.argv[sys.argv.index("--") + 1]
if "gltf" not in dir(bpy.ops.export_scene):
    import addon_utils

    addon_utils.enable("io_scene_gltf2", default_set=True)
wanted = {
    "filepath": output,
    "export_format": "GLB",
    "use_visible": True,
    "export_apply": True,
    "export_yup": True,
    "export_texcoords": True,
    "export_normals": True,
    "export_materials": "EXPORT",
    "export_image_format": "AUTO",
    "export_cameras": False,
    "export_lights": False,
    "export_animations": False,
    "export_skins": False,
    "export_morph": False,
    "export_draco_mesh_compression_enable": False,
}
known = bpy.ops.export_scene.gltf.get_rna_type().properties.keys()
result = bpy.ops.export_scene.gltf(**{key: value for key, value in wanted.items() if key in known})
if "FINISHED" not in result:
    raise RuntimeError("the glTF export did not finish: " + str(result))
"#;

/// Whether `path` is a Blender file, which Blender must convert to be imported.
pub fn is_blend_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("blend"))
}

/// The Blender program installed in the usual places, the newest first.
pub fn find_blender() -> Option<PathBuf> {
    installed_blenders().into_iter().next()
}

/// The program to run for the Blender the user chose at `path`: on Windows,
/// Blender's console program (`blender.exe`) rather than its launcher next to it.
pub fn blender_program(path: &Path) -> PathBuf {
    let is_launcher = path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("blender-launcher.exe"));
    match is_launcher {
        true => path.with_file_name("blender.exe"),
        false => path.to_path_buf(),
    }
}

/// The name of Blender's program on this system.
fn program_name() -> &'static str {
    if cfg!(windows) {
        "blender.exe"
    } else {
        "blender"
    }
}

/// Every Blender program installed in the usual places: Blender's installer's
/// folders (newest version first), the system's package folders, Steam's
/// copy, then the folders on the `PATH`.
fn installed_blenders() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut candidates: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for variable in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            let Some(base) = std::env::var_os(variable) else { continue };
            let Ok(entries) = std::fs::read_dir(PathBuf::from(base).join("Blender Foundation")) else {
                continue;
            };
            let mut versions: Vec<PathBuf> =
                entries.flatten().map(|entry| entry.path().join(program_name())).collect();
            versions.sort_by_key(|program| std::cmp::Reverse(version_of(program)));
            candidates.extend(versions);
        }
    } else if cfg!(target_os = "macos") {
        let bundle =
            Path::new("Applications").join("Blender.app").join("Contents").join("MacOS").join("Blender");
        candidates.push(Path::new("/").join(&bundle));
        candidates.extend(home.iter().map(|home| home.join(&bundle)));
    } else {
        candidates.extend(
            ["/snap/bin/blender", "/var/lib/flatpak/exports/bin/org.blender.Blender", "/usr/bin/blender"]
                .map(PathBuf::from),
        );
        candidates.extend(
            home.iter().map(|home| home.join(".local/share/flatpak/exports/bin/org.blender.Blender")),
        );
    }
    for library in steam_libraries() {
        let folder = library.join("steamapps").join("common").join("Blender");
        candidates.push(match cfg!(target_os = "macos") {
            true => folder.join("Blender.app").join("Contents").join("MacOS").join("Blender"),
            false => folder.join(program_name()),
        });
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|folder| folder.join(program_name())));
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        if candidate.is_file() && !found.contains(&candidate) {
            found.push(candidate);
        }
    }
    found
}

/// The version in the name of the folder holding `program` ("Blender 4.2" is
/// 4.2), to put the newest first.
fn version_of(program: &Path) -> Vec<u32> {
    let folder = program.parent().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned());
    let folder = folder.unwrap_or_default();
    let digits = folder.trim_start_matches(|character: char| !character.is_ascii_digit());
    digits.split('.').map_while(|part| part.parse().ok()).collect()
}

/// Steam's library folders: where Steam is installed, and the libraries its
/// `libraryfolders.vdf` lists.
fn steam_libraries() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
            roots.extend(std::env::var_os(variable).map(|base| PathBuf::from(base).join("Steam")));
        }
    } else if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.push(home.join("Library/Application Support/Steam"));
        roots.push(home.join(".steam/steam"));
        roots.push(home.join(".local/share/Steam"));
        roots.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
    }
    let mut libraries = roots.clone();
    for root in &roots {
        let Ok(text) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) else {
            continue;
        };
        libraries.extend(library_paths(&text));
    }
    libraries
}

/// The library folders a Steam `libraryfolders.vdf` file lists: its `"path"` entries.
fn library_paths(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("\"path\"")?.trim();
            let value = rest.strip_prefix('"')?.strip_suffix('"')?;
            Some(PathBuf::from(value.replace("\\\\", "\\")))
        })
        .collect()
}

/// A `.blend` file being converted to GLB by Blender, in the background.
/// Dropping it removes the files the conversion made.
pub struct BlendConversion {
    /// The `.blend` file.
    source: PathBuf,
    /// The folder the conversion works in, which only it uses.
    folder: PathBuf,
    /// The GLB file Blender makes.
    output: PathBuf,
    finished: mpsc::Receiver<Result<(), String>>,
}

impl BlendConversion {
    /// Starts Blender (`blender`) converting `source`, in a folder of its own
    /// under `work_folder`.
    pub fn start(blender: &Path, source: &Path, work_folder: &Path) -> Result<Self, String> {
        static CONVERSIONS: AtomicU32 = AtomicU32::new(0);
        let number = CONVERSIONS.fetch_add(1, Ordering::Relaxed);
        let folder = work_folder.join(format!("{}-{number}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("Could not create '{}': {error}", folder.display()))?;
        let script = folder.join("export.py");
        std::fs::write(&script, EXPORT_SCRIPT)
            .map_err(|error| format!("Could not save '{}': {error}", script.display()))?;
        let stem =
            source.file_stem().map_or_else(|| "model".into(), |stem| stem.to_string_lossy().into_owned());
        let output = folder.join(format!("{stem}.glb"));

        let (sender, finished) = mpsc::channel();
        let (blender, blend, glb, log) =
            (blender.to_path_buf(), source.to_path_buf(), output.clone(), folder.join("blender.log"));
        std::thread::spawn(move || {
            let _ = sender.send(run_blender(&blender, &blend, &script, &glb, &log));
        });
        Ok(Self { source: source.to_path_buf(), folder, output, finished })
    }

    /// The `.blend` file being converted.
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Once Blender has finished: the GLB file it made, or what went wrong.
    pub fn poll(&self) -> Option<Result<PathBuf, String>> {
        match self.finished.try_recv() {
            Ok(result) => Some(result.map(|()| self.output.clone())),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("the conversion stopped unexpectedly".to_string()))
            }
        }
    }
}

impl Drop for BlendConversion {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

/// Runs Blender without a window to export `blend` as `glb` with `script`,
/// writing what it prints to `log`, and waits for it.
fn run_blender(blender: &Path, blend: &Path, script: &Path, glb: &Path, log: &Path) -> Result<(), String> {
    let log_file = std::fs::File::create(log)
        .map_err(|error| format!("Could not create '{}': {error}", log.display()))?;
    let log_copy = log_file.try_clone().map_err(|error| error.to_string())?;
    let mut command = Command::new(blender);
    command
        // Without a window, with Blender's own settings (which keep its glTF
        // exporter on), and without running scripts saved in the file.
        .args(["--background", "--factory-startup", "--disable-autoexec"])
        .arg(blend)
        .args(["--python-exit-code", "1", "--python"])
        .arg(script)
        .arg("--")
        .arg(glb)
        .stdin(Stdio::null())
        .stdout(log_file)
        .stderr(log_copy);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start Blender ('{}'): {error}", blender.display()))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > TIME_LIMIT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Blender took longer than {} minutes to convert the file, so it was stopped",
                    TIME_LIMIT.as_secs() / 60
                ));
            }
            Ok(None) => std::thread::sleep(CHECK_INTERVAL),
            Err(error) => return Err(format!("Could not wait for Blender: {error}")),
        }
    };
    if status.success() && glb.is_file() {
        return Ok(());
    }
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let details = log_errors(&text);
    let mut message = format!("Blender could not convert the file ({status})");
    if !details.is_empty() {
        let _ = write!(message, ":\n{details}");
    }
    Err(message)
}

/// The lines of what Blender printed that explain a failure, or its last lines.
fn log_errors(log: &str) -> String {
    let lines: Vec<&str> = log.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    let start = lines
        .iter()
        .position(|line| line.starts_with("Traceback") || line.to_lowercase().contains("error"))
        .unwrap_or(lines.len().saturating_sub(6));
    lines[start..].iter().take(12).copied().collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_blend_files_and_blenders_versions() {
        assert!(is_blend_file(Path::new("C:/Models/Tower.BLEND")));
        assert!(!is_blend_file(Path::new("C:/Models/Tower.blend1")));
        let program = Path::new("C:/Program Files/Blender Foundation/Blender 4.2/blender.exe");
        assert_eq!(version_of(program), vec![4, 2]);
        assert!(version_of(Path::new("C:/Blender 10.1/blender.exe")) > version_of(program));
        assert_eq!(
            blender_program(Path::new("D:/Blender/blender-launcher.exe")),
            PathBuf::from("D:/Blender/blender.exe")
        );
    }

    #[test]
    fn reads_steams_library_folders() {
        let text =
            "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\
                    \t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"E:\\\\SteamLibrary\"\n\t}\n}\n";
        assert_eq!(
            library_paths(text),
            vec![PathBuf::from("C:\\Program Files (x86)\\Steam"), PathBuf::from("E:\\SteamLibrary")]
        );
    }

    #[test]
    fn explains_what_went_wrong_from_blenders_log() {
        let log = "Blender 5.3.0\nRead blend: \"x.blend\"\nTraceback (most recent call last):\n  File \"export.py\"\n\
                   RuntimeError: Error: something broke\n\nBlender quit\n";
        assert!(log_errors(log).starts_with("Traceback"));
        assert!(log_errors(log).contains("something broke"));
        assert_eq!(log_errors("one\ntwo"), "one\ntwo");
    }
}
