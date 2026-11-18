//! Custom models: the user's own 3D models, imported from Wavefront OBJ files
//! (with their MTL materials and textures) or glTF files (`.gltf` with its
//! buffers and images, or `.glb`) to place in cells like Daggerfall's.
//!
//! An imported model gets an id of its own, from [`FIRST_CUSTOM_MODEL_ID`]
//! up, far above Daggerfall's. It is converted once and kept in the
//! application's data folder as a JSON file of its geometry and a PNG for each
//! of its materials. The editor shows it through [`Arena2Library`] like any of
//! Daggerfall's models. In the game, a `.dfmod` carries those files, and its
//! build makes them a Unity prefab named after the id, which Daggerfall Unity
//! puts wherever a cell uses that model, as it does for any mod's models.
//!
//! OBJ and glTF files are right-handed with Y up, in metres. Like Unity's own
//! OBJ importer and its glTF importers, the import mirrors the model along X
//! into the game's left-handed space and turns its triangles round, so it
//! looks in the game as it did in the program it was made with.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use glam::{Affine2, Mat2, Mat3, Mat4, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::arena2::{Arena2Library, MeshVertex, ModelMesh, RgbaImage, SubMesh, TextureKey};

/// Id of the first imported model. Daggerfall's own model ids are below 100 000.
pub const FIRST_CUSTOM_MODEL_ID: u32 = 8_000_000;
/// Texture archive numbers of imported models' textures start here (one per
/// model, its materials as records); Daggerfall's archives are below 512.
const CUSTOM_TEXTURE_ARCHIVE_BASE: u16 = 0xF000;
/// How many models can have textures of their own (the archive numbers left).
const MAX_CUSTOM_TEXTURE_ARCHIVES: u32 = 0x0FFF;
/// Side of the texture made for a material that has a colour but no image, in pixels.
const COLOUR_TEXTURE_SIDE: u32 = 4;
/// Colour of a material without colour or image.
const DEFAULT_COLOUR: [f32; 3] = [0.8, 0.8, 0.8];
/// The most nodes of a glTF scene that are visited (a scene whose nodes loop
/// round would go on for ever).
const MAX_NODE_VISITS: usize = 100_000;

/// The file extensions of the models that can be imported (`.blend` files
/// through Blender, see [`crate::blender`]).
pub const MODEL_EXTENSIONS: [&str; 4] = ["obj", "gltf", "glb", "blend"];
/// The folder, in the models' folder, that Blender converts `.blend` files in.
const CONVERSIONS_FOLDER: &str = "converting";

/// Whether `model_id` is an imported model rather than one of Daggerfall's.
pub fn is_custom_model(model_id: u32) -> bool {
    model_id >= FIRST_CUSTOM_MODEL_ID
}

/// Name of the file holding the geometry of the model `id`.
pub fn model_file_name(id: u32) -> String {
    format!("{id}_model.json")
}

/// Name of the file left in place of a deleted model `id`'s, so its id is never used again.
fn deleted_marker_name(id: u32) -> String {
    format!("{id}_deleted")
}

/// Name of the image file of material `index` of the model `id`.
fn texture_file_name(id: u32, index: usize) -> String {
    format!("{id}_texture_{index}.png")
}

/// The geometry of an imported model as it is saved, and as a `.dfmod` build
/// reads it (the field names are those the build's C# script expects).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelFile {
    pub id: u32,
    pub name: String,
    /// The file it was imported from.
    pub source: String,
    pub submeshes: Vec<SubmeshFile>,
}

/// The part of an imported model drawn with one material.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubmeshFile {
    /// Its image: a PNG file next to the model's file.
    pub texture: String,
    /// The name of its material in the file it was imported from.
    #[serde(default)]
    pub material: String,
    /// Whether its image is a texture, rather than a plain image of the material's colour.
    #[serde(default)]
    pub textured: bool,
    /// x, y, z of each vertex, in the game's space (metres, left-handed, Y up).
    pub positions: Vec<f32>,
    /// x, y, z of each vertex's normal.
    pub normals: Vec<f32>,
    /// u, v of each vertex, with v going up the image as Unity has it.
    pub uvs: Vec<f32>,
    /// Three vertex indices per triangle, wound as Unity draws front faces
    /// (and as Daggerfall's models are, here).
    pub indices: Vec<u32>,
}

/// An imported model, ready to draw.
#[derive(Clone)]
pub struct CustomModel {
    pub id: u32,
    /// The name it was imported under: its file's name.
    pub name: String,
    /// The file it was imported from.
    pub source: String,
    pub mesh: Arc<ModelMesh>,
    /// The image of each material, by the texture key its sub-mesh uses.
    pub textures: Vec<(TextureKey, RgbaImage)>,
    /// Its saved files: the geometry, then the images.
    pub files: Vec<PathBuf>,
    /// Its parts, in the order of its sub-meshes and images.
    pub parts: Vec<ModelPart>,
}

/// One part of an imported model: what is drawn with one material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPart {
    /// The material's name in the file it was imported from.
    pub material: String,
    /// Whether it is drawn with a texture, rather than a plain image of its colour.
    pub textured: bool,
}

/// A model just imported, and what it names that could not be read.
pub struct ImportOutcome {
    pub model: CustomModel,
    /// The material files and textures the model names that were not found
    /// or could not be read, described for the user; its parts that needed
    /// them are drawn in their colour.
    pub missing: Vec<String>,
    /// The MTL file the model names that was not found, if one was not: the
    /// user can choose it (see [`CustomModelLibrary::set_materials`]).
    pub missing_material_file: Option<String>,
    /// When nothing is missing yet no part has a texture, though the parts
    /// have texture coordinates for one: the names of their materials, which
    /// give only a colour. (An OBJ file exported from a material whose image
    /// is not plugged straight into its colour names none.)
    pub colour_only: Vec<String>,
}

/// What a model file is converted into: its parts and their images.
struct Converted {
    submeshes: Vec<SubmeshFile>,
    images: Vec<RgbaImage>,
    /// What the file names that could not be read (see [`ImportOutcome::missing`]).
    missing: Vec<String>,
    /// The MTL file it names that was not found (see [`ImportOutcome::missing_material_file`]).
    missing_material_file: Option<String>,
}

/// Names of folders beside a model that often hold its textures.
const TEXTURE_FOLDERS: [&str; 6] = ["textures", "texture", "tex", "images", "maps", "materials"];

/// The user's imported models, kept in a folder of the application's data.
#[derive(Default)]
pub struct CustomModelLibrary {
    /// Where the models are kept; `None` if the system gives no data folder.
    folder: Option<PathBuf>,
    models: Vec<CustomModel>,
}

