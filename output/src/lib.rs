//! Desktop and ordinary-window presentation of GPU frames.

use anyhow::{Context, Result, bail};
use rxwpe_gpu::{Blend, Compositor, Frame, Gpu, Layer};
use std::sync::Arc;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "linux")]
mod wayland_runner;
mod window;

#[derive(Debug, Clone, Copy, Default)]
pub enum ScaleMode {
    Fit,
    #[default]
    Fill,
    Stretch,
}

#[derive(Debug, Clone)]
pub struct OutputOptions {
    pub window: bool,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub title: String,
    pub scale: ScaleMode,
    pub display: Option<String>,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            window: false,
            width: 1280,
            height: 720,
            fps: 30,
            title: "rxwpe".into(),
            scale: ScaleMode::Fill,
            display: None,
        }
    }
}

pub fn run<F>(options: OutputOptions, render: F) -> Result<()>
where
    F: FnMut(&Gpu, f64, (u32, u32)) -> Result<Frame> + 'static,
{
    if options.width == 0 || options.height == 0 || options.fps == 0 {
        bail!("output dimensions and frame rate must be greater than zero");
    }
    if options.window {
        return window::run(options, render);
    }
    #[cfg(target_os = "macos")]
    {
        macos::run(options, render)
    }
    #[cfg(target_os = "linux")]
    {
        wayland_runner::run(options, render)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    bail!("desktop playback requires Linux with Wayland or macOS; use --window for a preview")
}

pub(crate) struct Presenter<'a> {
    pub gpu: Arc<Gpu>,
    surface: wgpu::Surface<'a>,
    config: wgpu::SurfaceConfiguration,
    compositor: Compositor,
    scale: ScaleMode,
}

impl<'a> Presenter<'a> {
    pub fn new(
        gpu: Arc<Gpu>,
        surface: wgpu::Surface<'a>,
        width: u32,
        height: u32,
        scale: ScaleMode,
    ) -> Result<Self> {
        let capabilities = surface.get_capabilities(&gpu.adapter);
        let format = capabilities
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .or_else(|| capabilities.formats.first().copied())
            .context("surface exposes no texture formats")?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&gpu.device, &config);
        let compositor = Compositor::new(&gpu.device, format);
        Ok(Self {
            gpu,
            surface,
            config,
            compositor,
            scale,
        })
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.gpu.device, &self.config);
    }
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }
    /// Returns false if no buffer was committed and the caller should retry.
    pub fn present(&mut self, frame: &Frame) -> Result<bool> {
        let target = match self.surface.get_current_texture() {
            Ok(target) => target,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.gpu.device, &self.config);
                return Ok(false);
            }
            Err(wgpu::SurfaceError::Timeout) => return Ok(false),
            Err(error) => return Err(error).context("acquire output frame"),
        };
        let (width, height) = self.size();
        let [draw_width, draw_height] =
            scaled_size(frame.width(), frame.height(), width, height, self.scale);
        let layer = Layer {
            corners: None,
            image: frame.image.clone(),
            center: [width as f32 / 2., height as f32 / 2.],
            size: [draw_width, draw_height],
            rotation: 0.,
            color: [1.; 4],
            blend: Blend::Replace,
        };
        let view = target
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.compositor
            .draw(&self.gpu, &view, width, height, [0., 0., 0., 1.], &[layer]);
        target.present();
        Ok(true)
    }
}

fn scaled_size(
    source_width: u32,
    source_height: u32,
    width: u32,
    height: u32,
    mode: ScaleMode,
) -> [f32; 2] {
    let sw = source_width as f32;
    let sh = source_height as f32;
    let sx = width as f32 / sw;
    let sy = height as f32 / sh;
    match mode {
        ScaleMode::Stretch => [width as f32, height as f32],
        ScaleMode::Fit => [sw * sx.min(sy), sh * sx.min(sy)],
        ScaleMode::Fill => [sw * sx.max(sy), sh * sx.max(sy)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scaling_keeps_aspect_ratio_and_distinguishes_crop() {
        let fit = scaled_size(1920, 1080, 1000, 1000, ScaleMode::Fit);
        assert!((fit[0] - 1000.).abs() < 0.001);
        assert!((fit[1] - 562.5).abs() < 0.001);
        let fill = scaled_size(1920, 1080, 1000, 1000, ScaleMode::Fill);
        assert!((fill[0] - 1777.7778).abs() < 0.001);
        assert!((fill[1] - 1000.).abs() < 0.001);
        assert_eq!(
            scaled_size(1920, 1080, 1000, 1000, ScaleMode::Stretch),
            [1000., 1000.]
        );
    }
}
