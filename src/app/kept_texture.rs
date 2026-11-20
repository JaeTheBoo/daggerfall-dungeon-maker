//! egui images freed a frame after they are dropped.
//!
//! egui-wgpu 0.29 destroys a freed image before it hands the frame to the
//! GPU, so dropping an image in a frame that still draws it makes the frame
//! fail ("Texture … has been destroyed") and the program stop. This happens
//! whenever what owns an image goes in the frame it was drawn: a settlement
//! with its town map replaced by the cell double-clicked in it, or a cell
//! editor with flat thumbnails in its palette closed.
//!
//! A [`KeptTexture`] is put aside when dropped instead, and
//! [`free_put_aside`], called at the start of every frame, frees what was
//! put aside: by then no frame draws it.

use std::ops::Deref;
use std::sync::Mutex;

use eframe::egui;

/// Images dropped since the frame began, freed when the next one begins.
static PUT_ASIDE: Mutex<Vec<egui::TextureHandle>> = Mutex::new(Vec::new());

/// An egui image freed one frame after it is dropped.
pub struct KeptTexture(egui::TextureHandle);

impl KeptTexture {
    pub fn new(handle: egui::TextureHandle) -> Self {
        Self(handle)
    }
}

impl Deref for KeptTexture {
    type Target = egui::TextureHandle;

    fn deref(&self) -> &egui::TextureHandle {
        &self.0
    }
}

impl Drop for KeptTexture {
    fn drop(&mut self) {
        // The copy keeps the image until `free_put_aside` drops it.
        PUT_ASIDE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(self.0.clone());
    }
}

/// Frees the images dropped during the previous frame. Called at the start
/// of every frame, before anything is drawn.
pub fn free_put_aside() {
    let put_aside = std::mem::take(&mut *PUT_ASIDE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    drop(put_aside);
}