impl CustomModelLibrary {
    /// Reads every model kept in `folder`. Models that cannot be read are
    /// left out; the problems are returned, to tell the user.
    pub fn open(folder: PathBuf) -> (Self, Vec<String>) {
        let mut library = Self { folder: Some(folder.clone()), models: Vec::new() };
        let mut problems = Vec::new();
        let Ok(entries) = std::fs::read_dir(&folder) else { return (library, problems) };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name().is_some_and(|name| name.to_string_lossy().ends_with("_model.json"))
            })
            .collect();
        paths.sort();
        for path in paths {
            match read_model(&path) {
                Ok(model) => library.models.push(model),
                Err(error) => {
                    problems.push(format!("Could not read the custom model {}: {error}", path.display()))
                }
            }
        }
        library.models.sort_by_key(|model| model.id);
        (library, problems)
    }

    /// Every imported model, oldest first.
    pub fn models(&self) -> &[CustomModel] {
        &self.models
    }

    /// The imported model `id`, if there is one.
    pub fn get(&self, id: u32) -> Option<&CustomModel> {
        self.models.iter().find(|model| model.id == id)
    }

    /// Imports the model file at `path` (OBJ, glTF or GLB): converts it,
    /// saves it under a new id and adds it to the library. Returns the new
    /// model, and what it names that could not be read.
    pub fn import(&mut self, path: &Path) -> Result<ImportOutcome, String> {
        self.import_as(path, path)
    }

    /// Imports `converted`, a model file made from the file `source` (a
    /// `.blend` file Blender exported), named after `source`.
    pub fn import_converted(&mut self, converted: &Path, source: &Path) -> Result<ImportOutcome, String> {
        self.import_as(converted, source)
    }

    /// Deletes the imported model `id`: its saved files (not the file it was
    /// imported from) and its place in the library. Its id is never given to
    /// another model, so a project still placing it never shows another
    /// model instead. Returns the model as it was.
    pub fn delete(&mut self, id: u32) -> Result<CustomModel, String> {
        let position = self
            .models
            .iter()
            .position(|model| model.id == id)
            .ok_or_else(|| format!("there is no imported model {id}"))?;
        let folder = self.folder.clone().ok_or("This system has no folder to keep imported models in")?;
        let marker = folder.join(deleted_marker_name(id));
        std::fs::write(&marker, "")
            .map_err(|error| format!("Could not save '{}': {error}", marker.display()))?;
        for file in &self.models[position].files {
            if let Err(error) = std::fs::remove_file(file) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err(format!("Could not delete '{}': {error}", file.display()));
                }
            }
        }
        Ok(self.models.remove(position))
    }

    /// Draws part `part` of the imported model `id` (or, for `None`, every
    /// part without a texture, or every part if all have one) with the image
    /// at `image`, and saves it so. Returns the model as it is now.
    pub fn set_texture(&mut self, id: u32, part: Option<usize>, image: &Path) -> Result<CustomModel, String> {
        let folder = self.folder.clone().ok_or("This system has no folder to keep imported models in")?;
        let model_path = folder.join(model_file_name(id));
        let text = std::fs::read_to_string(&model_path)
            .map_err(|error| format!("Could not read '{}': {error}", model_path.display()))?;
        let mut file: ModelFile = serde_json::from_str(&text).map_err(|error| error.to_string())?;
        let picture = read_image(image)?;
        let parts: Vec<usize> = match part {
            Some(index) if index < file.submeshes.len() => vec![index],
            Some(index) => return Err(format!("the model has no part {}", index + 1)),
            None => {
                let untextured: Vec<usize> =
                    (0..file.submeshes.len()).filter(|&index| !file.submeshes[index].textured).collect();
                match untextured.is_empty() {
                    true => (0..file.submeshes.len()).collect(),
                    false => untextured,
                }
            }
        };
        for index in parts {
            let submesh = &mut file.submeshes[index];
            if submesh.texture.is_empty() {
                submesh.texture = texture_file_name(id, index);
            }
            save_image(&folder.join(&submesh.texture), &picture)?;
            submesh.textured = true;
        }
        let text = serde_json::to_string(&file).map_err(|error| error.to_string())?;
        std::fs::write(&model_path, text)
            .map_err(|error| format!("Could not save '{}': {error}", model_path.display()))?;
        let model = read_model(&model_path)?;
        match self.models.iter_mut().find(|known| known.id == id) {
            Some(known) => *known = model.clone(),
            None => self.models.push(model.clone()),
        }
        Ok(model)
    }

    /// The folder Blender converts `.blend` files in, if there is a folder for models.
    pub fn conversions_folder(&self) -> Option<PathBuf> {
        self.folder.as_ref().map(|folder| folder.join(CONVERSIONS_FOLDER))
    }

    /// Imports the model file at `path`, named after (and noted as imported
    /// from) the file `source`.
    fn import_as(&mut self, path: &Path, source: &Path) -> Result<ImportOutcome, String> {
        if crate::blender::is_blend_file(path) {
            return Err(format!("'{}' is a Blender file, which Blender converts first", path.display()));
        }
        let extension = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let converted = match extension.as_str() {
            "obj" => convert_obj(path, None)?,
            "gltf" | "glb" => convert_gltf(path)?,
            _ => {
                return Err(format!(
                    "'{}' is not a model this editor can import: choose an OBJ, glTF, GLB or Blender (.blend) file",
                    path.display()
                ))
            }
        };
        let folder = self.folder.clone().ok_or("This system has no folder to keep imported models in")?;
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("Could not create '{}': {error}", folder.display()))?;
        let id = self.new_id(&folder);
        if id - FIRST_CUSTOM_MODEL_ID >= MAX_CUSTOM_TEXTURE_ARCHIVES {
            return Err("No more models can be imported: the ids for their textures have run out".to_string());
        }
        let name = source
            .file_stem()
            .map_or_else(|| format!("Model {id}"), |stem| stem.to_string_lossy().into_owned());
        self.save_converted(&folder, id, name, source.display().to_string(), converted)
    }

    /// Saves the model `converted` under `id` (in place of any model saved
    /// under it before) in `folder`, and puts it in the library.
    fn save_converted(
        &mut self,
        folder: &Path,
        id: u32,
        name: String,
        source: String,
        converted: Converted,
    ) -> Result<ImportOutcome, String> {
        let Converted { submeshes, images, missing, missing_material_file } = converted;
        let mut file = ModelFile { id, name, source, submeshes };
        for (index, (submesh, image)) in file.submeshes.iter_mut().zip(&images).enumerate() {
            submesh.texture = texture_file_name(id, index);
            save_image(&folder.join(&submesh.texture), image)?;
        }
        let model_path = folder.join(model_file_name(id));
        let text = serde_json::to_string(&file).map_err(|error| error.to_string())?;
        std::fs::write(&model_path, text)
            .map_err(|error| format!("Could not save '{}': {error}", model_path.display()))?;

        let model = read_model(&model_path)?;
        match self.models.iter().position(|known| known.id == id) {
            Some(position) => {
                // Images of parts the model no longer has.
                for old in &self.models[position].files {
                    if !model.files.contains(old) {
                        let _ = std::fs::remove_file(old);
                    }
                }
                self.models[position] = model.clone();
            }
            None => self.models.push(model.clone()),
        }
        let colour_only = match missing.is_empty() && missing_material_file.is_none() {
            true => colour_only_materials(&file),
            false => Vec::new(),
        };
        Ok(ImportOutcome { model, missing, missing_material_file, colour_only })
    }

    /// Gives the imported model `id` the materials of the MTL file
    /// `materials`, as when its own was not found: the OBJ file it was
    /// imported from is read again with them, keeping its id and name. If
    /// that file is no longer there, the MTL file's materials are given to
    /// the parts by their names, or its one material to every part.
    pub fn set_materials(&mut self, id: u32, materials: &Path) -> Result<ImportOutcome, String> {
        let model = self.get(id).ok_or_else(|| format!("there is no imported model {id}"))?.clone();
        let folder = self.folder.clone().ok_or("This system has no folder to keep imported models in")?;
        let source = PathBuf::from(&model.source);
        let is_obj = source.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("obj"));
        if is_obj && source.is_file() {
            let converted = convert_obj(&source, Some(materials))?;
            return self.save_converted(&folder, id, model.name, model.source, converted);
        }
        self.set_materials_by_name(&folder, id, materials)
    }

    /// Gives the parts of the imported model `id` the textures of the
    /// materials of the MTL file `materials` with the same names (or, if it
    /// has one material, that one's), as [`Self::set_materials`] does when the
    /// model's own file is gone.
    fn set_materials_by_name(
        &mut self,
        folder: &Path,
        id: u32,
        materials: &Path,
    ) -> Result<ImportOutcome, String> {
        let (materials_found, _) = tobj::load_mtl(materials)
            .map_err(|error| format!("Could not read '{}': {error}", materials.display()))?;
        let model_path = folder.join(model_file_name(id));
        let text = std::fs::read_to_string(&model_path)
            .map_err(|error| format!("Could not read '{}': {error}", model_path.display()))?;
        let mut file: ModelFile = serde_json::from_str(&text).map_err(|error| error.to_string())?;
        let places = texture_places(materials, Path::new(&file.source));
        let mut missing = Vec::new();
        let mut matched = 0;
        for (index, submesh) in file.submeshes.iter_mut().enumerate() {
            let material = materials_found
                .iter()
                .find(|material| !submesh.material.is_empty() && material.name == submesh.material)
                .or_else(|| (materials_found.len() == 1).then(|| &materials_found[0]));
            let Some(material) = material else { continue };
            matched += 1;
            let (image, textured) = material_image(Some(material), &places, &mut missing);
            if submesh.texture.is_empty() {
                submesh.texture = texture_file_name(id, index);
            }
            save_image(&folder.join(&submesh.texture), &image)?;
            submesh.textured = textured;
        }
        if matched == 0 {
            return Err(format!(
                "none of the materials of '{}' fit the model's parts, and the file it was imported from ({}) is \
                 no longer there to read again with them",
                materials.display(),
                file.source
            ));
        }
        let text = serde_json::to_string(&file).map_err(|error| error.to_string())?;
        std::fs::write(&model_path, text)
            .map_err(|error| format!("Could not save '{}': {error}", model_path.display()))?;
        let model = read_model(&model_path)?;
        if let Some(known) = self.models.iter_mut().find(|known| known.id == id) {
            *known = model.clone();
        }
        let colour_only = if missing.is_empty() { colour_only_materials(&file) } else { Vec::new() };
        Ok(ImportOutcome { model, missing, missing_material_file: None, colour_only })
    }

    /// Lets `library` show every imported model, as it shows Daggerfall's.
    pub fn register(&self, library: &mut Arena2Library) {
        for model in &self.models {
            library.add_custom_model(model.mesh.clone(), model.textures.clone(), model.files.clone());
        }
    }

    /// An id no model in the library or file in `folder` uses yet.
    fn new_id(&self, folder: &Path) -> u32 {
        let saved =
            std::fs::read_dir(folder).into_iter().flatten().flatten().filter_map(|entry| {
                entry.file_name().to_string_lossy().split('_').next()?.parse::<u32>().ok()
            });
        let highest = self.models.iter().map(|model| model.id).chain(saved).max().unwrap_or(0);
        highest.max(FIRST_CUSTOM_MODEL_ID - 1) + 1
    }
}

