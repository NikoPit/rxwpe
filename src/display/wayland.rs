//! A layer-shell surface used as a wallpaper, independent of the renderer.
//!
//! The GPU surface must be dropped before this host, because its raw handles
//! point into the Wayland objects owned here.

use std::{collections::VecDeque, ptr::NonNull};

use anyhow::{Context, Result, anyhow, bail};
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
};
use wayland_client::{
    Connection, EventQueue, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_surface},
};

/// An event relevant to the output's swapchain or render scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaylandEvent {
    /// Physical buffer dimensions, including the compositor's integer scale.
    Configure { width: u32, height: u32 },
    /// The compositor is ready for another frame.
    Frame,
    /// The compositor closed the wallpaper surface or removed its output.
    Closed,
}

/// Owns one background surface on one Wayland output.
pub(crate) struct WaylandHost {
    queue: EventQueue<State>,
    state: State,
    connection: Connection,
}

impl WaylandHost {
    pub(crate) fn new(output_name: Option<&str>) -> Result<Self> {
        let connection =
            Connection::connect_to_env().context("cannot connect to the Wayland compositor")?;
        let (globals, mut queue) =
            registry_queue_init(&connection).context("cannot enumerate Wayland globals")?;
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh)
            .context("the Wayland compositor does not expose wl_compositor")?;
        let shell = LayerShell::bind(&globals, &qh).context(
            "the compositor does not support wlr-layer-shell; use window output instead",
        )?;
        let mut state = State {
            registry: RegistryState::new(&globals),
            outputs: OutputState::new(&globals, &qh),
            layer: None,
            selected_output: None,
            logical_size: (1, 1),
            scale: 1,
            configured: false,
            frame_pending: false,
            events: VecDeque::new(),
        };

        // Output names arrive asynchronously after registry binding.
        queue
            .roundtrip(&mut state)
            .context("cannot read Wayland output information")?;
        let selected_output = if let Some(name) = output_name {
            let output = state.outputs.outputs().find(|output| {
                state
                    .outputs
                    .info(output)
                    .is_some_and(|info| info.name.as_deref() == Some(name))
            });
            match output {
                Some(output) => Some(output),
                None => {
                    let names: Vec<_> = state
                        .outputs
                        .outputs()
                        .filter_map(|output| state.outputs.info(&output).and_then(|info| info.name))
                        .collect();
                    bail!(
                        "Wayland output {name:?} was not found; available outputs: {}",
                        names.join(", ")
                    );
                }
            }
        } else {
            None
        };
        if let Some(info) = selected_output
            .as_ref()
            .and_then(|output| state.outputs.info(output))
        {
            state.scale = info.scale_factor.max(1);
            if let Some((width, height)) = info.logical_size {
                state.logical_size = (width.max(1) as u32, height.max(1) as u32);
            }
        }
        let surface = compositor.create_surface(&qh);
        surface.set_buffer_scale(state.scale);
        // Wallpapers must not consume pointer events destined for desktop apps.
        let empty_region =
            Region::new(&compositor).context("cannot create Wayland input region")?;
        surface.set_input_region(Some(empty_region.wl_region()));
        let layer = shell.create_layer_surface(
            &qh,
            surface,
            Layer::Background,
            Some("rxwpe"),
            selected_output.as_ref(),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, 0);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        state.selected_output = selected_output;
        state.layer = Some(layer);
        // The initial commit has no buffer; attaching before configure is a protocol error.
        state.layer.as_ref().expect("layer initialized").commit();
        connection
            .flush()
            .context("cannot flush initial wallpaper surface")?;
        Ok(Self {
            queue,
            state,
            connection,
        })
    }

    /// Wait for a configure, frame callback or close event.
    pub(crate) fn next_event(&mut self) -> Result<WaylandEvent> {
        loop {
            if let Some(event) = self.state.events.pop_front() {
                return Ok(event);
            }
            self.queue
                .blocking_dispatch(&mut self.state)
                .context("Wayland event dispatch failed")?;
        }
    }

    /// Obtain raw handles while retaining ownership of their Wayland objects.
    ///
    /// A renderer using these handles must keep this host alive until its GPU
    /// surface has been destroyed.
    pub(crate) fn raw_handles(&self) -> Result<(RawDisplayHandle, RawWindowHandle)> {
        let display = NonNull::new(self.connection.backend().display_ptr().cast())
            .ok_or_else(|| anyhow!("Wayland display pointer is null"))?;
        let layer = self
            .state
            .layer
            .as_ref()
            .context("Wayland layer surface is unavailable")?;
        let surface = NonNull::new(layer.wl_surface().id().as_ptr().cast())
            .ok_or_else(|| anyhow!("Wayland surface pointer is null"))?;
        Ok((
            RawDisplayHandle::Wayland(WaylandDisplayHandle::new(display)),
            RawWindowHandle::Wayland(WaylandWindowHandle::new(surface)),
        ))
    }

    /// Request a frame callback before presenting the next GPU buffer.
    pub(crate) fn request_frame(&mut self) -> Result<()> {
        if !self.state.configured {
            bail!("cannot request a frame before the first layer-shell configure");
        }
        if !self.state.frame_pending {
            let surface = self
                .state
                .layer
                .as_ref()
                .context("Wayland layer is unavailable")?
                .wl_surface();
            surface.frame(&self.queue.handle(), surface.clone());
            self.state.frame_pending = true;
        }
        Ok(())
    }

    /// Send queued Wayland requests after the renderer has presented its buffer.
    pub(crate) fn flush(&self) -> Result<()> {
        self.connection
            .flush()
            .context("cannot flush Wayland requests")
    }
}

struct State {
    registry: RegistryState,
    outputs: OutputState,
    layer: Option<LayerSurface>,
    selected_output: Option<wl_output::WlOutput>,
    logical_size: (u32, u32),
    scale: i32,
    configured: bool,
    frame_pending: bool,
    events: VecDeque<WaylandEvent>,
}

impl State {
    fn physical_size(&self) -> (u32, u32) {
        (
            self.logical_size.0.saturating_mul(self.scale as u32),
            self.logical_size.1.saturating_mul(self.scale as u32),
        )
    }

    fn resized(&mut self) {
        if self.configured {
            let (width, height) = self.physical_size();
            self.events
                .push_back(WaylandEvent::Configure { width, height });
        }
    }
}

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        if self
            .layer
            .as_ref()
            .is_some_and(|layer| layer.wl_surface() == surface)
        {
            let scale = new_factor.max(1);
            if self.scale != scale {
                self.scale = scale;
                surface.set_buffer_scale(scale);
                self.resized();
            }
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.events.push_back(WaylandEvent::Frame);
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.events.push_back(WaylandEvent::Closed);
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        if configure.new_size.0 != 0 {
            self.logical_size.0 = configure.new_size.0;
        }
        if configure.new_size.1 != 0 {
            self.logical_size.1 = configure.new_size.1;
        }
        self.configured = true;
        self.resized();
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if self.selected_output.as_ref() == Some(&output) {
            self.events.push_back(WaylandEvent::Closed);
        }
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState];
}

delegate_compositor!(State);
delegate_output!(State);
delegate_layer!(State);
delegate_registry!(State);
