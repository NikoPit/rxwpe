//! Wallpaper project descriptors and a supported subset of scene data.
//!
//! Loading preserves the original JSON. Scene conversion reports unsupported
//! objects and effects rather than claiming full Wallpaper Engine compatibility.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use rxwpe_assets::AssetStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod scene;
mod texture;
pub use scene::{BlendMode, ImageLayer, SceneCamera, SceneDocument};
pub use texture::{DecodedTexture, decode_texture};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WallpaperType {
    Scene,
    Video,
    Web,
}

impl std::fmt::Display for WallpaperType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Scene => "scene",
            Self::Video => "video",
            Self::Web => "web",
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectMetadata {
    pub title: String,
    pub kind: WallpaperType,
    pub file: String,
    pub properties: BTreeMap<String, Value>,
}

#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub metadata: ProjectMetadata,
    pub assets: Arc<AssetStore>,
    /// Original descriptor, including fields unsupported by this version.
    pub descriptor: Value,
}

impl Project {
    pub fn load(path: impl AsRef<Path>, shared_assets: Option<&Path>) -> Result<Self> {
        let assets = Arc::new(AssetStore::open(path.as_ref(), shared_assets)?);
        let descriptor = read_json(&assets, "project.json")?;
        let type_name = descriptor
            .get("type")
            .and_then(Value::as_str)
            .context("project.json has no string 'type'")?;
        let kind = match type_name.to_ascii_lowercase().as_str() {
            "scene" => WallpaperType::Scene,
            "video" => WallpaperType::Video,
            "web" => WallpaperType::Web,
            _ => bail!("unsupported wallpaper type: {type_name}"),
        };
        let file = descriptor
            .get("file")
            .and_then(Value::as_str)
            .context("project.json has no string 'file' entry point")?;
        let file = rxwpe_assets::normalize_path(file)?;
        ensure!(
            assets.exists(&file),
            "project entry point does not exist: {file}"
        );
        let properties = descriptor
            .pointer("/general/properties")
            .and_then(Value::as_object)
            .map(|p| p.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        let metadata = ProjectMetadata {
            title: descriptor
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Untitled wallpaper")
                .to_owned(),
            kind,
            file,
            properties,
        };
        Ok(Self {
            root: assets.root().to_owned(),
            assets,
            metadata,
            descriptor,
        })
    }

    pub fn scene(&self) -> Result<SceneDocument> {
        ensure!(
            self.metadata.kind == WallpaperType::Scene,
            "project is not a scene wallpaper"
        );
        scene::load(self)
    }

    /// Applies an override before creating a player or parsing scene properties.
    pub fn set_property(&mut self, name: &str, value: Value) -> Result<()> {
        let property = self
            .metadata
            .properties
            .get_mut(name)
            .with_context(|| format!("unknown wallpaper property: {name}"))?;
        if let Some(object) = property.as_object_mut() {
            object.insert("value".into(), value);
        } else {
            *property = value;
        }
        Ok(())
    }

    pub fn entry_path(&self) -> Result<PathBuf> {
        self.assets.physical_path(&self.metadata.file)
    }
}

pub(crate) fn read_json(assets: &AssetStore, path: &str) -> Result<Value> {
    let bytes = assets.read(path)?;
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    serde_json::from_slice(bytes).with_context(|| format!("invalid JSON in {path}"))
}
