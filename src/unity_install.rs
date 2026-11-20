//! Text Daggerfall Unity keeps in its own install folder, which names the
//! buildings of Daggerfall's settlements as the game does.
//!
//! Daggerfall Unity ships its word lists in
//! `DaggerfallUnity_Data/StreamingAssets/Text/Master Localization CSV Files`:
//! `Internal_Strings.csv` (the halves of shop and tavern names, "The Bank
//! of", rulers' titles…) and `NameGen.txt` (the parts people's names are
//! made of). Nothing of them is stored with the program: they are read from
//! the user's install, which is found where Daggerfall Unity is usually
//! unzipped, or chosen with File > Set Daggerfall Unity folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Daggerfall Unity's data folder inside its install folder (Windows and Linux).
const DATA_FOLDER: &str = "DaggerfallUnity_Data";
/// The text folder inside the data folder.
const TEXT_FOLDER: &str = "StreamingAssets/Text";
/// The folder of Daggerfall Unity's text tables inside the text folder.
const MASTER_FOLDER: &str = "Master Localization CSV Files";
/// The table of strings building names are made from.
const STRINGS_FILE: &str = "Internal_Strings.csv";
/// The parts people's names are made from.
const NAME_GENERATOR_FILE: &str = "NameGen.txt";

/// Daggerfall Unity's strings and name parts.
#[derive(Debug, Clone, Default)]
pub struct UnityText {
    /// Every string of `Internal_Strings.csv`, by key.
    strings: HashMap<String, String>,
    /// The sets of name parts of each name bank ("Breton", "Redguard"…).
    name_banks: HashMap<String, Vec<Vec<String>>>,
}

impl UnityText {
    /// Reads the text of the Daggerfall Unity installed in `install_folder`.
    pub fn load(install_folder: &Path) -> Result<Self, String> {
        let text = text_folder(install_folder)
            .ok_or_else(|| format!("{} is not a Daggerfall Unity folder", install_folder.display()))?;
        let master = text.join(MASTER_FOLDER);
        let read = |path: &Path| {
            std::fs::read_to_string(path)
                .map_err(|error| format!("Could not read {}: {error}", path.display()))
        };
        let strings = parse_strings(&read(&master.join(STRINGS_FILE))?);
        // Daggerfall Unity uses a NameGen.txt put straight into the text folder instead of its own.
        let names_path = [text.join(NAME_GENERATOR_FILE), master.join(NAME_GENERATOR_FILE)]
            .into_iter()
            .find(|path| path.is_file())
            .ok_or_else(|| format!("{NAME_GENERATOR_FILE} is missing from {}", master.display()))?;
        let name_banks = parse_name_banks(&read(&names_path)?)?;
        Ok(Self { strings, name_banks })
    }

    /// Daggerfall Unity's text from its strings and the text of its `NameGen.txt`.
    #[cfg(test)]
    pub(crate) fn from_parts(strings: HashMap<String, String>, name_generator: &str) -> Result<Self, String> {
        Ok(Self { strings, name_banks: parse_name_banks(name_generator)? })
    }

    /// The string called `key`, if there is one.
    pub fn string(&self, key: &str) -> Option<&str> {
        self.strings.get(key).map(String::as_str)
    }

    /// The list of strings called `key` (stored one per line); empty if there is none.
    pub fn list(&self, key: &str) -> Vec<&str> {
        self.string(key).map(|text| text.lines().collect()).unwrap_or_default()
    }

    /// The sets of name parts of the name bank `bank`, for example "Breton".
    pub fn name_bank(&self, bank: &str) -> Option<&[Vec<String>]> {
        self.name_banks.get(bank).map(Vec::as_slice)
    }
}

/// The text folder of the Daggerfall Unity installed in `folder`, if it is one.
fn text_folder(folder: &Path) -> Option<PathBuf> {
    let text = folder.join(DATA_FOLDER).join(TEXT_FOLDER);
    text.join(MASTER_FOLDER).join(STRINGS_FILE).is_file().then_some(text)
}

/// The install folder of Daggerfall Unity for `folder`, which may be the
/// install folder itself or a folder inside it (its data folder, its
/// `StreamingAssets` or `Mods` folder…).
pub fn resolve_install_folder(folder: &Path) -> Option<PathBuf> {
    folder.ancestors().take(5).find(|candidate| text_folder(candidate).is_some()).map(Path::to_path_buf)
}

/// Looks for Daggerfall Unity where it is usually unzipped, and in and
/// above the folders of `hints` (such as the export's Mods folder).
pub fn find_install_folder(hints: &[PathBuf]) -> Option<PathBuf> {
    if let Some(found) = hints.iter().find_map(|hint| resolve_install_folder(hint)) {
        return Some(found);
    }
    const NAMES: [&str; 2] = ["Daggerfall Unity", "DaggerfallUnity"];
    let mut parents: Vec<PathBuf> = Vec::new();
    if let Some(home) = home_folder() {
        for folder in ["", "Games", "Desktop", "Downloads", "Documents"] {
            parents.push(home.join(folder));
        }
    }
    if cfg!(windows) {
        for drive in 'C'..='Z' {
            let root = PathBuf::from(format!("{drive}:\\"));
            for folder in ["", "Games", "Program Files", "Program Files (x86)"] {
                parents.push(root.join(folder));
            }
        }
    } else {
        parents.push(PathBuf::from("/opt"));
    }
    parents
        .iter()
        .flat_map(|parent| NAMES.iter().map(move |name| parent.join(name)))
        .find(|candidate| text_folder(candidate).is_some())
}

