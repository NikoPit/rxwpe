use super::{OutputOptions, Presenter};
use anyhow::{Context, Result};
use rxwpe_gpu::{Frame, Gpu};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

pub fn run<F>(options: OutputOptions, render: F) -> Result<()>
where
    F: FnMut(&Gpu, f64, (u32, u32)) -> Result<Frame> + 'static,
{
    let mut builder = EventLoop::builder();
    #[cfg(target_os = "macos")]
    if matches!(options.backend, super::Backend::Macos) {
        super::macos::configure_event_loop(&mut builder);
    }
    let event_loop = builder.build().context("create window event loop")?;
    let mut app = App {
        options,
        render,
        window: None,
        presenter: None,
        #[cfg(target_os = "macos")]
        desktop_display: None,
        start: Instant::now(),
        next_frame: Instant::now(),
        error: None,
    };
    event_loop
        .run_app(&mut app)
        .context("run window event loop")?;
    if let Some(error) = app.error {
        return Err(error);
    }
    Ok(())
}

struct App<F> {
    options: OutputOptions,
    render: F,
    window: Option<Arc<Window>>,
    presenter: Option<Presenter<'static>>,
    #[cfg(target_os = "macos")]
    desktop_display: Option<super::macos::DesktopDisplay>,
    start: Instant,
    next_frame: Instant,
    error: Option<anyhow::Error>,
}

impl<F> App<F>
where
    F: FnMut(&Gpu, f64, (u32, u32)) -> Result<Frame> + 'static,
{
    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        #[cfg(target_os = "macos")]
        let desktop = matches!(self.options.backend, super::Backend::Macos);
        #[cfg(target_os = "macos")]
        let window = if desktop {
            let (window, display) = super::macos::create_desktop_window(event_loop, &self.options)?;
            self.desktop_display = Some(display);
            window
        } else {
            create_window(event_loop, &self.options)?
        };
        #[cfg(not(target_os = "macos"))]
        let window = create_window(event_loop, &self.options)?;
        let window = Arc::new(window);
        let size = window.inner_size();
        let instance = Arc::new(wgpu::Instance::new(&wgpu::InstanceDescriptor::default()));
        let surface = instance
            .create_surface(window.clone())
            .context("create window GPU surface")?;
        let gpu = pollster::block_on(Gpu::from_instance(instance, Some(&surface)))?;
        self.presenter = Some(Presenter::new(
            gpu,
            surface,
            size.width,
            size.height,
            self.options.scale,
        )?);
        #[cfg(target_os = "macos")]
        if desktop {
            super::macos::show_desktop_window(&window)?;
        }
        self.window = Some(window);
        self.start = Instant::now();
        self.next_frame = self.start;
        Ok(())
    }
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error = Some(error);
        event_loop.exit();
    }
}

fn create_window(event_loop: &ActiveEventLoop, options: &OutputOptions) -> Result<Window> {
    event_loop
        .create_window(
            Window::default_attributes()
                .with_title(&options.title)
                .with_inner_size(PhysicalSize::new(options.width, options.height)),
        )
        .context("create wallpaper window")
}

impl<F> ApplicationHandler for App<F>
where
    F: FnMut(&Gpu, f64, (u32, u32)) -> Result<Frame> + 'static,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let initialized = if self.window.is_some() {
            Ok(())
        } else {
            self.initialize(event_loop)
        };
        if let Err(error) = initialized {
            self.fail(event_loop, error);
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && event.logical_key == Key::Named(NamedKey::Escape) =>
            {
                event_loop.exit()
            }
            WindowEvent::Resized(size) => {
                if let Some(presenter) = &mut self.presenter {
                    presenter.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(presenter) = &mut self.presenter else {
                    return;
                };
                let size = presenter.size();
                let result =
                    (self.render)(&presenter.gpu, self.start.elapsed().as_secs_f64(), size)
                        .and_then(|frame| presenter.present(&frame));
                if let Err(error) = result {
                    self.fail(event_loop, error);
                    return;
                }
                self.next_frame =
                    Instant::now() + Duration::from_secs_f64(1. / self.options.fps as f64);
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "macos")]
        if let (Some(display), Some(window)) = (&mut self.desktop_display, &self.window) {
            if let Err(error) = display.refresh(event_loop, window) {
                self.fail(event_loop, error);
                return;
            }
        }
        if let Some(window) = self
            .window
            .as_ref()
            .filter(|_| Instant::now() >= self.next_frame)
        {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}
