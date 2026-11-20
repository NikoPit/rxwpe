use super::{
    OutputOptions, Presenter,
    wayland::{WaylandEvent, WaylandHost},
};
use crate::render::{Frame, Gpu};
use anyhow::{Context, Result};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) fn run<F>(options: OutputOptions, mut render: F) -> Result<()>
where
    F: FnMut(&Gpu, f64, (u32, u32)) -> Result<Frame> + 'static,
{
    let mut host = WaylandHost::new(options.display.as_deref())?;
    let initial = loop {
        match host.next_event()? {
            WaylandEvent::Configure { width, height } if width > 0 && height > 0 => {
                break (width, height);
            }
            WaylandEvent::Closed => return Ok(()),
            _ => {}
        }
    };
    let instance = Arc::new(wgpu::Instance::new(&wgpu::InstanceDescriptor::default()));
    let (raw_display_handle, raw_window_handle) = host.raw_handles()?;
    // SAFETY: host owns the Wayland display and surface and outlives presenter.
    // Presenter is declared after host and is dropped before it on every exit path.
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle,
            raw_window_handle,
        })
    }
    .context("create layer-shell GPU surface")?;
    let gpu = pollster::block_on(Gpu::from_instance(instance, Some(&surface)))?;
    let mut presenter = Presenter::new(gpu, surface, initial.0, initial.1, options.scale)?;
    let start = Instant::now();
    let interval = Duration::from_secs_f64(1. / options.fps as f64);
    let mut next_frame = Instant::now();
    loop {
        if let Some(wait) = next_frame.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        let frame = render(
            &presenter.gpu,
            start.elapsed().as_secs_f64(),
            presenter.size(),
        )?;
        host.request_frame()?;
        let committed = presenter.present(&frame)?;
        host.flush()?;
        next_frame = Instant::now() + interval;
        // A frame callback is delivered only after a buffer commit. Do not
        // block on the callback when swapchain recovery skipped presentation.
        if !committed {
            continue;
        }
        loop {
            match host.next_event()? {
                WaylandEvent::Configure { width, height } => presenter.resize(width, height),
                WaylandEvent::Frame => break,
                WaylandEvent::Closed => return Ok(()),
            }
        }
    }
}