/// The user's home folder.
fn home_folder() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Reads a `Key,Value` table as Daggerfall Unity writes its CSV files:
/// values in double quotes may hold commas and line breaks, and a quote
/// inside one is doubled.
fn parse_strings(text: &str) -> HashMap<String, String> {
    let mut strings = HashMap::new();
    let mut fields: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut characters = text.trim_start_matches('\u{feff}').chars().peekable();
    let mut end_row = |fields: &mut Vec<String>, field: &mut String| {
        fields.push(std::mem::take(field));
        if let [key, value, ..] = fields.as_slice() {
            if !key.is_empty() {
                strings.insert(key.clone(), value.replace("\r\n", "\n"));
            }
        }
        fields.clear();
    };
    while let Some(character) = characters.next() {
        match (character, in_quotes) {
            ('"', true) if characters.peek() == Some(&'"') => {
                characters.next();
                field.push('"');
            }
            ('"', _) => in_quotes = !in_quotes,
            (',', false) => fields.push(std::mem::take(&mut field)),
            ('\r', false) => {}
            ('\n', false) => end_row(&mut fields, &mut field),
            (character, _) => field.push(character),
        }
    }
    if !field.is_empty() || !fields.is_empty() {
        end_row(&mut fields, &mut field);
    }
    strings.remove("Key");
    strings
}

/// Reads `NameGen.txt`: for each name bank, its sets of name parts.
fn parse_name_banks(text: &str) -> Result<HashMap<String, Vec<Vec<String>>>, String> {
    #[derive(serde::Deserialize)]
    struct Bank {
        sets: Vec<Set>,
    }
    #[derive(serde::Deserialize)]
    struct Set {
        parts: Vec<String>,
    }
    let text = repair_missing_commas(text.trim_start_matches('\u{feff}'));
    let banks: HashMap<String, Bank> = serde_json::from_str(&text)
        .map_err(|error| format!("{NAME_GENERATOR_FILE} does not read: {error}"))?;
    Ok(banks
        .into_iter()
        .map(|(name, bank)| (name, bank.sets.into_iter().map(|set| set.parts).collect()))
        .collect())
}

/// Mends the commas of Daggerfall Unity's own `NameGen.txt`, which its JSON
/// reader does without: a value starting after another, with only white
/// space between, gets one, and one just before a closing bracket goes.
fn repair_missing_commas(text: &str) -> String {
    let mut repaired = String::with_capacity(text.len() + 16);
    let mut in_string = false;
    let mut escaped = false;
    // The last character outside white space, and whether white space followed it.
    let mut last = ' ';
    let mut spaced = false;
    // Where the last comma was written, while it is the last character outside white space.
    let mut last_comma: Option<usize> = None;
    for character in text.chars() {
        if in_string {
            repaired.push(character);
            match (escaped, character) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => {
                    in_string = false;
                    last = '"';
                    spaced = false;
                }
                _ => {}
            }
            continue;
        }
        if character.is_whitespace() {
            spaced = true;
            repaired.push(character);
            continue;
        }
        let starts_value = matches!(character, '{' | '[' | '"' | '-' | '0'..='9' | 't' | 'f' | 'n');
        let ends_value = matches!(last, '}' | ']' | '"' | '0'..='9' | 'e' | 'l');
        if starts_value && ends_value && spaced {
            repaired.push(',');
        }
        if matches!(character, '}' | ']') {
            if let Some(comma) = last_comma {
                repaired.remove(comma);
            }
        }
        last_comma = (character == ',').then_some(repaired.len());
        repaired.push(character);
        in_string = character == '"';
        last = character;
        spaced = false;
    }
    repaired
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_quoted_multi_line_strings() {
        let text = "\u{feff}Key,Value\nTavernsB,\"Chasm\nMug\"\nquote,\"He said \"\"hi\"\", then left\"\r\nplain,Palace\n";
        let strings = parse_strings(text);
        assert_eq!(strings["TavernsB"], "Chasm\nMug");
        assert_eq!(strings["quote"], "He said \"hi\", then left");
        assert_eq!(strings["plain"], "Palace");
        assert!(!strings.contains_key("Key"));

        let unity = UnityText { strings, ..UnityText::default() };
        assert_eq!(unity.list("TavernsB"), ["Chasm", "Mug"]);
        assert!(unity.list("missing").is_empty());
    }

    #[test]
    fn reads_name_banks() {
        let text = r#"{"Breton": {"setCount": 2, "sets": [{"setIndex": 0, "parts": ["Theod", "And"]},
            {"setIndex": 1, "parts": ["ric"]}]}}"#;
        let banks = parse_name_banks(text).unwrap();
        assert_eq!(banks["Breton"], [vec!["Theod".to_string(), "And".to_string()], vec!["ric".to_string()]]);

        // Daggerfall Unity's own file misses some commas between sets.
        let missing = "{\"Nord\": {\"sets\": [{\"parts\": [\"a\"]}\n\t{\"parts\": [\"b\", \"c,d\"]},\n]}}";
        assert_eq!(parse_name_banks(missing).unwrap()["Nord"][1], ["b", "c,d"]);
    }

    #[test]
    fn finds_the_install_folder_from_inside_it() {
        let root = std::env::temp_dir().join(format!("ddm-unity-{}", std::process::id()));
        let master = root.join(DATA_FOLDER).join(TEXT_FOLDER).join(MASTER_FOLDER);
        std::fs::create_dir_all(&master).unwrap();
        std::fs::write(master.join(STRINGS_FILE), "Key,Value\n").unwrap();
        let mods = root.join(DATA_FOLDER).join("StreamingAssets").join("Mods");
        assert_eq!(resolve_install_folder(&mods).as_deref(), Some(root.as_path()));
        assert_eq!(resolve_install_folder(&root).as_deref(), Some(root.as_path()));
        assert!(resolve_install_folder(&std::env::temp_dir().join("ddm-no-such-folder")).is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
