//! What a dungeon's treasure holds, as Daggerfall Unity fills it
//! (`LootTables.GenerateLoot`, MIT licence).
//!
//! Treasure markers and fixed treasure piles in a dungeon become loot piles
//! filled from one of 21 tables, picked by the dungeon's type. A table gives
//! a range of gold, multiplied by the player's level, and for each kind of
//! item a chance in 100: an item is added on a successful roll, then the
//! chance is halved and rolled again, until a roll fails. Tables J to O add
//! a chance of a map, a potion and a potion recipe.

use crate::encounters::DungeonType;

/// One loot table (`LootChanceMatrix`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LootTable {
    /// Its letter, A to U.
    pub key: char,
    /// Gold, before it is multiplied by the player's level.
    pub min_gold: u16,
    pub max_gold: u16,
    /// Chances in 100 of each kind of item, in [`ITEM_KINDS`]' order.
    chances: [u8; 13],
}

/// The kinds of item a table gives chances of, in its order: plant, creature
/// and other ingredients, armor, weapons, magic items, clothing, books and
/// religious items.
const ITEM_KINDS: [&str; 13] = [
    "plant ingredients",
    "plant ingredients",
    "creature ingredients",
    "creature ingredients",
    "creature ingredients",
    "other ingredients",
    "armor",
    "weapons",
    "magic items",
    "clothing",
    "books",
    "other ingredients",
    "religious items",
];

