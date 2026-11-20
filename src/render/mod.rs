//! Shared GPU device, uploaded images, and textured-quad composition.

use std::sync::Arc;
#[cfg(test)]
use std::sync::mpsc;

use anyhow::{Context, Result, bail};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

pub(crate) const FRAME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
mod bloom;
#[expect(dead_code, reason = "Custom shader rendering is not implemented yet.")]
mod shader;

pub(crate) struct Gpu {
    _instance: Arc<wgpu::Instance>,
    pub(crate) adapter: Arc<wgpu::Adapter>,
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    compositor: Compositor,
    bloom: bloom::Bloom,
}

pub(crate) struct GpuImage {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Clone)]
pub(crate) struct Frame {
    pub(crate) image: Arc<GpuImage>,
}

impl Frame {
    pub(crate) fn width(&self) -> u32 {
        self.image.width
    }
    pub(crate) fn height(&self) -> u32 {
        self.image.height
    }
}

/// RGB composition rule for an image layer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) enum Blend {
    #[default]
    Alpha,
    Replace,
    Additive,
}

/// A quad in bottom-left-origin coordinates. Rotation is in radians.
#[derive(Clone)]
pub(crate) struct Layer {
    pub(crate) image: Arc<GpuImage>,
    pub(crate) center: [f32; 2],
    pub(crate) size: [f32; 2],
    pub(crate) rotation: f32,
    pub(crate) color: [f32; 4],
    pub(crate) blend: Blend,
    /// Optional projected corners in bottom-left, bottom-right, top-right, top-left order.
    pub(crate) corners: Option<[[f32; 2]; 4]>,
    pub(crate) uv: [f32; 4],
}

impl Layer {
    #[cfg(test)]
    pub(crate) fn full_frame(image: Arc<GpuImage>, width: u32, height: u32) -> Self {
        Self {
            image,
            center: [width as f32 / 2., height as f32 / 2.],
            size: [width as f32, height as f32],
            rotation: 0.,
            color: [1.; 4],
            blend: Blend::Alpha,
            corners: None,
            uv: [0., 0., 1., 1.],
        }
    }
}

impl Gpu {
    #[cfg(test)]
    pub(crate) fn headless() -> Result<Arc<Self>> {
        let instance = Arc::new(wgpu::Instance::new(
            &wgpu::InstanceDescriptor::from_env_or_default(),
        ));
        pollster::block_on(Self::from_instance(instance, None))
    }