/// Reads a model saved at `path` (its JSON file) and its images, next to it.
fn read_model(path: &Path) -> Result<CustomModel, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let file: ModelFile = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if !is_custom_model(file.id) || file.id - FIRST_CUSTOM_MODEL_ID >= MAX_CUSTOM_TEXTURE_ARCHIVES {
        return Err(format!("{} is not an id for an imported model", file.id));
    }
    let folder = path.parent().unwrap_or(Path::new("."));
    let mut images = Vec::with_capacity(file.submeshes.len());
    let mut files = vec![path.to_path_buf()];
    for submesh in &file.submeshes {
        let image_path = folder.join(&submesh.texture);
        let image = image::open(&image_path)
            .map_err(|error| format!("could not read '{}': {error}", image_path.display()))?
            .to_rgba8();
        images.push(RgbaImage { width: image.width(), height: image.height(), pixels: image.into_raw() });
        files.push(image_path);
    }
    let (mesh, textures) = mesh_of(&file, images)?;
    let parts = file
        .submeshes
        .iter()
        .map(|submesh| ModelPart { material: submesh.material.clone(), textured: submesh.textured })
        .collect();
    Ok(CustomModel {
        id: file.id,
        name: file.name,
        source: file.source,
        mesh: Arc::new(mesh),
        textures,
        files,
        parts,
    })
}

/// The materials of `file`'s parts that have texture coordinates (not all
/// nought) but are drawn only in a colour, if none of its parts has a texture.
fn colour_only_materials(file: &ModelFile) -> Vec<String> {
    if file.submeshes.iter().any(|submesh| submesh.textured) {
        return Vec::new();
    }
    file.submeshes
        .iter()
        .filter(|submesh| submesh.uvs.iter().any(|&value| value != 0.0))
        .map(|submesh| match submesh.material.is_empty() {
            true => "(unnamed)".to_string(),
            false => submesh.material.clone(),
        })
        .collect()
}

/// Saves `image` as a PNG file at `path`.
fn save_image(path: &Path, image: &RgbaImage) -> Result<(), String> {
    image::save_buffer(path, &image.pixels, image.width, image.height, image::ColorType::Rgba8)
        .map_err(|error| format!("Could not save '{}': {error}", path.display()))
}

/// The mesh the editor draws for `file`, whose materials' images are `images`,
/// and those images by the texture key each sub-mesh uses.
fn mesh_of(
    file: &ModelFile,
    images: Vec<RgbaImage>,
) -> Result<(ModelMesh, Vec<(TextureKey, RgbaImage)>), String> {
    let archive = CUSTOM_TEXTURE_ARCHIVE_BASE + (file.id - FIRST_CUSTOM_MODEL_ID) as u16;
    let mut submeshes = Vec::with_capacity(file.submeshes.len());
    let mut textures = Vec::with_capacity(images.len());
    let (mut minimum, mut maximum) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for (index, (submesh, image)) in file.submeshes.iter().zip(images).enumerate() {
        let record = u8::try_from(index).map_err(|_| "the model has more than 256 materials".to_string())?;
        let texture = TextureKey::new(archive, record);
        let count = submesh.positions.len() / 3;
        if submesh.normals.len() != count * 3 || submesh.uvs.len() != count * 2 {
            return Err("its vertices are incomplete".to_string());
        }
        if submesh.indices.iter().any(|&vertex| vertex as usize >= count) {
            return Err("a triangle uses a vertex it does not have".to_string());
        }
        // The editor measures texture coordinates in texels, from the top of the image.
        let size = Vec2::new(image.width as f32, image.height as f32);
        let vertices: Vec<MeshVertex> = (0..count)
            .map(|vertex| {
                let at = |values: &[f32], index: usize| values[index];
                let position = Vec3::new(
                    at(&submesh.positions, 3 * vertex),
                    at(&submesh.positions, 3 * vertex + 1),
                    at(&submesh.positions, 3 * vertex + 2),
                );
                minimum = minimum.min(position);
                maximum = maximum.max(position);
                let normal = Vec3::new(
                    at(&submesh.normals, 3 * vertex),
                    at(&submesh.normals, 3 * vertex + 1),
                    at(&submesh.normals, 3 * vertex + 2),
                );
                let uv = Vec2::new(at(&submesh.uvs, 2 * vertex), 1.0 - at(&submesh.uvs, 2 * vertex + 1));
                MeshVertex { position, normal, uv_texels: uv * size }
            })
            .collect();
        submeshes.push(SubMesh { texture, vertices, indices: submesh.indices.clone() });
        textures.push((texture, image));
    }
    if submeshes.iter().all(|submesh| submesh.indices.is_empty()) {
        return Err("it has no triangles".to_string());
    }
    Ok((ModelMesh { model_id: file.id, submeshes, bounds_min: minimum, bounds_max: maximum }, textures))
}

/// Reads the OBJ file at `path` (and its MTL materials): its parts by
/// material, in the game's space, and the image of each material.
///
/// An OBJ file holds no images: it names an MTL file, which names each
/// material's texture. These are looked for as written, then by their file
/// names (ignoring case) beside the model or in a textures folder there.
/// Those that cannot be found are noted, and the parts drawn in their colour.
///
/// With `materials_file`, that MTL file is read in place of the one the OBJ file
/// names, and textures are looked for beside it first.
fn convert_obj(path: &Path, materials_file: Option<&Path>) -> Result<Converted, String> {
    let options =
        tobj::LoadOptions { single_index: true, triangulate: true, ignore_points: true, ignore_lines: true };
    let folder = path.parent().unwrap_or(Path::new("."));
    let file =
        std::fs::File::open(path).map_err(|error| format!("Could not read '{}': {error}", path.display()))?;
    let missing_files = std::cell::RefCell::new(Vec::new());
    let missing_material_file = std::cell::RefCell::new(None);
    let (models, materials) = tobj::load_obj_buf(&mut std::io::BufReader::new(file), &options, |name| {
        if let Some(chosen) = materials_file {
            return tobj::load_mtl(chosen);
        }
        let name = name.to_string_lossy();
        match find_file(&name, folder) {
            Some(found) => tobj::load_mtl(found),
            None => {
                missing_files
                    .borrow_mut()
                    .push(format!("its material file '{name}', which is not beside it"));
                missing_material_file.borrow_mut().get_or_insert_with(|| name.to_string());
                Err(tobj::LoadError::OpenFileFailed)
            }
        }
    })
    .map_err(|error| format!("Could not read '{}': {error}", path.display()))?;
    let mut missing = missing_files.into_inner();
    let missing_material_file = missing_material_file.into_inner();
    // Textures are looked for beside the MTL file the materials come from, then beside the model.
    let places = match materials_file {
        Some(chosen) => texture_places(chosen, path),
        None => vec![folder.to_path_buf()],
    };
    // Without its MTL file, every part is drawn plain.
    let materials = match materials {
        Ok(materials) => materials,
        Err(error) => {
            if missing.is_empty() {
                missing.push(format!("its materials ({error})"));
            }
            Vec::new()
        }
    };

    // The parts that share a material are drawn as one.
    let mut parts: BTreeMap<Option<usize>, SubmeshFile> = BTreeMap::new();
    for model in &models {
        let mesh = &model.mesh;
        let count = mesh.positions.len() / 3;
        if count == 0 || mesh.indices.is_empty() {
            continue;
        }
        let material = mesh.material_id.filter(|&index| index < materials.len());
        let part = parts.entry(material).or_insert_with(|| SubmeshFile {
            material: material.map(|index| materials[index].name.clone()).unwrap_or_default(),
            ..SubmeshFile::default()
        });
        let first = (part.positions.len() / 3) as u32;
        // Mirrored along X into the game's left-handed space.
        for vertex in 0..count {
            part.positions.extend([
                -mesh.positions[3 * vertex],
                mesh.positions[3 * vertex + 1],
                mesh.positions[3 * vertex + 2],
            ]);
            let (u, v) = match mesh.texcoords.len() >= 2 * count {
                true => (mesh.texcoords[2 * vertex], mesh.texcoords[2 * vertex + 1]),
                false => (0.0, 0.0),
            };
            part.uvs.extend([u, v]);
        }
        // Mirroring turns triangles inside out; turning each round puts them right.
        let triangles: Vec<[u32; 3]> =
            mesh.indices.as_chunks::<3>().0.iter().map(|&[a, b, c]| [a, c, b]).collect();
        match mesh.normals.len() >= 3 * count {
            true => {
                for vertex in 0..count {
                    part.normals.extend([
                        -mesh.normals[3 * vertex],
                        mesh.normals[3 * vertex + 1],
                        mesh.normals[3 * vertex + 2],
                    ]);
                }
            }
            // Without normals of its own, each vertex gets the average of its triangles' facing.
            false => {
                let mut sums = vec![Vec3::ZERO; count];
                let position = |vertex: u32| {
                    let base = (first as usize + vertex as usize) * 3;
                    Vec3::new(part.positions[base], part.positions[base + 1], part.positions[base + 2])
                };
                for triangle in &triangles {
                    let [a, b, c] = triangle.map(position);
                    let facing = (b - a).cross(c - a);
                    for &vertex in triangle {
                        sums[vertex as usize] += facing;
                    }
                }
                for sum in sums {
                    part.normals.extend(sum.normalize_or(Vec3::Y).to_array());
                }
            }
        }
        part.indices.extend(triangles.iter().flatten().map(|&vertex| first + vertex));
    }
    if parts.is_empty() {
        return Err(format!("'{}' has no triangles to import", path.display()));
    }

    let mut images = Vec::with_capacity(parts.len());
    for (&material, part) in parts.iter_mut() {
        let material = material.map(|index| &materials[index]);
        let (image, textured) = material_image(material, &places, &mut missing);
        part.textured = textured;
        images.push(image);
    }
    Ok(Converted { submeshes: parts.into_values().collect(), images, missing, missing_material_file })
}

