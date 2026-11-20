//! Daggerfall's factions, listed in the text file `FACTION.TXT`.
//!
//! Each faction is a block of lines: `#` and its id, then `tag: value`
//! lines such as `name: Clavicus Vile` and `type: 0`. Lines starting with
//! `;` are comments. A faction indented by more tabs than the one before
//! belongs to it (its parent). A flat of a person in a cell can name a
//! faction; one of the "individual" kind makes the flat that named person,
//! whom quests use. Temples and guild halls take their names from theirs.

use std::collections::BTreeMap;

use super::TextureKey;

/// One faction of `FACTION.TXT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Faction {
    pub id: u16,
    pub name: String,
    /// The kind of faction (see [`faction_kind_name`]), or -1 if not given.
    pub kind: i32,
    /// The region it belongs to (0-based, as the game counts them), or -1 for none.
    pub region: i32,
    /// For a province, its ruler's title (1 King, 2 Queen, 3 Duke…), or -1.
    pub ruler: i32,
    /// Its people's social group (see [`social_group_name`]): how they talk and react.
    /// 0 when not given, as Daggerfall Unity reads it.
    pub social_group: i32,
    /// Its people's guild group (see [`guild_group_name`]): which guild's services and quests they give.
    pub guild_group: i32,
    /// Its people's race (see [`faction_race_name`]).
    pub race: i32,
    /// The images its people are drawn with when the game places them: the
    /// first for both, the second for a woman.
    pub images: Vec<TextureKey>,
    /// The faction it belongs to, if any.
    pub parent: Option<u16>,
    /// The factions belonging to it, in the file's order.
    pub children: Vec<u16>,
}

/// The kind of faction that is one named person.
pub const INDIVIDUAL_FACTION_KIND: i32 = 4;
/// The kind of faction that is a province, which has a ruler.
pub const PROVINCE_FACTION_KIND: i32 = 7;

/// The factions of `FACTION.TXT`, by id. When two share an id, the first
/// keeps it, as in Daggerfall Unity (which gives the others new ids).
pub fn read_factions(text: &str) -> BTreeMap<u16, Faction> {
    // Each faction's lines, and how many tabs its `#` line starts with.
    let mut blocks: Vec<(usize, Vec<&str>)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(';') || trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            let tabs = line.chars().take_while(|&character| character == '\t').count();
            blocks.push((tabs, Vec::new()));
        }
        if let Some((_, lines)) = blocks.last_mut() {
            lines.push(trimmed);
        }
    }

    let mut factions: BTreeMap<u16, Faction> = BTreeMap::new();
    // The ids of the factions enclosing the current one, by depth.
    let mut parents: Vec<u16> = Vec::new();
    let mut last_tabs = 0;
    let mut previous: Option<u16> = None;
    for (tabs, lines) in blocks {
        if tabs > last_tabs {
            parents.extend(previous);
        } else {
            parents.truncate(tabs.min(parents.len()));
        }
        last_tabs = tabs;
        let Some(mut faction) = parse_faction(&lines) else { continue };
        faction.parent = parents.last().copied();
        previous = Some(faction.id);
        if factions.contains_key(&faction.id) {
            continue;
        }
        if let Some(parent) = faction.parent.and_then(|parent| factions.get_mut(&parent)) {
            parent.children.push(faction.id);
        }
        factions.insert(faction.id, faction);
    }
    factions
}

/// One faction from its lines, or `None` if it has no id.
fn parse_faction(lines: &[&str]) -> Option<Faction> {
    let id = lines.first()?.strip_prefix('#')?.trim().parse().ok()?;
    let mut faction = Faction {
        id,
        name: String::new(),
        kind: -1,
        region: -1,
        ruler: -1,
        social_group: 0,
        guild_group: 0,
        race: 0,
        images: Vec::new(),
        parent: None,
        children: Vec::new(),
    };
    for line in &lines[1..] {
        // One tag in Daggerfall's file lacks its colon; a space separates it instead.
        let (tag, value) =
            line.split_once(':').or_else(|| line.split_once(char::is_whitespace)).unwrap_or((line, ""));
        let number = || value.trim().parse::<i32>().unwrap_or(-1);
        match tag.trim().to_ascii_lowercase().as_str() {
            "name" => faction.name = value.trim().to_string(),
            "type" => faction.kind = number(),
            // Stored counting from 1; the game counts from 0, as here.
            "region" => faction.region = if number() == -1 { -1 } else { number() - 1 },
            "ruler" => faction.ruler = number(),
            "sgroup" => faction.social_group = number(),
            "ggroup" => faction.guild_group = number(),
            "race" => faction.race = number(),
            "flat" => {
                let mut numbers = value.split_whitespace().map(str::parse::<u16>);
                if let (Some(Ok(archive)), Some(Ok(record))) = (numbers.next(), numbers.next()) {
                    if archive > 0 {
                        faction.images.push(TextureKey::new(archive, record as u8));
                    }
                }
            }
            _ => {}
        }
    }
    Some(faction)
}

