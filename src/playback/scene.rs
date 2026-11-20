//! Scene textures, particle simulation, audio, and frame composition.

use std::{collections::HashMap, sync::Arc, time::Duration};

use super::{audio::AudioPlayback, decoder::VideoDecoder, file::MediaFile, particles};
use crate::project::{
    Project,
    scene::{BlendMode, SceneDocument},
    texture::{decode_texture, video_texture},
};
use crate::render::{Blend, Frame, Gpu, GpuImage, Layer};
use anyhow::{Context, Result, ensure};

pub(crate) struct ScenePlayer {
    document: SceneDocument,
    layers: Vec<Layer>,
    frame: Option<Frame>,
    videos: Vec<VideoTexture>,
    particles: Vec<particles::ParticleSystem>,
    image_orders: Vec<usize>,
    _audio: Vec<AudioPlayback>,
}

struct VideoTexture {
    decoder: VideoDecoder,
    layers: Vec<usize>,
    timestamp: Duration,
    // Keep the asset until after its decoder closes.
    _file: MediaFile,
}

impl ScenePlayer {
    pub(crate) fn open(project: &Project, gpu: &Gpu, allow_partial: bool) -> Result<Self> {
        let document = SceneDocument::load(project)?;
        ensure!(
            allow_partial || document.warnings.is_empty(),
            "scene requires unsupported behavior:\n  {}\nUse --allow-partial only to preview the supported image layers.",
            document.warnings.join("\n  ")
        );

        let mut textures: HashMap<String, Arc<GpuImage>> = HashMap::new();
        let mut layers = Vec::new();
        let mut videos: Vec<VideoTexture> = Vec::new();
        let mut video_indices = HashMap::new();
        let mut image_orders = Vec::new();
        for image in &document.images {
            if !image.visible {
                continue;
            }
            image_orders.push(image.order);
            let texture = if let Some(texture) = textures.get(&image.texture) {
                texture.clone()
            } else {
                let bytes = project.assets.read(&image.texture)?;
                let texture = if let Some(payload) = video_texture(&bytes)? {
                    let file = MediaFile::from_bytes(payload)?;
                    let mut decoder = VideoDecoder::open(file.path())?;
                    let decoded = decoder.frame_at(Duration::ZERO)?;
                    let texture = gpu.upload_rgba(decoded.width, decoded.height, &decoded.rgba)?;
                    video_indices.insert(image.texture.clone(), videos.len());
                    videos.push(VideoTexture {
                        decoder,
                        layers: Vec::new(),
                        timestamp: decoded.timestamp,
                        _file: file,
                    });
                    texture
                } else {
                    let decoded = decode_texture(&bytes).with_context(|| {
                        format!(
                            "cannot decode texture {} for object {}",
                            image.texture, image.id
                        )
                    })?;
                    gpu.upload_rgba(decoded.width, decoded.height, &decoded.rgba)?
                };
                textures.insert(image.texture.clone(), texture.clone());
                texture
            };
            if let Some(index) = video_indices.get(&image.texture) {
                videos[*index].layers.push(layers.len());
            }
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
                uv: [0., 0., 1., 1.],
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
        let mut particles = Vec::new();
        for definition in &document.particles {
            if !definition.visible {
                continue;
            }
            let decoded = crate::project::texture::decode_sprite_texture(
                &project.assets.read(&definition.texture)?,
            )
            .with_context(|| format!("decode particle texture {}", definition.texture))?;
            let image = gpu.upload_rgba(
                decoded.texture.width,
                decoded.texture.height,
                &decoded.texture.rgba,
            )?;
            particles.push(particles::ParticleSystem::new(
                definition.clone(),
                image,
                decoded.frames,
            ));
        }
        let mut audio = Vec::new();
        for sound in &document.sounds {
            if sound.startsilent {
                continue;
            }
            for path in &sound.paths {
                audio.push(AudioPlayback::open(
                    &project.assets.read(path)?,
                    sound.looped,
                    sound.volume,
                )?);
            }
        }
        Ok(Self {
            document,
            layers,
            frame: None,
            videos,
            particles,
            image_orders,
            _audio: audio,
        })
    }

    pub(crate) fn warnings(&self) -> &[String] {
        &self.document.warnings
    }

    /// Static scenes reuse their completed GPU frame until the player changes.
    pub(crate) fn render(&mut self, gpu: &Gpu, elapsed: Duration) -> Result<Frame> {
        if !self.particles.is_empty() {
            self.frame = None;
        }
        for video in &mut self.videos {
            let decoded = video.decoder.frame_at(elapsed)?;
            if decoded.timestamp != video.timestamp {
                let texture = gpu.upload_rgba(decoded.width, decoded.height, &decoded.rgba)?;
                for index in &video.layers {
                    self.layers[*index].image = texture.clone();
                }
                video.timestamp = decoded.timestamp;
                self.frame = None;
            }
        }
        if let Some(frame) = &self.frame {
            return Ok(frame.clone());
        }
        let mut ordered: Vec<(usize, Vec<Layer>)> = self
            .layers
            .iter()
            .zip(&self.image_orders)
            .map(|(layer, order)| (*order, vec![layer.clone()]))
            .collect();
        for system in &mut self.particles {
            system.advance(elapsed);
            ordered.push((system.order(), system.layers(&self.document.camera)));
        }
        ordered.sort_by_key(|(order, _)| *order);
        let layers: Vec<Layer> = ordered.into_iter().flat_map(|(_, layers)| layers).collect();
        let frame = gpu.render_layers(
            self.document.width,
            self.document.height,
            self.document.clear_color.map(f64::from),
            &layers,
        )?;
        let frame = if let Some(bloom) = &self.document.bloom {
            gpu.bloom(&frame, bloom.threshold, bloom.strength, bloom.tint)?
        } else {
            frame
        };
        self.frame = Some(frame.clone());
        Ok(frame)
    }
}