/// The folders textures named in the MTL file `materials` are looked for in:
/// its own, then that of the model file `model`.
fn texture_places(materials: &Path, model: &Path) -> Vec<PathBuf> {
    let mut places: Vec<PathBuf> = materials.parent().map(Path::to_path_buf).into_iter().collect();
    if let Some(folder) = model.parent().filter(|folder| !places.iter().any(|place| place == folder)) {
        places.push(folder.to_path_buf());
    }
    places
}

/// The image of `material` (whose files are found from `folder`): its
/// texture, and `true`; or, without one (noting in `missing` a texture that
/// cannot be read), a plain image of its colour, and `false`.
fn material_image(
    material: Option<&tobj::Material>,
    folders: &[PathBuf],
    missing: &mut Vec<String>,
) -> (RgbaImage, bool) {
    if let Some(statement) = material.and_then(|material| material.diffuse_texture.as_deref()) {
        match folders.iter().find_map(|folder| find_texture(statement, folder)) {
            Some(path) => match read_image(&path) {
                Ok(image) => return (image, true),
                Err(error) => missing.push(error),
            },
            None => {
                missing.push(format!("the texture '{}', which is not beside it", texture_name(statement)))
            }
        }
    }
    (colour_image(material.and_then(|material| material.diffuse).unwrap_or(DEFAULT_COLOUR)), false)
}

/// Reads the texture image at `path`.
fn read_image(path: &Path) -> Result<RgbaImage, String> {
    let image = image::open(path)
        .map_err(|error| format!("Could not read the texture '{}': {error}", path.display()))?
        .to_rgba8();
    Ok(RgbaImage { width: image.width(), height: image.height(), pixels: image.into_raw() })
}

/// A plain image of `colour` (red, green and blue from 0 to 1, as an image has them).
fn colour_image(colour: [f32; 3]) -> RgbaImage {
    let pixel = colour.map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8);
    let pixels = (0..COLOUR_TEXTURE_SIDE * COLOUR_TEXTURE_SIDE)
        .flat_map(|_| [pixel[0], pixel[1], pixel[2], 255])
        .collect();
    RgbaImage { width: COLOUR_TEXTURE_SIDE, height: COLOUR_TEXTURE_SIDE, pixels }
}

/// Finds the file a model names as `name` (an MTL file, or a texture): as
/// written (from the model's `folder`), or else by its file name, ignoring
/// case, beside the model, in the folder the name gives, or in a textures
/// folder beside the model, as models made on another computer often need.
fn find_file(name: &str, folder: &Path) -> Option<PathBuf> {
    let name = name.trim().trim_matches('"').replace('\\', "/");
    if name.is_empty() {
        return None;
    }
    let written = folder.join(&name);
    if written.is_file() {
        return Some(written);
    }
    let named = Path::new(&name);
    let file_name = named.file_name()?.to_string_lossy().to_lowercase();
    let mut places = vec![folder.to_path_buf()];
    if let Some(parent) = named.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        places.push(folder.join(parent));
    }
    let subfolders = std::fs::read_dir(folder).into_iter().flatten().flatten().filter(|entry| {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        entry.path().is_dir() && TEXTURE_FOLDERS.contains(&name.as_str())
    });
    places.extend(subfolders.map(|entry| entry.path()));
    places.iter().find_map(|place| {
        std::fs::read_dir(place).ok()?.flatten().map(|entry| entry.path()).find(|path| {
            path.is_file()
                && path.file_name().is_some_and(|found| found.to_string_lossy().to_lowercase() == file_name)
        })
    })
}

/// Finds the texture an MTL statement names (`map_Kd [options] file`): the
/// file, skipping any options written before it.
fn find_texture(statement: &str, folder: &Path) -> Option<PathBuf> {
    let words: Vec<&str> = statement.split_whitespace().collect();
    (0..words.len()).find_map(|first| find_file(&words[first..].join(" "), folder))
}

/// The file name an MTL statement names, without the options before it, to show the user.
fn texture_name(statement: &str) -> String {
    let words: Vec<&str> = statement.split_whitespace().collect();
    let is_option_word =
        |word: &&str| word.starts_with('-') || word.parse::<f32>().is_ok() || matches!(*word, "on" | "off");
    let first = words.iter().position(|word| !is_option_word(word)).unwrap_or(0);
    words[first..].join(" ")
}

/// Reads the glTF file at `path` (a `.gltf` with its buffers and images, or a
/// `.glb`): the triangles of its scene by material, in the game's space, and
/// the image of each material.
fn convert_gltf(path: &Path) -> Result<Converted, String> {
    let cannot_read = |error: String| format!("Could not read '{}': {error}", path.display());
    let bytes = std::fs::read(path).map_err(|error| cannot_read(error.to_string()))?;
    let gltf::Gltf { document, blob } = gltf::Gltf::from_slice(&bytes).map_err(|error| {
        let unreadable = unreadable_extensions(&bytes);
        let hint = match unreadable.iter().any(|name| name == "KHR_draco_mesh_compression") {
            true => " (with Draco mesh compression turned off)",
            false => "",
        };
        match unreadable.is_empty() {
            true => cannot_read(error.to_string()),
            false => format!(
                "'{}' needs glTF extensions this editor cannot read ({}); export it again without them{hint}",
                path.display(),
                unreadable.join(", ")
            ),
        }
    })?;
    let folder = path.parent().unwrap_or(Path::new("."));
    let buffers = gltf::import_buffers(&document, Some(folder), blob)
        .map_err(|error| cannot_read(format!("its geometry: {error}")))?;

    // Every mesh where the scene puts it: a node is placed by its parents' transforms and its own.
    let mut placed: Vec<(gltf::Mesh, Mat4)> = Vec::new();
    match document.default_scene().or_else(|| document.scenes().next()) {
        Some(scene) => {
            let mut nodes: Vec<(gltf::Node, Mat4)> =
                scene.nodes().map(|node| (node, Mat4::IDENTITY)).collect();
            let mut visits = 0;
            while let Some((node, parent)) = nodes.pop() {
                visits += 1;
                if visits > MAX_NODE_VISITS {
                    return Err(cannot_read("its nodes loop round".to_string()));
                }
                let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
                if let Some(mesh) = node.mesh() {
                    placed.push((mesh, transform));
                }
                nodes.extend(node.children().map(|child| (child, transform)));
            }
        }
        // A file without a scene has its meshes as they are.
        None => placed.extend(document.meshes().map(|mesh| (mesh, Mat4::IDENTITY))),
    }

    // The parts that share a material are drawn as one.
    let mut parts: BTreeMap<Option<usize>, SubmeshFile> = BTreeMap::new();
    for (mesh, transform) in &placed {
        for primitive in mesh.primitives() {
            add_primitive(&primitive, *transform, &buffers, &mut parts)?;
        }
    }
    if parts.values().all(|part| part.indices.is_empty()) {
        return Err(format!("'{}' has no triangles to import", path.display()));
    }

    let materials: Vec<gltf::Material> = document.materials().collect();
    let mut images = Vec::with_capacity(parts.len());
    let mut missing = Vec::new();
    for (&material, part) in parts.iter_mut() {
        let material = material.and_then(|index| materials.get(index));
        part.material = material
            .map(|material| match material.name() {
                Some(name) => name.to_string(),
                None => format!("Material {}", material.index().unwrap_or(0) + 1),
            })
            .unwrap_or_default();
        let (image, textured) = gltf_material_image(material, folder, &buffers, &mut missing);
        part.textured = textured;
        images.push(image);
    }
    Ok(Converted { submeshes: parts.into_values().collect(), images, missing, missing_material_file: None })
}