/// What social group `group` is (`FactionFile.SocialGroups`): it decides
/// how the faction's people talk and react to the player.
pub fn social_group_name(group: i32) -> String {
    match group {
        -1 => "None".to_string(),
        0 => "Commoners".to_string(),
        1 => "Merchants".to_string(),
        2 => "Scholars".to_string(),
        3 => "Nobility".to_string(),
        4 => "Underworld".to_string(),
        6 => "Supernatural beings".to_string(),
        7 => "Guild members".to_string(),
        group => format!("Social group {group}"),
    }
}

/// What guild group `group` is (`FactionFile.GuildGroups`): it decides
/// which guild's services and quests the faction's people give.
pub fn guild_group_name(group: i32) -> String {
    let name = match group {
        -1 => "None",
        2 => "Oblivion",
        3 => "Dark Brotherhood",
        4 => "General populace (the Thieves Guild when underworld)",
        5 => "Bards",
        6 => "The Fey",
        7 => "Prostitutes",
        9 => "Knightly order",
        10 => "Mages Guild",
        11 => "Fighters Guild",
        14 => "Necromancers",
        15 => "Region",
        17 => "Holy order",
        22 => "Witches",
        23 => "Vampires",
        24 => "Orsinium",
        group => return format!("Guild group {group}"),
    };
    name.to_string()
}

/// What race `race` is in `FACTION.TXT` (`FactionFile.FactionRaces`).
pub fn faction_race_name(race: i32) -> String {
    let name = match race {
        -1 => "None",
        0 => "Nord",
        1 => "Khajiit",
        2 => "Redguard",
        3 => "Breton",
        4 => "Argonian",
        5 => "Wood Elf",
        6 => "High Elf",
        7 => "Dark Elf",
        11 => "Skakmat",
        17 => "Orc",
        18 => "Vampire",
        19 => "Fey",
        race => return format!("Race {race}"),
    };
    name.to_string()
}

/// A province's ruler's title (`ruler` in `FACTION.TXT`), as Daggerfall Unity
/// words it; "Lord" for any other number.
pub fn ruler_title_name(ruler: i32) -> &'static str {
    match ruler {
        1 => "King",
        2 => "Queen",
        3 => "Duke",
        4 => "Duchess",
        5 => "Marquis",
        6 => "Marquise",
        7 => "Count",
        8 => "Countess",
        9 => "Baron",
        10 => "Baroness",
        12 => "Lady",
        _ => "Lord",
    }
}

/// What kind of faction `kind` is, as `FACTION.TXT` lists them.
pub fn faction_kind_name(kind: i32) -> &'static str {
    match kind {
        0 => "Daedra",
        1 => "God",
        2 => "Group",
        3 => "Subgroup",
        4 => "Individual",
        5 => "Official",
        6 => "Vampire clan",
        7 => "Province",
        8 => "Witches coven",
        9 => "Temple",
        10 => "Knightly guard",
        11 => "Magic user",
        12 => "Generic",
        13 => "Thieves",
        14 => "Court",
        15 => "People",
        _ => "Unknown kind",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nested_factions_and_keeps_the_first_of_an_id() {
        let text = "; comment\n#17\ntype: 2\nname: Oblivion\n\n\t#1\n\ttype: 0\n\tname: Clavicus Vile\n\
                    \t#1\n\tname: Duplicate\n#145\nname Lord Kilbar\ntype: 4\nregion: 18\nruler: 1\n";
        let factions = read_factions(text);
        assert_eq!(factions.len(), 3);
        assert_eq!(factions[&17].name, "Oblivion");
        assert_eq!(factions[&17].kind, 2);
        assert_eq!(factions[&17].children, [1]);
        assert_eq!(factions[&1].name, "Clavicus Vile");
        assert_eq!(factions[&1].parent, Some(17));
        assert_eq!(factions[&145].name, "Lord Kilbar", "a tag without its colon still reads");
        assert_eq!(factions[&145].kind, INDIVIDUAL_FACTION_KIND);
        assert_eq!(factions[&145].parent, None, "back at the top level");
        assert_eq!((factions[&145].region, factions[&145].ruler), (17, 1));
        assert_eq!(faction_kind_name(factions[&1].kind), "Daedra");
    }
}
