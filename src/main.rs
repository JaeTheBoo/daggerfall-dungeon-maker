// Release builds are GUI applications without a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Entry point: parses the command line, creates the window and starts the dungeon maker.
//!
//! Usage: `daggerfall-dungeon-maker [project.ddm] [--arena2 <folder>] [--cell <name.RDB or name.RMB>]
//! [--dungeon <name>] [--settlement <name>] [--interior <name.RMB>:<building>]`
//!
//! Every argument is optional. Without `--arena2` the program uses the
//! folder chosen last time, or asks for one. Without a project file the
//! project open last time is reopened.

use std::path::PathBuf;

use daggerfall_dungeon_maker::app::{DungeonMakerApp, StartupOptions};
use eframe::egui;

/// The program's name, which also names the folder its settings and crash log are kept in.
const APP_NAME: &str = "Daggerfall Dungeon Maker";

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    log_crashes();
    let startup_options = parse_arguments(std::env::args().skip(1));

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Daggerfall Dungeon Maker")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([800.0, 500.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        native_options,
        Box::new(|creation_context| Ok(Box::new(DungeonMakerApp::new(creation_context, startup_options)))),
    )
}

/// Adds what made the program stop, and where, to `crash.log` in the
/// settings folder: release builds have no console to print it to.
fn log_crashes() {
    let Some(log_path) = eframe::storage_dir(APP_NAME).map(|folder| folder.join("crash.log")) else { return };
    let print_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let thread = std::thread::current();
        let report = format!(
            "--- crash at {seconds} s after 1970 (UTC), version {}, thread '{}'\n{info}\n{}\n",
            env!("CARGO_PKG_VERSION"),
            thread.name().unwrap_or("unnamed"),
            std::backtrace::Backtrace::force_capture(),
        );
        let written = std::fs::create_dir_all(log_path.parent().unwrap_or(&log_path)).and_then(|()| {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)?
                .write_all(report.as_bytes())
        });
        if let Err(error) = written {
            eprintln!("could not write {}: {error}", log_path.display());
        }
        print_panic(info);
    }));
}

/// Reads a project file, `--arena2 <folder>`, `--cell <name>` (or `--room`), `--dungeon <name>`,
/// `--settlement <name>` and `--interior <name>:<building>` from the command line, ignoring anything else.
fn parse_arguments(mut arguments: impl Iterator<Item = String>) -> StartupOptions {
    let mut options = StartupOptions::default();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--arena2" => options.arena2_folder = arguments.next().map(PathBuf::from),
            "--cell" | "--room" => options.room_name = arguments.next(),
            "--dungeon" => options.dungeon_name = arguments.next(),
            "--settlement" => options.settlement_name = arguments.next(),
            "--interior" => options.interior = arguments.next(),
            "--project" => options.project_file = arguments.next().map(PathBuf::from),
            path if path.to_ascii_lowercase().ends_with(".ddm") => {
                options.project_file = Some(PathBuf::from(path))
            }
            unknown => log::warn!("ignoring unknown argument '{unknown}'"),
        }
    }
    options
}