/// The extensions the glTF file `bytes` needs that this editor cannot read.
fn unreadable_extensions(bytes: &[u8]) -> Vec<String> {
    let Ok(gltf) = gltf::Gltf::from_slice_without_validation(bytes) else { return Vec::new() };
    gltf.document
        .extensions_required()
        .filter(|name| !gltf::json::extensions::ENABLED_EXTENSIONS.contains(name))
        .map(str::to_string)
        .collect()
}

/// Adds the triangles of `primitive`, placed by `transform`, to the part of its material.
fn add_primitive(
    primitive: &gltf::Primitive,
    transform: Mat4,
    buffers: &[gltf::buffer::Data],
    parts: &mut BTreeMap<Option<usize>, SubmeshFile>,
) -> Result<(), String> {
    use gltf::mesh::Mode;
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| &data[..]));
    let Some(positions) = reader.read_positions() else { return Ok(()) };
    let positions: Vec<Vec3> = positions.map(Vec3::from).collect();
    let count = positions.len() as u32;
    let indices: Vec<u32> = match reader.read_indices() {
        Some(indices) => indices.into_u32().collect(),
        None => (0..count).collect(),
    };
    if indices.iter().any(|&vertex| vertex >= count) {
        return Err("a triangle of the model uses a vertex it does not have".to_string());
    }
    let mut triangles: Vec<[u32; 3]> = match primitive.mode() {
        Mode::Triangles => indices.as_chunks::<3>().0.to_vec(),
        // Every other triangle of a strip is wound the other way round.
        Mode::TriangleStrip => indices
            .windows(3)
            .enumerate()
            .map(|(number, window)| match number % 2 {
                0 => [window[0], window[1], window[2]],
                _ => [window[0], window[2], window[1]],
            })
            .collect(),
        Mode::TriangleFan => {
            indices.windows(2).skip(1).map(|window| [window[0], window[1], indices[0]]).collect()
        }
        // Points and lines are not drawn.
        _ => return Ok(()),
    };
    if triangles.is_empty() {
        return Ok(());
    }

    // Mirrored along X into the game's left-handed space, which turns
    // triangles inside out, unless the node mirrors them already.
    let mirror = Vec3::new(-1.0, 1.0, 1.0);
    let linear = Mat3::from_mat4(transform);
    if linear.determinant() > 0.0 {
        for triangle in &mut triangles {
            triangle.swap(1, 2);
        }
    }
    let positions: Vec<Vec3> =
        positions.iter().map(|&position| transform.transform_point3(position) * mirror).collect();
    let turn_normal = linear.inverse().transpose();
    let normals: Option<Vec<Vec3>> = reader
        .read_normals()
        .map(|normals| {
            normals.map(|normal| (turn_normal * Vec3::from(normal) * mirror).normalize_or(Vec3::Y)).collect()
        })
        .filter(|normals: &Vec<Vec3>| normals.len() == positions.len());

    let (_, texture) = base_colour(&primitive.material());
    let (set, uv_transform) = texture.as_ref().map_or((0, Affine2::IDENTITY), uv_transform);
    let uvs: Vec<Vec2> =
        reader.read_tex_coords(set).map(|uvs| uvs.into_f32().map(Vec2::from).collect()).unwrap_or_default();
    // glTF measures v down the image; the saved model has it going up.
    let uv = |vertex: u32| {
        let uv = uv_transform.transform_point2(uvs.get(vertex as usize).copied().unwrap_or(Vec2::ZERO));
        [uv.x, 1.0 - uv.y]
    };

    let part = parts.entry(primitive.material().index()).or_default();
    match normals {
        Some(normals) => {
            let first = (part.positions.len() / 3) as u32;
            for vertex in 0..count {
                part.positions.extend(positions[vertex as usize].to_array());
                part.normals.extend(normals[vertex as usize].to_array());
                part.uvs.extend(uv(vertex));
            }
            part.indices.extend(triangles.iter().flatten().map(|&vertex| first + vertex));
        }
        // Without normals of its own, each triangle is drawn flat, as glTF has it.
        None => {
            for triangle in &triangles {
                let [a, b, c] = triangle.map(|vertex| positions[vertex as usize]);
                let normal = (b - a).cross(c - a).normalize_or(Vec3::Y);
                for &vertex in triangle {
                    part.indices.push((part.positions.len() / 3) as u32);
                    part.positions.extend(positions[vertex as usize].to_array());
                    part.normals.extend(normal.to_array());
                    part.uvs.extend(uv(vertex));
                }
            }
        }
    }
    Ok(())
}

/// The colour of `material` (linear, with alpha) and the texture it multiplies,
/// if any: its base colour, or the diffuse colour of an older
/// specular-glossiness material.
fn base_colour<'a>(material: &gltf::Material<'a>) -> ([f32; 4], Option<gltf::texture::Info<'a>>) {
    let base = material.pbr_metallic_roughness();
    match (base.base_color_texture(), material.pbr_specular_glossiness()) {
        (None, Some(older)) => (older.diffuse_factor(), older.diffuse_texture()),
        (texture, _) => (base.base_color_factor(), texture),
    }
}

/// The texture coordinate set the texture `info` uses, and how its texture
/// transform moves those coordinates.
fn uv_transform(info: &gltf::texture::Info) -> (u32, Affine2) {
    let Some(transform) = info.texture_transform() else { return (info.tex_coord(), Affine2::IDENTITY) };
    let (sin, cos) = transform.rotation().sin_cos();
    let ([scale_u, scale_v], offset) = (transform.scale(), Vec2::from(transform.offset()));
    let turn =
        Mat2::from_cols(Vec2::new(cos * scale_u, -sin * scale_u), Vec2::new(sin * scale_v, cos * scale_v));
    (transform.tex_coord().unwrap_or(info.tex_coord()), Affine2::from_mat2_translation(turn, offset))
}

/// The image of the glTF `material` (`None` for the default one): its
/// texture, tinted by its colour, and `true`; or, without one (noting in
/// `missing` a texture that cannot be read), a plain image of its colour, and `false`.
fn gltf_material_image(
    material: Option<&gltf::Material>,
    folder: &Path,
    buffers: &[gltf::buffer::Data],
    missing: &mut Vec<String>,
) -> (RgbaImage, bool) {
    let Some(material) = material else { return (colour_image(DEFAULT_COLOUR), false) };
    let (colour, texture) = base_colour(material);
    let plain = colour_image([colour[0], colour[1], colour[2]].map(to_srgb));
    let Some(info) = texture else { return (plain, false) };
    match gltf_image(&info.texture().source(), folder, buffers) {
        Ok(mut image) => {
            if colour != [1.0; 4] {
                tint(&mut image, colour);
            }
            (image, true)
        }
        Err(problem) => {
            missing.push(problem);
            (plain, false)
        }
    }
}

/// The pixels of a glTF image: a file next to the model, or data in it.
fn gltf_image(
    image: &gltf::Image,
    folder: &Path,
    buffers: &[gltf::buffer::Data],
) -> Result<RgbaImage, String> {
    if let gltf::image::Source::Uri { uri, .. } = image.source() {
        if !uri.starts_with("data:") {
            let name = percent_decoded(uri);
            let path = find_file(&name, folder)
                .ok_or_else(|| format!("the texture '{name}', which is not beside it"))?;
            return read_image(&path);
        }
    }
    let data = gltf::image::Data::from_source(image.source(), Some(folder), buffers)
        .map_err(|error| format!("a texture in the model, which could not be read ({error})"))?;
    rgba_of(data)
}