    pub(crate) async fn from_instance(
        instance: Arc<wgpu::Instance>,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Arc<Self>> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: surface,
                force_fallback_adapter: false,
            })
            .await
            .context("no compatible GPU adapter found")?;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("rxwpe device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                },
                None,
            )
            .await
            .context("create GPU device")?;
        let compositor = Compositor::new(&device, FRAME_FORMAT);
        let bloom = bloom::Bloom::new(&device);
        Ok(Arc::new(Self {
            _instance: instance,
            adapter: Arc::new(adapter),
            device: Arc::new(device),
            queue: Arc::new(queue),
            compositor,
            bloom,
        }))
    }

    fn validate_size(&self, width: u32, height: u32) -> Result<()> {
        let maximum = self.device.limits().max_texture_dimension_2d;
        if width == 0 || height == 0 || width > maximum || height > maximum {
            bail!("invalid image size {width}x{height}; GPU limit is {maximum}");
        }
        Ok(())
    }

    pub(crate) fn upload_rgba(
        &self,
        width: u32,
        height: u32,
        bytes: &[u8],
    ) -> Result<Arc<GpuImage>> {
        self.validate_size(width, height)?;
        let expected = u64::from(width) * u64::from(height) * 4;
        if bytes.len() as u64 != expected {
            bail!("RGBA image requires {expected} bytes, got {}", bytes.len());
        }
        let image = self.create_image(width, height)?;
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &image.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        Ok(image)
    }

    pub(crate) fn create_image(&self, width: u32, height: u32) -> Result<Arc<GpuImage>> {
        self.create_image_format(width, height, FRAME_FORMAT)
    }

    fn create_image_format(
        &self,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Result<Arc<GpuImage>> {
        self.validate_size(width, height)?;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("wallpaper image"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Ok(Arc::new(GpuImage {
            texture,
            view,
            width,
            height,
        }))
    }

    pub(crate) fn render_layers(
        &self,
        width: u32,
        height: u32,
        clear: [f64; 4],
        layers: &[Layer],
    ) -> Result<Frame> {
        let image = self.create_image(width, height)?;
        self.compositor
            .draw(self, &image.view, width, height, clear, layers);
        Ok(Frame { image })
    }

    #[cfg(test)]
    pub(crate) fn read_rgba(&self, frame: &Frame) -> Result<Vec<u8>> {
        let width = frame.width();
        let height = frame.height();
        let row_bytes = width * 4;
        let padded_row = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame readback"),
            size: u64::from(padded_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame readback"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &frame.image.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (tx, rx) = mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .context("readback callback disappeared")?
            .context("map frame readback")?;
        let mapped = slice.get_mapped_range();
        let mut rgba = Vec::with_capacity(row_bytes as usize * height as usize);
        for row in mapped.chunks_exact(padded_row as usize) {
            rgba.extend_from_slice(&row[..row_bytes as usize]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(rgba)
    }

    pub(crate) fn bloom(
        &self,
        frame: &Frame,
        threshold: f32,
        strength: f32,
        tint: [f32; 3],
    ) -> Result<Frame> {
        self.bloom.apply(self, frame, threshold, strength, tint)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

/// Draws shared GPU images onto a render target of one format.
pub(crate) struct Compositor {
    pipeline: wgpu::RenderPipeline,
    replace_pipeline: wgpu::RenderPipeline,
    additive_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl Compositor {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quad textures"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("textured quad"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });
        let make_pipeline = |blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quad compositor"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x2, 1=>Float32x2, 2=>Float32x4],
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        })
        };
        let pipeline = make_pipeline(Some(wgpu::BlendState::ALPHA_BLENDING));
        let replace_pipeline = make_pipeline(None);
        let additive_component = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let additive_pipeline = make_pipeline(Some(wgpu::BlendState {
            color: additive_component,
            alpha: additive_component,
        }));
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("quad sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            replace_pipeline,
            additive_pipeline,
            layout,
            sampler,
        }
    }

    pub(crate) fn draw(
        &self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        clear: [f64; 4],
        layers: &[Layer],
    ) {
        let resources: Vec<_> = layers
            .iter()
            .map(|layer| {
                let vertices = quad_vertices(layer, width, height);
                let vertex = gpu
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("quad vertices"),
                        contents: bytemuck::cast_slice(&vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("quad image"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&layer.image.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                });
                (vertex, bind)
            })
            .collect();
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("compose frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("compose layers"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear[0],
                            g: clear[1],
                            b: clear[2],
                            a: clear[3],
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            for ((vertex, bind), layer) in resources.iter().zip(layers) {
                pass.set_pipeline(match layer.blend {
                    Blend::Alpha => &self.pipeline,
                    Blend::Replace => &self.replace_pipeline,
                    Blend::Additive => &self.additive_pipeline,
                });
                pass.set_bind_group(0, bind, &[]);
                pass.set_vertex_buffer(0, vertex.slice(..));
                pass.draw(0..6, 0..1);
            }
        }
        gpu.queue.submit([encoder.finish()]);
    }
}

fn quad_vertices(layer: &Layer, width: u32, height: u32) -> [Vertex; 6] {
    let corners = [
        ([-0.5, -0.5], [0., 1.]),
        ([0.5, -0.5], [1., 1.]),
        ([0.5, 0.5], [1., 0.]),
        ([-0.5, 0.5], [0., 0.]),
    ];
    let (sin, cos) = layer.rotation.sin_cos();
    let points = std::array::from_fn::<_, 4, _>(|index| {
        let (p, uv) = corners[index];
        let uv = [
            layer.uv[0] + uv[0] * (layer.uv[2] - layer.uv[0]),
            layer.uv[1] + uv[1] * (layer.uv[3] - layer.uv[1]),
        ];
        let x = p[0] * layer.size[0];
        let y = p[1] * layer.size[1];
        let point = layer.corners.map(|corners| corners[index]).unwrap_or([
            layer.center[0] + x * cos - y * sin,
            layer.center[1] + x * sin + y * cos,
        ]);
        let position = [
            point[0] * 2. / width as f32 - 1.,
            point[1] * 2. / height as f32 - 1.,
        ];
        Vertex {
            position,
            uv,
            color: layer.color,
        }
    });
    [
        points[0], points[1], points[2], points[0], points[2], points[3],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a GPU adapter, including software renderer"]
    fn offscreen_composition_preserves_orientation_and_alpha() -> Result<()> {
        let gpu = Gpu::headless()?;
        let image = gpu.upload_rgba(
            2,
            2,
            &[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        )?;
        let frame = gpu.render_layers(2, 2, [0., 0., 0., 1.], &[Layer::full_frame(image, 2, 2)])?;
        assert_eq!(
            gpu.read_rgba(&frame)?,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255
            ]
        );
        let foreground = gpu.upload_rgba(1, 1, &[255, 0, 0, 128])?;
        let frame = gpu.render_layers(
            1,
            1,
            [0., 0., 1., 1.],
            &[Layer::full_frame(foreground, 1, 1)],
        )?;
        let pixel = gpu.read_rgba(&frame)?;
        assert!(pixel[0] > 180 && pixel[2] > 180);
        assert_eq!(pixel[3], 255);
        // A normal material replaces the background even when its texture has
        // partial alpha. It must not accidentally use translucent composition.
        let foreground = gpu.upload_rgba(1, 1, &[255, 0, 0, 128])?;
        let mut layer = Layer::full_frame(foreground, 1, 1);
        layer.blend = Blend::Replace;
        let frame = gpu.render_layers(1, 1, [0., 0., 1., 1.], &[layer.clone()])?;
        assert_eq!(gpu.read_rgba(&frame)?, [255, 0, 0, 128]);
        layer.blend = Blend::Additive;
        let frame = gpu.render_layers(1, 1, [0., 0., 1., 1.], &[layer])?;
        let pixel = gpu.read_rgba(&frame)?;
        assert!(pixel[0] > 180);
        assert_eq!(
            pixel[2], 255,
            "additive blending retains the blue background"
        );
        Ok(())
    }
}
