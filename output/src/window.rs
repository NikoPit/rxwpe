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
    let event_loop = EventLoop::new().context("create window event loop")?;
    let mut app = App {
        options,
        render,
        window: None,
        presenter: None,
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
    start: Instant,
    next_frame: Instant,
    error: Option<anyhow::Error>,
}

impl<F> App<F>
where
    F: FnMut(&Gpu, f64, (u32, u32)) -> Result<Frame> + 'static,
{
    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(&self.options.title)
                        .with_inner_size(PhysicalSize::new(
                            self.options.width,
                            self.options.height,
                        )),
                )
                .context("create wallpaper window")?,
        );
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