/// Daggerfall Unity's loot tables (`LootTables.DefaultLootTables`): key, gold
/// range, then P1, P2, C1, C2, C3, M1, AM, WP, MI, CL, BK, M2, RL.
const TABLES: [(char, u16, u16, [u8; 13]); 21] = [
    ('A', 1, 10, [0, 0, 0, 0, 0, 0, 5, 5, 2, 4, 0, 2, 0]),
    ('B', 0, 0, [10, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
    ('C', 2, 20, [10, 10, 5, 5, 5, 5, 5, 25, 3, 0, 2, 2, 2]),
    ('D', 1, 4, [6, 6, 6, 6, 6, 6, 0, 0, 0, 0, 0, 0, 4]),
    ('E', 20, 80, [0, 0, 0, 0, 0, 0, 10, 10, 3, 4, 2, 1, 15]),
    ('F', 4, 30, [2, 2, 5, 5, 5, 2, 50, 50, 1, 0, 0, 3, 0]),
    ('G', 3, 15, [0, 0, 0, 0, 0, 0, 50, 50, 1, 5, 0, 3, 0]),
    ('H', 2, 10, [0, 0, 0, 0, 0, 0, 0, 100, 1, 2, 0, 0, 0]),
    ('I', 0, 0, [0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 5]),
    ('J', 50, 150, [0, 0, 0, 0, 0, 0, 5, 5, 3, 0, 0, 0, 0]),
    ('K', 1, 10, [3, 3, 3, 3, 3, 3, 5, 5, 3, 0, 5, 2, 100]),
    ('L', 1, 20, [0, 0, 3, 3, 3, 3, 50, 50, 1, 75, 0, 5, 3]),
    ('M', 1, 15, [1, 1, 1, 1, 1, 2, 10, 10, 1, 15, 2, 3, 1]),
    ('N', 1, 80, [5, 5, 5, 5, 5, 5, 5, 5, 1, 20, 5, 2, 5]),
    ('O', 5, 20, [1, 1, 1, 1, 1, 1, 10, 15, 2, 0, 0, 0, 0]),
    ('P', 5, 20, [5, 5, 5, 5, 5, 5, 5, 10, 2, 0, 10, 5, 0]),
    ('Q', 20, 80, [2, 2, 8, 8, 8, 2, 10, 25, 3, 35, 5, 3, 0]),
    ('R', 5, 20, [0, 0, 3, 3, 3, 5, 5, 15, 2, 0, 0, 0, 0]),
    ('S', 50, 125, [5, 5, 5, 5, 5, 15, 10, 10, 3, 0, 5, 5, 0]),
    ('T', 20, 80, [0, 0, 0, 0, 0, 0, 100, 100, 1, 0, 0, 0, 0]),
    ('U', 7, 30, [5, 5, 5, 5, 5, 10, 10, 10, 2, 0, 2, 2, 10]),
];

/// The table of each dungeon type, by its number (`LootTables.lootTableKeys`).
const DUNGEON_TABLES: [char; 19] =
    ['K', 'N', 'N', 'N', 'K', 'M', 'M', 'Q', 'K', 'U', 'D', 'N', 'L', 'F', 'S', 'N', 'M', 'L', 'N'];

/// Chances in 100 of a map in tables J to O.
const MAP_CHANCES: [(char, u8); 6] = [('J', 2), ('K', 1), ('L', 1), ('M', 2), ('N', 2), ('O', 15)];
/// Chances in 100 of a potion and of a potion recipe in tables J to O.
const POTION_CHANCE: u8 = 4;
const POTION_RECIPE_CHANCE: u8 = 2;

/// The loot table the treasure of a dungeon of `dungeon_type` is filled from.
pub fn loot_table(dungeon_type: DungeonType) -> Option<LootTable> {
    let key = *DUNGEON_TABLES.get(usize::from(dungeon_type.raw()))?;
    TABLES
        .iter()
        .find(|(table_key, ..)| *table_key == key)
        .map(|&(key, min_gold, max_gold, chances)| LootTable { key, min_gold, max_gold, chances })
}

impl LootTable {
    /// Each kind of item it can give, with its best chance in 100, most likely first.
    pub fn items(&self) -> Vec<(&'static str, u8)> {
        let mut items: Vec<(&'static str, u8)> = Vec::new();
        for (&kind, &chance) in ITEM_KINDS.iter().zip(&self.chances) {
            if chance == 0 {
                continue;
            }
            match items.iter_mut().find(|(known, _)| *known == kind) {
                Some((_, best)) => *best = (*best).max(chance),
                None => items.push((kind, chance)),
            }
        }
        items.sort_by_key(|&(_, chance)| std::cmp::Reverse(chance));
        items
    }

    /// What it adds besides, with chances in 100: a map, a potion and a potion recipe, for tables J to O.
    pub fn extras(&self) -> Vec<(&'static str, u8)> {
        match MAP_CHANCES.iter().find(|(key, _)| *key == self.key) {
            Some(&(_, map)) => {
                vec![("a potion", POTION_CHANCE), ("a map", map), ("a potion recipe", POTION_RECIPE_CHANCE)]
            }
            None => Vec::new(),
        }
    }

    /// For example "Table N: 1 to 80 gold × the player's level; clothing 20%, armor 5%…".
    pub fn describe(&self) -> String {
        let gold = match self.max_gold {
            0 => "no gold".to_string(),
            max => format!("{} to {max} gold × the player's level", self.min_gold),
        };
        let items: Vec<String> = self
            .items()
            .into_iter()
            .chain(self.extras())
            .map(|(kind, chance)| format!("{kind} {chance}%"))
            .collect();
        match items.is_empty() {
            true => format!("Table {}: {gold}", self.key),
            false => format!("Table {}: {gold}; {}", self.key, items.join(", ")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dungeon_types_pick_their_tables() {
        let crypt = loot_table(DungeonType::from_name("Crypt").unwrap()).unwrap();
        assert_eq!((crypt.key, crypt.min_gold, crypt.max_gold), ('K', 1, 10));
        assert_eq!(crypt.items()[0], ("religious items", 100));
        assert!(crypt.extras().contains(&("a map", 1)));
        let den = loot_table(DungeonType::from_name("Dragon's Den").unwrap()).unwrap();
        assert_eq!(den.key, 'S');
        assert!(den.extras().is_empty(), "only tables J to O add maps and potions");
        assert!(den
            .describe()
            .starts_with("Table S: 50 to 125 gold × the player's level; other ingredients 15%"));
        // Every dungeon type has a table.
        assert!(DungeonType::all().all(|dungeon_type| loot_table(dungeon_type).is_some()));
    }
}
