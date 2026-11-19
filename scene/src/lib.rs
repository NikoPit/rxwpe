//! Static scene-type wallpaper playback.
//!
//! Project and material parsing belong to `parser`; this crate instantiates
//! uploaded textures and produces a GPU frame for `output`. Unsupported scene
//! behavior fails by default. `allow_partial` permits an explicit preview.

use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Context, Result, ensure};
use rxwpe_gpu::{Blend, Frame, Gpu, GpuImage, Layer};
use rxwpe_parser::{BlendMode, Project, SceneDocument, decode_texture};

#[derive(Debug, Default, Clone, Copy)]
pub struct SceneOptions {
    pub allow_partial: bool,
}

pub struct ScenePlayer {
    document: SceneDocument,
    layers: Vec<Layer>,
    frame: Option<Frame>,
}

impl ScenePlayer {
    pub fn open(project: &Project, gpu: &Gpu, options: SceneOptions) -> Result<Self> {
        let document = project.scene()?;
        ensure!(
            options.allow_partial || document.warnings.is_empty(),
            "scene requires unsupported behavior:\n  {}\nUse --allow-partial only to preview the supported image layers.",
            document.warnings.join("\n  ")
        );

        let mut textures: HashMap<String, Arc<GpuImage>> = HashMap::new();
        let mut layers = Vec::new();
        for image in &document.images {
            if !image.visible {
                continue;
            }
            let texture = if let Some(texture) = textures.get(&image.texture) {
                texture.clone()
            } else {
                let bytes = project.assets.read(&image.texture)?;
                let decoded = decode_texture(&bytes).with_context(|| {
                    format!(
                        "cannot decode texture {} for object {}",
                        image.texture, image.id
                    )
                })?;
                let texture = gpu.upload_rgba(decoded.width, decoded.height, &decoded.rgba)?;
                textures.insert(image.texture.clone(), texture.clone());
                texture
            };
            let size = [
                if image.size[0] == 0.0 {
                    texture.width as f32
                } else {
                    image.size[0]
                },
                if image.size[1] == 0.0 {
                    texture.height as f32
                } else {
                    image.size[1]
                },
            ];
            layers.push(Layer {
                corners: Some(
                    [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]].map(|p| {
                        let mut v = [
                            p[0] * size[0] * image.scale[0],
                            p[1] * size[1] * image.scale[1],
                            0.,
                        ];
                        for (axis, angle) in image.angles.iter().enumerate() {
                            let (s, c) = angle.to_radians().sin_cos();
                            let a = (axis + 1) % 3;
                            let b = (axis + 2) % 3;
                            let va = v[a] * c - v[b] * s;
                            v[b] = v[a] * s + v[b] * c;
                            v[a] = va;
                        }
                        document
                            .camera
                            .project(std::array::from_fn(|i| v[i] + image.origin[i]))
                    }),
                ),
                image: texture,
                center: [image.origin[0], image.origin[1]],
                size: [size[0] * image.scale[0], size[1] * image.scale[1]],
                rotation: image.angles[2].to_radians(),
                blend: match image.blending {
                    BlendMode::Normal | BlendMode::Opaque => Blend::Replace,
                    BlendMode::Translucent => Blend::Alpha,
                    BlendMode::Additive => Blend::Additive,
                },
                color: [
                    image.color[0] * image.brightness,
                    image.color[1] * image.brightness,
                    image.color[2] * image.brightness,
                    image.alpha.clamp(0.0, 1.0),
                ],
            });
        }
        Ok(Self {
            document,
            layers,
            frame: None,
        })
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.document.width, self.document.height)
    }

    pub fn warnings(&self) -> &[String] {
        &self.document.warnings
    }

    /// Static scenes reuse their completed GPU frame until the player changes.
    pub fn render(&mut self, gpu: &Gpu, _elapsed: Duration) -> Result<Frame> {
        if let Some(frame) = &self.frame {
            return Ok(frame.clone());
        }
        let frame = gpu.render_layers(
            self.document.width,
            self.document.height,
            self.document.clear_color.map(f64::from),
            &self.layers,
        )?;
        self.frame = Some(frame.clone());
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a GPU adapter, including software Vulkan"]
    fn project_assets_to_gpu_frame_with_property_override() -> Result<()> {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/static-scene");
        let mut project = Project::load(directory, None)?;
        let gpu = Gpu::headless()?;
        let mut player = ScenePlayer::open(&project, &gpu, SceneOptions::default())?;
        let frame = player.render(&gpu, Duration::ZERO)?;
        assert_eq!((frame.width(), frame.height()), (320, 180));
        let rgba = gpu.read_rgba(&frame)?;
        let pixel = |x: usize, y: usize| &rgba[(y * 320 + x) * 4..(y * 320 + x) * 4 + 4];
        assert!(
            pixel(90, 60)[2] > pixel(90, 60)[0],
            "top-left checker must be blue"
        );
        assert!(
            pixel(230, 60)[0] > pixel(230, 60)[2],
            "top-right checker must be orange"
        );
        let original_background = pixel(0, 0).to_vec();
        project.set_property("opacity", 0.into())?;
        let mut player = ScenePlayer::open(&project, &gpu, SceneOptions::default())?;
        let frame = player.render(&gpu, Duration::ZERO)?;
        let rgba = gpu.read_rgba(&frame)?;
        let center = &rgba[(90 * 320 + 160) * 4..(90 * 320 + 160) * 4 + 4];
        assert_eq!(center, original_background);
        Ok(())
    }
}
