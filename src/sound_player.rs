//! Playing Daggerfall's sound effects, to hear what an action plays.
//!
//! The sound is written to the temporary folder as a WAV file and played
//! with what the system already has: on Windows its own sound player, on
//! macOS `afplay`, on Linux `pw-play`, `paplay` or `aplay`. Playing a sound
//! stops the one before.

use std::path::Path;

/// Plays the WAV file `wav`, saved as `name` in the temporary folder.
pub fn play_wav(wav: &[u8], name: &str) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!("daggerfall-dungeon-maker-{name}.wav"));
    std::fs::write(&path, wav).map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    play_file(&path)
}

/// Plays the WAV file at `path` through Windows' own sound player.
#[cfg(windows)]
fn play_file(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "winmm")]
    extern "system" {
        fn PlaySoundW(sound: *const u16, module: *mut std::ffi::c_void, flags: u32) -> i32;
    }
    const SND_ASYNC: u32 = 0x0001;
    const SND_NODEFAULT: u32 = 0x0002;
    const SND_FILENAME: u32 = 0x0002_0000;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL-terminated file name that lives until the call
    // returns, and no module handle is passed. With SND_FILENAME Windows
    // reads the file itself, so nothing has to outlive the call; a new call
    // stops the sound that is playing.
    let played =
        unsafe { PlaySoundW(wide.as_ptr(), std::ptr::null_mut(), SND_ASYNC | SND_NODEFAULT | SND_FILENAME) };
    if played != 0 {
        Ok(())
    } else {
        Err("Windows could not play the sound".to_string())
    }
}

/// Plays the WAV file at `path` with the first sound player found.
#[cfg(not(windows))]
fn play_file(path: &Path) -> Result<(), String> {
    use std::process::{Child, Command, Stdio};
    use std::sync::Mutex;

    static PLAYING: Mutex<Option<Child>> = Mutex::new(None);
    let players: &[&str] =
        if cfg!(target_os = "macos") { &["afplay"] } else { &["pw-play", "paplay", "aplay"] };

    let mut playing = PLAYING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(mut child) = playing.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    for player in players {
        let spawned = Command::new(player)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(child) = spawned {
            *playing = Some(child);
            return Ok(());
        }
    }
    Err(format!("Playing sounds needs one of these programs installed: {}", players.join(", ")))
}
