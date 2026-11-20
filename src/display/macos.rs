//! AppKit desktop windows. All calls run on winit's main event-loop thread.

use super::OutputOptions;
use crate::render::{Frame, Gpu};
use anyhow::{Context, Result, bail};
use objc2::rc::Retained;
use objc2_app_kit::{NSScreen, NSView, NSWindow, NSWindowCollectionBehavior};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::time::{Duration, Instant};
use winit::{
    dpi::LogicalSize,
    event_loop::{ActiveEventLoop, EventLoopBuilder},
    monitor::MonitorHandle,
    platform::macos::{
        ActivationPolicy, EventLoopBuilderExtMacOS, MonitorHandleExtMacOS, WindowAttributesExtMacOS,
    },
    window::Window,
};

// CGWindowLevelKey's public desktop key. Query the level rather than relying on
// a numeric window level, which macOS is free to change.
const DESKTOP_WINDOW_LEVEL_KEY: u32 = 2;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowLevelForKey(key: u32) -> i32;
}

pub(crate) fn run<F>(options: OutputOptions, render: F) -> Result<()>
where
    F: FnMut(&Gpu, f64, (u32, u32)) -> Result<Frame> + 'static,
{
    super::window::run(options, render)
}

pub(crate) fn configure_event_loop(builder: &mut EventLoopBuilder<()>) {
    builder
        .with_activation_policy(ActivationPolicy::Accessory)
        .with_activate_ignoring_other_apps(false)
        .with_default_menu(false);
}

pub(crate) fn create_desktop_window(
    event_loop: &ActiveEventLoop,
    options: &OutputOptions,
) -> Result<(Window, DesktopDisplay)> {
    let monitor = select_monitor(event_loop, options.display.as_deref())?;
    let screen_ptr = monitor
        .ns_screen()
        .context("selected display has no NSScreen")?;
    // SAFETY: winit supplies this NSScreen pointer on the main thread. Retain it
    // while reading the frame so a display notification cannot invalidate it.
    let screen = unsafe { Retained::<NSScreen>::retain(screen_ptr.cast()) }
        .context("selected display has no NSScreen")?;
    let frame = screen.frame();
    let window = event_loop
        .create_window(
            Window::default_attributes()
                .with_title(&options.title)
                .with_inner_size(LogicalSize::new(frame.size.width, frame.size.height))
                .with_decorations(false)
                .with_resizable(false)
                .with_active(false)
                .with_visible(false)
                .with_has_shadow(false),
        )
        .context("create macOS desktop window")?;
    let native = native_window(&window)?;
    // One level above the system wallpaper and below desktop icons. Never use
    // normal/fullscreen window levels for a background.
    native.setLevel(unsafe { CGWindowLevelForKey(DESKTOP_WINDOW_LEVEL_KEY) } as isize + 1);
    native.setIgnoresMouseEvents(true);
    native.setMovable(false);
    // Use NSScreen's frame directly. It is in AppKit points, including the menu
    // bar and Dock areas, and handles mixed Retina/non-Retina display origins.
    native.setFrame_display(frame, false);
    // SAFETY: These flags do not change ownership or the window delegate. The
    // window remains owned by winit and every call occurs on the main thread.
    unsafe {
        native.setHidesOnDeactivate(false);
        native.setExcludedFromWindowsMenu(true);
        native.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
    }
    let display = DesktopDisplay {
        monitor_id: monitor.native_id(),
        frame: [
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        ],
        next_check: Instant::now() + Duration::from_secs(1),
    };
    Ok((window, display))
}

/// Keep the selected display identity when macOS rearranges its screen list.
pub(crate) struct DesktopDisplay {
    monitor_id: u32,
    frame: [f64; 4],
    next_check: Instant,
}

impl DesktopDisplay {
    pub(crate) fn refresh(&mut self, event_loop: &ActiveEventLoop, window: &Window) -> Result<()> {
        if Instant::now() < self.next_check {
            return Ok(());
        }
        self.next_check = Instant::now() + Duration::from_secs(1);
        let monitor = event_loop
            .available_monitors()
            .find(|monitor| monitor.native_id() == self.monitor_id)
            .context("macOS wallpaper display was disconnected")?;
        let screen_ptr = monitor.ns_screen().context("display has no NSScreen")?;
        // SAFETY: As during creation, this is winit's live NSScreen pointer on
        // the main thread. Retain it for the frame lookup.
        let screen = unsafe { Retained::<NSScreen>::retain(screen_ptr.cast()) }
            .context("display has no NSScreen")?;
        let frame = screen.frame();
        let coordinates = [
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        ];
        if self.frame != coordinates {
            native_window(window)?.setFrame_display(frame, false);
            self.frame = coordinates;
        }
        Ok(())
    }
}

pub(crate) fn show_desktop_window(window: &Window) -> Result<()> {
    // SAFETY: The native window is valid on this main thread. Unlike winit's
    // set_visible, orderFrontRegardless does not make it the key window.
    unsafe { native_window(window)?.orderFrontRegardless() };
    Ok(())
}

fn native_window(window: &Window) -> Result<Retained<NSWindow>> {
    let handle = window.window_handle().context("get macOS window handle")?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        bail!("expected an AppKit window handle");
    };
    // SAFETY: The raw handle belongs to the live winit window, and the caller is
    // on the main thread. Retaining the NSView keeps it alive for this lookup.
    let view = unsafe { Retained::<NSView>::retain(handle.ns_view.as_ptr().cast()) }
        .context("macOS window has no NSView")?;
    view.window().context("macOS view has no NSWindow")
}

fn select_monitor(event_loop: &ActiveEventLoop, display: Option<&str>) -> Result<MonitorHandle> {
    let monitors: Vec<_> = event_loop.available_monitors().collect();
    let Some(display) = display else {
        return event_loop
            .primary_monitor()
            .or_else(|| monitors.first().cloned())
            .context("no macOS displays are available");
    };
    if let Ok(index) = display.parse::<usize>() {
        if let Some(monitor) = monitors.get(index) {
            return Ok(monitor.clone());
        }
    } else if let Some(monitor) = monitors
        .iter()
        .find(|monitor| monitor.name().as_deref() == Some(display))
    {
        return Ok(monitor.clone());
    }
    let available = monitors
        .iter()
        .enumerate()
        .map(|(index, monitor)| format!("{index}: {}", monitor.name().unwrap_or_default()))
        .collect::<Vec<_>>()
        .join(", ");
    bail!("macOS display {display:?} not found; available displays: {available}")
}
