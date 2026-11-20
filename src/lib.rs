//! Daggerfall Dungeon Maker: a CAD-style viewer and editor for Daggerfall dungeon rooms.
//!
//! The crate is split into a library and a thin binary so the data readers
//! can be tested independently of the user interface:
//!
//! * [`arena2`] reads the original game files (models, blocks, textures).
//! * [`scene`] turns an RDB room block into an editable list of objects.
//! * [`catalog`] lists the models, flats, markers and lights that can be placed,
//!   and describes each model from its data.
//! * [`custom_names`] holds the names users give rooms and models in the editor.
//! * [`encounters`] lists which enemies random enemy markers can become, by dungeon type.
//! * [`project`] reads and writes project files holding the user's own rooms and dungeons.
//! * [`export`] turns the project into a Daggerfall Unity mod; [`playtest`] tests a cell,
//!   dungeon or settlement in a copy of Daggerfall Unity, with the player at its entrance.
//! * [`music`] chooses a dungeon's music and plays songs to preview them.
//! * [`sound_player`] plays Daggerfall's sound effects, to hear what an action plays.
//! * [`buildings`] names settlements' buildings as Daggerfall Unity does, finds their
//!   entrances, tells when they are open and what their furniture is for, and lays
//!   out their insides; [`people`] tells who their people are (gender, children,
//!   names); [`unity_install`] reads the word lists the names are made from, in
//!   Daggerfall Unity's own folder.
//! * [`connectors`] finds the open faces where dungeon pieces join, for snapping.
//! * [`loot`] tells what a dungeon's treasure holds; [`lighting`] where the game's lights
//!   are, for drawing a place with them.
//! * [`dungeon_textures`] picks the wall and floor textures Daggerfall Unity gives a dungeon.
//! * [`doors`] finds the doors Daggerfall Unity switches off because they overlap another.
//! * [`camera`] and [`picking`] provide viewport navigation and selection.
//! * [`render`] draws a scene with wgpu into an offscreen texture.
//! * [`app`] is the egui application: menus, dialogs, panels and the viewport.

pub mod app;
pub mod arena2;
pub mod blender;
pub mod buildings;
pub mod camera;
pub mod catalog;
pub mod connectors;
pub mod custom_models;
pub mod custom_names;
pub mod doors;
pub mod dungeon_textures;
pub mod encounters;
pub mod export;
pub mod exterior;
pub mod lighting;
pub mod loot;
pub mod merge;
pub mod music;
pub mod people;
pub mod picking;
pub mod playtest;
pub mod prefabs;
pub mod project;
pub mod render;
pub mod scene;
pub mod sound_player;
pub mod unity_install;