/// `uri` with its `%xx` escapes turned back into the characters they stand for.
fn percent_decoded(uri: &str) -> String {
    let bytes = uri.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .filter(|hex| hex.iter().all(u8::is_ascii_hexdigit))
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                index += 3;
            }
            None => {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// A glTF image decoded by the glTF reader, as red, green, blue and alpha bytes.
fn rgba_of(data: gltf::image::Data) -> Result<RgbaImage, String> {
    use gltf::image::Format;
    use image::{DynamicImage, ImageBuffer};
    let (width, height) = (data.width, data.height);
    let words = |bytes: &[u8]| -> Vec<u16> {
        bytes.as_chunks::<2>().0.iter().map(|&word| u16::from_ne_bytes(word)).collect()
    };
    let floats = |bytes: &[u8]| -> Vec<f32> {
        bytes.as_chunks::<4>().0.iter().map(|&float| f32::from_ne_bytes(float)).collect()
    };
    let image = match data.format {
        Format::R8 => ImageBuffer::from_raw(width, height, data.pixels).map(DynamicImage::ImageLuma8),
        Format::R8G8 => ImageBuffer::from_raw(width, height, data.pixels).map(DynamicImage::ImageLumaA8),
        Format::R8G8B8 => ImageBuffer::from_raw(width, height, data.pixels).map(DynamicImage::ImageRgb8),
        Format::R8G8B8A8 => ImageBuffer::from_raw(width, height, data.pixels).map(DynamicImage::ImageRgba8),
        Format::R16 => {
            ImageBuffer::from_raw(width, height, words(&data.pixels)).map(DynamicImage::ImageLuma16)
        }
        Format::R16G16 => {
            ImageBuffer::from_raw(width, height, words(&data.pixels)).map(DynamicImage::ImageLumaA16)
        }
        Format::R16G16B16 => {
            ImageBuffer::from_raw(width, height, words(&data.pixels)).map(DynamicImage::ImageRgb16)
        }
        Format::R16G16B16A16 => {
            ImageBuffer::from_raw(width, height, words(&data.pixels)).map(DynamicImage::ImageRgba16)
        }
        Format::R32G32B32FLOAT => {
            ImageBuffer::from_raw(width, height, floats(&data.pixels)).map(DynamicImage::ImageRgb32F)
        }
        Format::R32G32B32A32FLOAT => {
            ImageBuffer::from_raw(width, height, floats(&data.pixels)).map(DynamicImage::ImageRgba32F)
        }
    };
    let image = image.ok_or("a texture in the model is incomplete")?.to_rgba8();
    Ok(RgbaImage { width: image.width(), height: image.height(), pixels: image.into_raw() })
}

/// Multiplies the colours of `image` (sRGB) by `colour` (linear, with alpha), as glTF does.
fn tint(image: &mut RgbaImage, colour: [f32; 4]) {
    let channel_tables: [[u8; 256]; 3] = std::array::from_fn(|channel| {
        std::array::from_fn(|value| {
            (to_srgb(to_linear(value as f32 / 255.0) * colour[channel]) * 255.0).round() as u8
        })
    });
    let alpha = colour[3].clamp(0.0, 1.0);
    for pixel in image.pixels.as_chunks_mut::<4>().0 {
        for (value, table) in pixel.iter_mut().zip(&channel_tables) {
            *value = table[*value as usize];
        }
        pixel[3] = (f32::from(pixel[3]) * alpha).round() as u8;
    }
}

/// The sRGB value (as images have it) of the linear colour value `value`.
fn to_srgb(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    match value <= 0.003_130_8 {
        true => value * 12.92,
        false => 1.055 * value.powf(1.0 / 2.4) - 0.055,
    }
}

/// The linear colour value of the sRGB value `value`.
fn to_linear(value: f32) -> f32 {
    match value <= 0.040_45 {
        true => value / 12.92,
        false => ((value + 0.055) / 1.055).powf(2.4),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own under the system's temporary folder, emptied first.
    fn test_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("ddm_custom_models_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// Writes a one metre square in the XY plane facing +Z (towards the
    /// viewer in OBJ's right-handed space), textured, and a coloured triangle.
    fn write_sample(folder: &Path) -> PathBuf {
        let texture = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
        texture.save(folder.join("crate.png")).unwrap();
        std::fs::write(
            folder.join("crate.mtl"),
            "newmtl Wood\nKd 1 1 1\nmap_Kd C:\\Elsewhere\\crate.png\nnewmtl Red\nKd 1 0 0\n",
        )
        .unwrap();
        let obj = "mtllib crate.mtl\n\
                   v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 0 1 1\n\
                   vt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\n\
                   vn 0 0 1\n\
                   o Square\nusemtl Wood\nf 1/1/1 2/2/1 3/3/1 4/4/1\n\
                   o Triangle\nusemtl Red\nf 5 6 7\n";
        let path = folder.join("crate.obj");
        std::fs::write(&path, obj).unwrap();
        path
    }

    #[test]
    fn imports_an_obj_model_into_the_games_space() {
        let folder = test_folder("import");
        let source = write_sample(&folder);
        let store = folder.join("store");
        let (mut library, problems) = CustomModelLibrary::open(store.clone());
        assert!(problems.is_empty() && library.models().is_empty());

        let outcome = library.import(&source).unwrap();
        assert!(outcome.missing.is_empty(), "{:?}", outcome.missing);
        let model = outcome.model;
        assert_eq!((model.id, model.name.as_str()), (FIRST_CUSTOM_MODEL_ID, "crate"));
        let mesh = &model.mesh;
        assert_eq!(mesh.submeshes.len(), 2);
        // Mirrored along X: the square spans -1..0.
        assert_eq!((mesh.bounds_min, mesh.bounds_max), (Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 1.0)));
        // Its triangles still face the way their normals do, as Daggerfall's models' do.
        let square = &mesh.submeshes[0];
        assert_eq!(square.indices.len(), 6);
        for triangle in square.indices.chunks(3) {
            let [a, b, c] =
                [triangle[0], triangle[1], triangle[2]].map(|index| square.vertices[index as usize]);
            let facing = (b.position - a.position).cross(c.position - a.position);
            assert!(facing.dot(a.normal) > 0.0);
            assert_eq!(a.normal, Vec3::Z);
        }
        // Texture coordinates in texels from the top of the 2 x 2 texture, found by its file name alone.
        assert!(square.vertices.iter().any(|vertex| vertex.uv_texels == Vec2::new(0.0, 2.0)));
        assert_eq!(model.textures[0].1.pixels[..4], [10, 20, 30, 255]);
        // The triangle without normals or texture: its normal from its facing, its texture its colour.
        let triangle = &mesh.submeshes[1];
        assert!(triangle.vertices.iter().all(|vertex| vertex.normal.z.abs() > 0.99));
        assert_eq!(model.textures[1].1.pixels[..4], [255, 0, 0, 255]);
        assert_eq!(model.files.len(), 3);

        // Saved: opening the folder again reads it back, and the next import gets the next id.
        let (mut reopened, problems) = CustomModelLibrary::open(store);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(reopened.models().len(), 1);
        assert_eq!(reopened.models()[0].mesh.submeshes[0].vertices.len(), square.vertices.len());
        assert_eq!(reopened.import(&source).unwrap().model.id, FIRST_CUSTOM_MODEL_ID + 1);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The binary buffer of a glTF test model, with its buffer views and accessors.
    #[derive(Default)]
    struct GltfBuffer {
        bytes: Vec<u8>,
        views: Vec<serde_json::Value>,
        accessors: Vec<serde_json::Value>,
    }

    impl GltfBuffer {
        /// Adds `data` as a buffer view; returns its index.
        fn view(&mut self, data: &[u8]) -> usize {
            while !self.bytes.len().is_multiple_of(4) {
                self.bytes.push(0);
            }
            let offset = self.bytes.len();
            self.bytes.extend_from_slice(data);
            self.views.push(serde_json::json!({"buffer": 0, "byteOffset": offset, "byteLength": data.len()}));
            self.views.len() - 1
        }

        /// Adds an accessor of `values` (`kind` "VEC2" or "VEC3"); returns its index.
        fn floats(&mut self, values: &[f32], kind: &str) -> usize {
            let width = if kind == "VEC3" { 3 } else { 2 };
            let bytes: Vec<u8> = values.iter().flat_map(|value| value.to_le_bytes()).collect();
            let view = self.view(&bytes);
            let column = |index: usize| values.iter().skip(index).step_by(width).copied();
            let minimum: Vec<f32> = (0..width).map(|index| column(index).fold(f32::MAX, f32::min)).collect();
            let maximum: Vec<f32> = (0..width).map(|index| column(index).fold(f32::MIN, f32::max)).collect();
            self.accessors.push(serde_json::json!({
                "bufferView": view, "componentType": 5126, "count": values.len() / width,
                "type": kind, "min": minimum, "max": maximum,
            }));
            self.accessors.len() - 1
        }

        /// Adds an accessor of vertex indices; returns its index.
        fn indices(&mut self, values: &[u16]) -> usize {
            let bytes: Vec<u8> = values.iter().flat_map(|value| value.to_le_bytes()).collect();
            let view = self.view(&bytes);
            self.accessors.push(serde_json::json!({
                "bufferView": view, "componentType": 5123, "count": values.len(), "type": "SCALAR",
            }));
            self.accessors.len() - 1
        }

        /// Its bytes, padded to whole words as a GLB file needs them.
        fn padded(&self) -> Vec<u8> {
            let mut bytes = self.bytes.clone();
            while !bytes.len().is_multiple_of(4) {
                bytes.push(0);
            }
            bytes
        }
    }

    /// A 2 x 2 image of `pixel`, as PNG bytes.
    fn png(pixel: [u8; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(2, 2, image::Rgba(pixel))
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    /// The GLB file of `json` and its binary buffer `bin`.
    fn glb(json: &serde_json::Value, bin: &[u8]) -> Vec<u8> {
        let mut json = serde_json::to_vec(json).unwrap();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let length = 12 + 8 + json.len() + 8 + bin.len();
        let mut file = b"glTF".to_vec();
        file.extend(2u32.to_le_bytes());
        file.extend((length as u32).to_le_bytes());
        file.extend((json.len() as u32).to_le_bytes());
        file.extend(b"JSON");
        file.extend(json);
        file.extend((bin.len() as u32).to_le_bytes());
        file.extend(b"BIN\0");
        file.extend(bin);
        file
    }

    /// Checks that each triangle of `submesh` faces the way its normals do, as
    /// Daggerfall's models' triangles do.
    fn assert_faces_its_normals(submesh: &SubMesh) {
        for triangle in submesh.indices.chunks(3) {
            let [a, b, c] =
                [triangle[0], triangle[1], triangle[2]].map(|index| submesh.vertices[index as usize]);
            let facing = (b.position - a.position).cross(c.position - a.position);
            assert!(facing.dot(a.normal) > 0.0, "a triangle faces away from its normal");
        }
    }

    #[test]
    fn imports_a_gltf_model_with_its_files_next_to_it() {
        let folder = test_folder("gltf");
        // A one metre square in the XY plane facing +Z, lifted 2 m by its node;
        // glTF's v goes down the image, so the bottom corners have v = 1.
        let mut buffer = GltfBuffer::default();
        let positions = buffer.floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0], "VEC3");
        let normals = buffer.floats(&[0.0, 0.0, 1.0].repeat(4), "VEC3");
        let uvs = buffer.floats(&[0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0], "VEC2");
        let indices = buffer.indices(&[0, 1, 2, 0, 2, 3]);
        std::fs::write(folder.join("crate.bin"), &buffer.bytes).unwrap();
        std::fs::write(folder.join("my crate.png"), png([10, 20, 30, 255])).unwrap();
        let json = serde_json::json!({
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0, "translation": [0.0, 2.0, 0.0]}],
            "meshes": [{"primitives": [{
                "attributes": {"POSITION": positions, "NORMAL": normals, "TEXCOORD_0": uvs},
                "indices": indices, "material": 0,
            }]}],
            "materials": [{"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}}],
            "textures": [{"source": 0}],
            "images": [{"uri": "my%20crate.png"}],
            "buffers": [{"uri": "crate.bin", "byteLength": buffer.bytes.len()}],
            "bufferViews": buffer.views,
            "accessors": buffer.accessors,
        });
        let source = folder.join("crate.gltf");
        std::fs::write(&source, serde_json::to_string(&json).unwrap()).unwrap();

        let (mut library, _) = CustomModelLibrary::open(folder.join("store"));
        let outcome = library.import(&source).unwrap();
        assert!(outcome.missing.is_empty(), "{:?}", outcome.missing);
        let model = outcome.model;
        assert_eq!(model.name, "crate");
        let mesh = &model.mesh;
        // Mirrored along X, and where its node puts it.
        assert_eq!((mesh.bounds_min, mesh.bounds_max), (Vec3::new(-1.0, 2.0, 0.0), Vec3::new(0.0, 3.0, 0.0)));
        let square = &mesh.submeshes[0];
        assert_eq!(square.indices.len(), 6);
        assert!(square.vertices.iter().all(|vertex| vertex.normal == Vec3::Z));
        assert_faces_its_normals(square);
        // The bottom-left corner is at the bottom of the 2 x 2 image, found under its decoded name.
        let corner =
            square.vertices.iter().find(|vertex| vertex.position == Vec3::new(0.0, 2.0, 0.0)).unwrap();
        assert_eq!(corner.uv_texels, Vec2::new(0.0, 2.0));
        assert_eq!(model.textures[0].1.pixels[..4], [10, 20, 30, 255]);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn imports_a_glb_model_with_everything_in_it() {
        let folder = test_folder("glb");
        let mut buffer = GltfBuffer::default();
        // A triangle without normals, facing +Z once its node mirrors it.
        let triangle = buffer.floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], "VEC3");
        // A square as a strip of two triangles facing +Z, textured by an image in the file.
        let strip = buffer.floats(&[0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0], "VEC3");
        let strip_normals = buffer.floats(&[0.0, 0.0, 1.0].repeat(4), "VEC3");
        let strip_uvs = buffer.floats(&[0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0], "VEC2");
        let image = buffer.view(&png([200, 100, 50, 255]));
        let bin = buffer.padded();
        let json = serde_json::json!({
            "asset": {"version": "2.0"},
            "scenes": [{"nodes": [0, 1]}],
            "nodes": [{"mesh": 0, "scale": [-1.0, 1.0, 1.0]}, {"mesh": 1}],
            "meshes": [
                {"primitives": [{"attributes": {"POSITION": triangle}, "material": 0}]},
                {"primitives": [{
                    "attributes": {"POSITION": strip, "NORMAL": strip_normals, "TEXCOORD_0": strip_uvs},
                    "mode": 5, "material": 1,
                }]},
            ],
            "materials": [
                {"pbrMetallicRoughness": {"baseColorFactor": [1.0, 0.0, 0.0, 1.0]}},
                {"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}},
            ],
            "textures": [{"source": 0}],
            "images": [{"bufferView": image, "mimeType": "image/png"}],
            "buffers": [{"byteLength": bin.len()}],
            "bufferViews": buffer.views,
            "accessors": buffer.accessors,
        });
        let source = folder.join("set.glb");
        std::fs::write(&source, glb(&json, &bin)).unwrap();

        let (mut library, _) = CustomModelLibrary::open(folder.join("store"));
        let outcome = library.import(&source).unwrap();
        assert!(outcome.missing.is_empty(), "{:?}", outcome.missing);
        let model = outcome.model;
        let mesh = &model.mesh;
        assert_eq!(mesh.submeshes.len(), 2);
        let red = &mesh.submeshes[0];
        assert_eq!(red.indices.len(), 3);
        // Mirrored twice, by its node and into the game's space: as it was.
        assert!(red.vertices.iter().any(|vertex| vertex.position == Vec3::X));
        assert!(red.vertices.iter().all(|vertex| vertex.normal.z > 0.99));
        assert_faces_its_normals(red);
        assert_eq!(model.textures[0].1.pixels[..4], [255, 0, 0, 255]);
        let square = &mesh.submeshes[1];
        assert_eq!(square.indices.len(), 6);
        assert_faces_its_normals(square);
        assert_eq!(model.textures[1].1.pixels[..4], [200, 100, 50, 255]);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn names_the_gltf_extensions_it_cannot_read() {
        let folder = test_folder("draco");
        let json = serde_json::json!({
            "asset": {"version": "2.0"},
            "extensionsUsed": ["KHR_draco_mesh_compression"],
            "extensionsRequired": ["KHR_draco_mesh_compression"],
            "meshes": [],
        });
        let source = folder.join("packed.gltf");
        std::fs::write(&source, serde_json::to_string(&json).unwrap()).unwrap();
        let (mut library, _) = CustomModelLibrary::open(folder.join("store"));
        let error = library.import(&source).err().unwrap();
        assert!(error.contains("KHR_draco_mesh_compression"), "{error}");
        let wrong = folder.join("model.fbx");
        std::fs::write(&wrong, "").unwrap();
        assert!(library.import(&wrong).err().unwrap().contains("OBJ, glTF, GLB or Blender"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn refuses_files_without_triangles() {
        let folder = test_folder("empty");
        let path = folder.join("empty.obj");
        std::fs::write(&path, "v 0 0 0\nv 1 0 0\n").unwrap();
        let (mut library, _) = CustomModelLibrary::open(folder.join("store"));
        assert!(library.import(&path).is_err());
        assert!(library.models().is_empty());
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// A one metre textured square, with texture coordinates, using material `Skin` from `rat.mtl`.
    const SQUARE_OBJ: &str = "mtllib rat.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\n\
                              vt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nusemtl Skin\nf 1/1 2/2 3/3 4/4\n";

    #[test]
    fn imports_a_model_whose_material_file_is_missing_and_takes_its_texture_later() {
        let folder = test_folder("no_mtl");
        let source = folder.join("Rat.obj");
        std::fs::write(&source, SQUARE_OBJ).unwrap();
        let store = folder.join("store");
        let (mut library, _) = CustomModelLibrary::open(store.clone());

        // Imported in its colour, saying what is missing.
        let outcome = library.import(&source).unwrap();
        assert_eq!(outcome.missing.len(), 1);
        assert!(outcome.missing[0].contains("rat.mtl"), "{:?}", outcome.missing);
        assert_eq!(outcome.missing_material_file.as_deref(), Some("rat.mtl"));
        let id = outcome.model.id;
        assert_eq!(outcome.model.parts, [ModelPart { material: String::new(), textured: false }]);

        // Its texture chosen afterwards, kept when the library is opened again.
        let image = folder.join("fur.png");
        std::fs::write(&image, png([90, 60, 40, 255])).unwrap();
        let model = library.set_texture(id, None, &image).unwrap();
        assert!(model.parts[0].textured);
        assert_eq!(
            (model.textures[0].1.width, model.textures[0].1.pixels[..4].to_vec()),
            (2, vec![90, 60, 40, 255])
        );
        assert!(library.get(id).unwrap().parts[0].textured);
        let (reopened, problems) = CustomModelLibrary::open(store);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(reopened.get(id).unwrap().parts[0].textured);
        assert!(library.set_texture(id, Some(3), &image).is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn deletes_a_model_and_never_gives_its_id_again() {
        let folder = test_folder("delete");
        let source = write_sample(&folder);
        let store = folder.join("store");
        let (mut library, _) = CustomModelLibrary::open(store.clone());
        let first = library.import(&source).unwrap().model.id;
        let second = library.import(&source).unwrap().model.id;

        let deleted = library.delete(second).unwrap();
        assert!(deleted.files.iter().all(|file| !file.exists()));
        assert!(library.get(second).is_none() && library.get(first).is_some());
        assert!(library.delete(second).is_err());
        // The source is left alone, and the next model gets a new id, even after reopening.
        assert!(source.exists());
        let (mut reopened, problems) = CustomModelLibrary::open(store);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(reopened.models().len(), 1);
        assert_eq!(reopened.import(&source).unwrap().model.id, second + 1);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// A square of material `Skin` and a triangle of material `Eyes`, from `rat.mtl`.
    const TWO_MATERIAL_OBJ: &str = "mtllib rat.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\n\
                                    v 0 1 1\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nusemtl Skin\nf 1/1 2/2 3/3 4/4\n\
                                    usemtl Eyes\nf 5/1 6/2 7/3\n";

    #[test]
    fn says_when_a_material_file_names_no_texture() {
        let folder = test_folder("colour_only");
        let source = folder.join("Rat.obj");
        std::fs::write(&source, SQUARE_OBJ).unwrap();
        // As Blender writes it for a material whose image is not plugged into its colour.
        std::fs::write(folder.join("rat.mtl"), "newmtl Skin\nKd 0.8 0.8 0.8\nillum 2\n").unwrap();
        let (mut library, _) = CustomModelLibrary::open(folder.join("store"));
        let outcome = library.import(&source).unwrap();
        assert!(outcome.missing.is_empty() && outcome.missing_material_file.is_none());
        assert_eq!(outcome.colour_only, ["Skin"]);
        assert!(!outcome.model.parts[0].textured);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn takes_a_material_file_chosen_after_import() {
        let folder = test_folder("chosen_mtl");
        let source = folder.join("Rat.obj");
        std::fs::write(&source, TWO_MATERIAL_OBJ).unwrap();
        // The MTL file, under another name, and its textures are elsewhere.
        let elsewhere = folder.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let materials = elsewhere.join("rat_materials.mtl");
        std::fs::write(&materials, "newmtl Skin\nmap_Kd fur.png\nnewmtl Eyes\nmap_Kd eyes.png\n").unwrap();
        std::fs::write(elsewhere.join("fur.png"), png([90, 60, 40, 255])).unwrap();
        std::fs::write(elsewhere.join("eyes.png"), png([200, 0, 0, 255])).unwrap();

        let (mut library, _) = CustomModelLibrary::open(folder.join("store"));
        let imported = library.import(&source).unwrap();
        assert_eq!(imported.model.parts.len(), 1, "without materials, one part");
        let id = imported.model.id;

        // Read again with it: its two materials, each with its texture, under the same id.
        let outcome = library.set_materials(id, &materials).unwrap();
        assert!(outcome.missing.is_empty(), "{:?}", outcome.missing);
        assert!(outcome.colour_only.is_empty());
        assert_eq!(outcome.model.id, id);
        let parts: Vec<(&str, bool)> =
            outcome.model.parts.iter().map(|part| (part.material.as_str(), part.textured)).collect();
        assert_eq!(parts, [("Skin", true), ("Eyes", true)]);
        let firsts: Vec<[u8; 4]> =
            outcome.model.textures.iter().map(|(_, image)| image.pixels[..4].try_into().unwrap()).collect();
        assert_eq!(firsts, [[90, 60, 40, 255], [200, 0, 0, 255]]);
        assert_eq!(library.models().len(), 1);
        assert!(outcome.model.files.iter().all(|file| file.exists()));

        // With the model's own file gone, an MTL file of one material colours every part.
        std::fs::remove_file(&source).unwrap();
        let single = elsewhere.join("single.mtl");
        std::fs::write(&single, "newmtl Anything\nmap_Kd eyes.png\n").unwrap();
        let outcome = library.set_materials(id, &single).unwrap();
        assert!(outcome.model.parts.iter().all(|part| part.textured));
        assert!(outcome.model.textures.iter().all(|(_, image)| image.pixels[..4] == [200, 0, 0, 255]));
        // And one whose materials fit none of the parts is refused.
        let unfitting = elsewhere.join("unfitting.mtl");
        std::fs::write(&unfitting, "newmtl A\nmap_Kd eyes.png\nnewmtl B\nmap_Kd fur.png\n").unwrap();
        assert!(library.set_materials(id, &unfitting).is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn finds_textures_named_with_options_or_in_another_case_or_a_textures_folder() {
        let folder = test_folder("find_textures");
        std::fs::write(folder.join("Rat.obj"), SQUARE_OBJ).unwrap();
        // The MTL file differs in case; its texture has options before its name, and is in "Textures".
        std::fs::write(
            folder.join("RAT.MTL"),
            "newmtl Skin\nKd 1 1 1\nmap_Kd -s 1 1 1 -bm 0.5 C:\\Art\\rat_skin.PNG\n",
        )
        .unwrap();
        std::fs::create_dir_all(folder.join("Textures")).unwrap();
        std::fs::write(folder.join("Textures").join("Rat_Skin.png"), png([1, 2, 3, 255])).unwrap();

        let (mut library, _) = CustomModelLibrary::open(folder.join("store"));
        let outcome = library.import(&folder.join("Rat.obj")).unwrap();
        assert!(outcome.missing.is_empty(), "{:?}", outcome.missing);
        assert_eq!(outcome.model.parts, [ModelPart { material: "Skin".to_string(), textured: true }]);
        assert_eq!(outcome.model.textures[0].1.pixels[..4], [1, 2, 3, 255]);

        // A texture that is nowhere is said so, without the options, and the part drawn in its colour.
        std::fs::remove_dir_all(folder.join("Textures")).unwrap();
        let outcome = library.import(&folder.join("Rat.obj")).unwrap();
        assert_eq!(outcome.missing.len(), 1);
        assert!(outcome.missing[0].contains("'C:\\Art\\rat_skin.PNG'"), "{:?}", outcome.missing);
        assert!(!outcome.model.parts[0].textured);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
