use anyhow::Result;
use wgpu::util::DeviceExt;

use crate::{FRAME_FORMAT, Frame, Gpu};

pub(crate) struct Bloom {
    layout: wgpu::BindGroupLayout,
    threshold: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    combine: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

impl Bloom {
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom inputs"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bloom"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bloom"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bloom.wgsl").into()),
        });
        let pipeline = |entry, format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        Self {
            threshold: pipeline("threshold", wgpu::TextureFormat::Rgba16Float),
            blur: pipeline("blur", wgpu::TextureFormat::Rgba16Float),
            combine: pipeline("combine", FRAME_FORMAT),
            layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("bloom sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
        }
    }

    pub fn apply(
        &self,
        gpu: &Gpu,
        frame: &Frame,
        threshold: f32,
        strength: f32,
        tint: [f32; 3],
    ) -> Result<Frame> {
        if strength <= 0. {
            return Ok(frame.clone());
        }
        let width = frame.width().div_ceil(4);
        let height = frame.height().div_ceil(4);
        let a = gpu.create_image_format(width, height, wgpu::TextureFormat::Rgba16Float)?;
        let b = gpu.create_image_format(width, height, wgpu::TextureFormat::Rgba16Float)?;
        let c = gpu.create_image_format(width, height, wgpu::TextureFormat::Rgba16Float)?;
        let output = gpu.create_image(frame.width(), frame.height())?;
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scene bloom"),
            });
        for (pipeline, source, target, values) in [
            (
                &self.threshold,
                frame.image.as_ref(),
                a.as_ref(),
                [threshold, strength, 0., 0.],
            ),
            (&self.blur, a.as_ref(), b.as_ref(), [1., 0., 0., 0.]),
            (&self.blur, b.as_ref(), c.as_ref(), [0., 1., 0., 0.]),
            (&self.combine, c.as_ref(), output.as_ref(), [0.; 4]),
        ] {
            let params = [values, [tint[0], tint[1], tint[2], 0.]];
            let buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("bloom parameters"),
                    contents: bytemuck::cast_slice(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom pass"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&source.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&frame.image.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: buffer.as_entire_binding(),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bloom pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([encoder.finish()]);
        Ok(Frame { image: output })
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Layer;

    #[test]
    #[ignore = "requires a GPU adapter, including software renderer"]
    fn bloom_spreads_bright_pixels_and_respects_threshold_and_strength() -> Result<()> {
        let gpu = Gpu::headless()?;
        let mut pixels = vec![0; 64 * 64 * 4];
        for p in pixels.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
        for y in 28..36 {
            for x in 28..36 {
                pixels[(y * 64 + x) * 4..(y * 64 + x) * 4 + 3].fill(255);
            }
        }
        let image = gpu.upload_rgba(64, 64, &pixels)?;
        let frame = gpu.render_layers(64, 64, [0.; 4], &[Layer::full_frame(image, 64, 64)])?;
        let dark = (32 * 64 + 22) * 4;
        let bloom = gpu.bloom(&frame, 0.3, 1., [1.; 3])?;
        assert!(gpu.read_rgba(&bloom)?[dark] > 0);
        assert_eq!(
            gpu.read_rgba(&gpu.bloom(&frame, 0.3, 0., [1.; 3])?)?,
            pixels
        );
        assert_eq!(gpu.read_rgba(&gpu.bloom(&frame, 1., 1., [1.; 3])?)?, pixels);
        Ok(())
    }
}
