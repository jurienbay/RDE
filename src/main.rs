// SPDX-License-Identifier: BSD-3-Clause
// Copyright (C) <2026>, <Edward O'Callaghan>, All rights reserved.

use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Display, DisplayHandle};

use smithay::backend::allocator::Buffer;
use smithay::backend::winit::{self, WinitEvent};
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::output::{Output, PhysicalProperties, Subpixel};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{CompositorHandler, CompositorState};
use smithay::wayland::dmabuf::get_dmabuf;
use smithay::wayland::output::{OutputHandler, OutputManagerState};
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::session_lock::{
    LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker,
};
use smithay::wayland::shell::xdg::decoration::{XdgDecorationHandler, XdgDecorationState};
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState};

use secrecy::{ExposeSecret, SecretBox};

use std::sync::Arc;

mod constants;
use constants::*;
mod vk_engine;
use vk_engine::VulkanEngine;
mod glsl;
mod window_ctx;
use window_ctx::*;

// Add an active drag state tracker enum
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GrabState {
    Idle,
    Dragging {
        window_index: usize,
        start_mouse: (f64, f64),
        start_window: (f32, f32),
    },
    Resizing {
        window_index: usize,
        edge: ResizeEdge,
        start_mouse: (f64, f64),
        start_window_pos: (f32, f32),
        start_window_size: (f32, f32),
    },
    ButtonPressed {
        window_index: usize,
        button_type: u32,
    },
}

pub struct CdeCompositorState {
    pub display_handle: DisplayHandle,
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState, // Standard tracking manager for app frames
    pub xdg_decoration_state: XdgDecorationState,
    pub data_device_state: DataDeviceState,
    pub shm_state: ShmState,
    pub dmabuf_state: smithay::wayland::dmabuf::DmabufState,
    pub dmabuf_global: smithay::wayland::dmabuf::DmabufGlobal,
    pub output_manager_state: OutputManagerState,
    pub main_output: Output,
    pub seat_state: SeatState<CdeCompositorState>, // Handles input focus configuration
    pub seat: Seat<CdeCompositorState>,
    pub vulkan_engine: VulkanEngine,
    pub open_windows: Vec<WindowContext>, // Tracks the Z-order list of active clients
    pub cascade_step_index: u32,
    pub pointer_location: (f64, f64),
    pub current_grab: GrabState,
    pub active_workspace: u32,
    pub active_menu: Option<ActiveMenu>,
    pub session_lock_state: SessionLockManagerState,
    pub is_session_locked: bool,
    pub password_length: u32,
    pub secure_password_buffer: SecretBox<Vec<u8>>,
}

smithay::delegate_data_device!(CdeCompositorState);
smithay::delegate_dmabuf!(CdeCompositorState);
smithay::delegate_compositor!(CdeCompositorState);
smithay::delegate_xdg_shell!(CdeCompositorState);
smithay::delegate_xdg_decoration!(CdeCompositorState);
smithay::delegate_seat!(CdeCompositorState);
smithay::delegate_session_lock!(CdeCompositorState);
smithay::delegate_shm!(CdeCompositorState);
smithay::delegate_output!(CdeCompositorState);

impl CompositorHandler for CdeCompositorState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }
    fn client_compositor_state<'a>(
        &self,
        c: &'a wayland_server::Client,
    ) -> &'a smithay::wayland::compositor::CompositorClientState {
        &c.get_data::<ClientState>().unwrap().compositor_state
    }
    fn commit(&mut self, surface: &wayland_server::protocol::wl_surface::WlSurface) {
        let time = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u32;

        // XXX
        let mut committed_buffer = None;

        smithay::wayland::compositor::with_states(surface, |states| {
            // Signal to the application frame state machine that the buffer state is resolved
            let mut guard = states
                .cached_state
                .get::<smithay::wayland::compositor::SurfaceAttributes>();
            let attrs = guard.current();

            // XXX
            println!("SUCCESS: Buffer received from application window in commit!");
            // Clone the active buffer handle reference out of the state layer
            if let Some(smithay::wayland::compositor::BufferAssignment::NewBuffer(wl_buffer)) = &attrs.buffer {
                committed_buffer = Some(wl_buffer.clone());
            }

            // This signals to Smithay that the compositor has processed the buffer contents
            // and can dispatch a wl_buffer.release() event back to the client.
            //attrs.buffer = None;  // Release back immediately to unblock client queue tracking bounds
            // XXX

            // Clear out pending frame callbacks to un-freeze the client
            std::mem::take(&mut attrs.frame_callbacks)
                .into_iter()
                .for_each(|callback| callback.done(time));
        });

        // XXX
        // TODO use dmabuf and VK_EXT_external_memory_dma_buf.
        if let Some(wl_buffer) = committed_buffer {
            // FIRST PATH: Attempt to capture direct hardware DMABUF graphics frames

            // Locate which window context matches this active surface commit
            let target_window_id = self
                .open_windows
                .iter()
                .find(|w| w.toplevel.wl_surface() == surface)
                .map(|w| w.id);

            if let Some(window_id) = target_window_id {
                if let Ok(dmabuf) = get_dmabuf(&wl_buffer) {
                    println!(
                        "🚀 ZERO-COPY SUCCESS! Received active DMABUF from client window. Dimensions: {}x{}",
                        dmabuf.width(),
                        dmabuf.height()
                    );
                    // Extract attributes from plane 0 structure configuration paths
                    let borrowed_fd = dmabuf.handles().next().expect("DMABUF missing handle 0");
                    let stride = dmabuf.strides().next().expect("DMABUF missing stride 0") as u32;
                    let width = dmabuf.width();
                    let height = dmabuf.height();
                    // Extract the actual fourcc format code requested by the client
                    let fourcc = dmabuf.format().code;
                    // Pass direct hardware file descriptor addresses down to Vulkan!
                    self.vulkan_engine.import_dmabuf_as_texture(
                        window_id,
                        borrowed_fd,
                        width,
                        height,
                        stride,
                        fourcc,
                    );
                } else {
                    // FALLBACK PATH: Keep legacy SHM code for basic CPU clients
                    println!("⚠️ FALLBACK: Client is forcing CPU SHM memory copies.");

                    let _ = smithay::wayland::shm::with_buffer_contents(
                        &wl_buffer,
                        |ptr, len, data| {
                            let pixel_slice: &[u8] =
                                unsafe { std::slice::from_raw_parts(ptr, len) };
                            let src_offset = if pixel_slice.len() > 134217728 {
                                134217728
                            } else {
                                0
                            };

                            println!(
                                "Live committed frame bytes entering engine: {:?}",
                                &pixel_slice[0..16]
                            );

                            if pixel_slice[src_offset..].iter().any(|&b| b != 0) {
                                self.vulkan_engine.process_client_pixels(
                                    window_id,
                                    &pixel_slice[src_offset..],
                                    data.width as u32,
                                    data.height as u32,
                                );
                            }
                        },
                    );
                }
            }
            // XXX: Release the frame handle back down the wire
            wl_buffer.release();
        }
    }
}

impl BufferHandler for CdeCompositorState {
    fn buffer_destroyed(&mut self, _buffer: &wayland_server::protocol::wl_buffer::WlBuffer) {
        // Handled when applications release window resources
    }
}

impl ShmHandler for CdeCompositorState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl smithay::wayland::dmabuf::DmabufHandler for CdeCompositorState {
    fn dmabuf_state(&mut self) -> &mut smithay::wayland::dmabuf::DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &smithay::wayland::dmabuf::DmabufGlobal,
        _dmabuf: smithay::backend::allocator::dmabuf::Dmabuf, // Missing parameter added here
        notifier: smithay::wayland::dmabuf::ImportNotifier,
    ) {
        // Triggers once a client buffer maps via hardware paths successfully.
        // For zero-copy path, we already intercept the DMABUF inside the `commit` hook.
        // Notify Smithay that the driver negotiation succeeded!
        // This validates the client's zwp_linux_buffer_params_v1 request layout.
        let _ = notifier.successful::<Self>();
        println!(
            "DMABUF verification completed successfully! Client is linked via hardware acceleration."
        );
    }
}

impl SessionLockHandler for CdeCompositorState {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.session_lock_state
    }

    // Fires when a client initiates a lock handshake
    fn lock(&mut self, lock: SessionLocker) {
        println!("🔒 Session lock initiated!");
        // Acknowledge the lock. It instructs the Wayland server
        // to inhibit all other normal shell application frames.
        lock.lock();
        // Cache the active locker token in state
        self.is_session_locked = true;
        self.password_length = 0;
        self.secure_password_buffer = SecretBox::new(Box::new(Vec::new()));

        // Clear keyboard input focus for security
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.seat
            .get_keyboard()
            .unwrap()
            .set_focus(self, None, serial);
    }

    // Fires when the lock screen surface client spawns a layer for a display output
    fn new_surface(&mut self, surface: LockSurface, _output: WlOutput) {
        println!("🖥️ Lock screen surface created for output");

        // Configure the surface geometry layout matching Vulkan/Winit display size
        let size = self.vulkan_engine.swapchain_extent;
        surface.with_pending_state(|state| {
            state.size = Some(smithay::utils::Size::from((
                size.width as u32,
                size.height as u32,
            )));
        });
        surface.send_configure();

        // Direct absolute keyboard input focus onto the secure lock surface sheet
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.seat.get_keyboard().unwrap().set_focus(
            self,
            Some(surface.wl_surface().clone()),
            serial,
        );
    }

    // Fires once a valid unlock passphrase or sequence completes successfully
    fn unlock(&mut self) {
        println!("🔓 Session unlocked. Restoring desktop frame loops.");
        self.is_session_locked = false;
    }
}

impl OutputHandler for CdeCompositorState {}

impl SelectionHandler for CdeCompositorState {
    type SelectionUserData = ();
}
impl ClientDndGrabHandler for CdeCompositorState {}
impl ServerDndGrabHandler for CdeCompositorState {}

impl DataDeviceHandler for CdeCompositorState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}
// Implement the mandatory XdgShell protocol callbacks
impl XdgShellHandler for CdeCompositorState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, toplevel: ToplevelSurface) {
        // Triggers when a new client application launches (e.g., foot, alacritty)
        println!("New application window mapping request intercepted!");

        // Tell the application interior surface to draw at 700x500
        toplevel.with_pending_state(|state| {
            state.size = Some((700, 500).into());
        });
        // Send the standard protocol configure trigger event straight down to the client
        // This instructs the application that it is permitted to draw surfaces inside the compositor.
        //        toplevel.send_configure();
        toplevel.send_pending_configure();

        let surface = toplevel.wl_surface().clone();

        // Generate a simple unique sequential ID
        static WINDOW_ID_ALLOCATOR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
        let id = WINDOW_ID_ALLOCATOR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Factor the frame decorations into the window's total tracking size
        let overall_width = 700.0 + (CDE_BORDER_WIDTH * 2.0);
        let overall_height = 500.0 + CDE_BORDER_WIDTH + CDE_TITLEBAR_HEIGHT;

        // TODO(): Split cascade coordinate cal into own helper.
        // Calculate current raw coordinate offsets based on step indices
        let mut spawn_x = BASELINE_ORIGIN_X + (self.cascade_step_index as f32 * CDE_CASCADE_OFFSET_STEP);
        let mut spawn_y = BASELINE_ORIGIN_Y + (self.cascade_step_index as f32 * CDE_CASCADE_OFFSET_STEP);

        // Fetch monitor screen scale boundaries to enforce edge wrapping constraints
        let swapchain_extent = self.vulkan_engine.swapchain_extent;
        let screen_w = swapchain_extent.width as f32;
        let screen_h = swapchain_extent.height as f32;

        // Boundary constraints: Ensure the cascading blocks wrap cleanly if they hit the bottom dock panels margin edge
        let max_safe_y = screen_h - CDE_PANEL_HEIGHT - CDE_ICON_HEIGHT - overall_height - 40.0;
        let max_safe_x = screen_w - overall_width - 40.0;

        if spawn_y > max_safe_y || spawn_x > max_safe_x {
            self.cascade_step_index = 0;
            spawn_x = BASELINE_ORIGIN_X;
            spawn_y = BASELINE_ORIGIN_Y;
            println!("CDE Window Matrix: Cascade hit layout boundaries. Wrapping coordinates back to origin matrix index.");
        }
        self.cascade_step_index += 1;
        println!("CDE Window Matrix: Window ID {} parked at cascade coordinate index [({}, {})]", id, spawn_x, spawn_y);
        // TODO()^.

        // Register it in our CDE Z-order stacking list at coordinates (50, 50)
        self.open_windows.push(WindowContext {
            // TODO(): Make a new() impl for WindowContext or
            // Default::defaults().
            id,
            toplevel,
            x: spawn_x, // cascade spawned application coordinates
            y: spawn_y, // ..
            width: overall_width,
            height: overall_height,
            workspace: self.active_workspace,
            is_minimized: false,
        });

        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.seat.get_keyboard().unwrap().set_focus(
            self,
            Some(surface),
            serial,
        );
    }

    fn toplevel_destroyed(&mut self, toplevel: ToplevelSurface) {
        let surface = toplevel.wl_surface();

        // Find the index of the window matching this dead surface
        if let Some(index) = self
            .open_windows
            .iter()
            .position(|w| w.toplevel.wl_surface() == surface)
        {
            let dead_window = self.open_windows.remove(index);
            println!(
                "XDG Shell: Safely evicting decoupled Window context ID: {}",
                dead_window.id
            );

            // Explicitly clean out its texture allocation records inside Vulkan
            // now that the sockets are completely finalized on the OS layer
            let mut textures = self.vulkan_engine.window_textures.lock().unwrap();
            textures.remove(&dead_window.id);
        }
    }

    fn new_popup(&mut self, _popup: PopupSurface, _positioner: PositionerState) {
        // Triggers for pop-up UI components like right-click context dropdown menus
    }

    fn ack_configure(
        &mut self,
        surface: wayland_server::protocol::wl_surface::WlSurface,
        configure: smithay::wayland::shell::xdg::Configure,
    ) {
        if let smithay::wayland::shell::xdg::Configure::Toplevel(toplevel_configure) = configure {
            if let Some(window) = self
                .open_windows
                .iter_mut()
                .find(|w| w.toplevel.wl_surface() == &surface)
            {
                if let Some(new_size) = toplevel_configure.state.size {
                    // Update our bounding boxes factoring the 3D chiseled frames
                    window.width = new_size.w as f32 + (CDE_BORDER_WIDTH * 2.0);
                    window.height = new_size.h as f32 + CDE_BORDER_WIDTH + CDE_TITLEBAR_HEIGHT;
                }
            }
        }
    }

    fn reposition_request(
        &mut self,
        _: smithay::wayland::shell::xdg::PopupSurface,
        _: PositionerState,
        _: u32,
    ) {
        todo!()
    }
    fn grab(
        &mut self,
        _popup: PopupSurface,
        _seat: wayland_server::protocol::wl_seat::WlSeat,
        _serial: smithay::utils::Serial,
    ) {
        // Handle input grab allocation maps for nested application popup objects here.
        // Leaving this empty lets the windowing system default to standard window layers.
        todo!()
    }
}

impl XdgDecorationHandler for CdeCompositorState {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        // Force the client application to run in Server-Side Decoration mode!
        // This stops apps like 'foot' from spawning decoration subsurfaces.
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode::ServerSide);
        });

        // Notify the application window to synchronize its geometry
        toplevel.send_configure();
    }

    fn request_mode(
        &mut self,
        toplevel: ToplevelSurface,
        _mode: smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode,
    ) {
        // Acknowledge the mode selection by re-enforcing Server-Side configuration bounds
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode::ServerSide);
        });
        toplevel.send_configure();
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode::ServerSide);
        });
        toplevel.send_configure();
    }
}

impl SeatHandler for CdeCompositorState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface; // Added missing 0.7 touch type constraint

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {
        // Triggers when window context or chiseled border focus boundaries change
    }

    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        _image: smithay::input::pointer::CursorImageStatus,
    ) {
        // Triggers when a client sets a custom interactive cursor frame (e.g. resize arrows)
    }
}

pub struct ClientState {
    pub compositor_state: smithay::wayland::compositor::CompositorClientState,
}
impl wayland_server::backend::ClientData for ClientState {}

fn verify_current_user_inline_secure(secure_password: &SecretBox<Vec<u8>>) -> bool {
    let username = std::env::var("USER").unwrap_or_else(|_| "root".to_string());
    let raw_pwd_bytes = secure_password.expose_secret();

    if let Ok(password_str) = std::str::from_utf8(raw_pwd_bytes) {
        // NOTE: This will require the binary to have `setcap` privileges
        // (cap_dac_override) to read /etc/shadow as a rootless user.
        // sudo setcap cap_dac_override+ep ./target/debug/cde_compositor
        match pwcheck::pwcheck(&username, password_str) {
            pwcheck::PwcheckResult::Ok => true,
            pwcheck::PwcheckResult::WrongPassword => false,
            pwcheck::PwcheckResult::Err(err) => {
                println!("⚠️ Linux PAM backend threw an execution error: {err}");
                false
            }
        }
    } else {
        false
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop = EventLoop::<CdeCompositorState>::try_new()?;
    let display = Display::<CdeCompositorState>::new()?;
    let display_handle = display.handle();

    // Register global protocols into our Wayland socket
    let compositor_state = CompositorState::new::<CdeCompositorState>(&display_handle);
    let xdg_shell_state = XdgShellState::new::<CdeCompositorState>(&display_handle);
    let xdg_decoration_state = XdgDecorationState::new::<CdeCompositorState>(&display_handle);
    let shm_state = ShmState::new::<CdeCompositorState>(&display_handle, vec![]);
    let session_lock_state = SessionLockManagerState::new::<CdeCompositorState, _>(
        &display_handle,
        |_client| true, // Secure filter: allows all or specific client connections
    );
    let data_device_state = DataDeviceState::new::<CdeCompositorState>(&display_handle);

    let mut seat_state = SeatState::new();
    let mut seat = seat_state.new_wl_seat(&display_handle, "cde-seat");

    // Give our virtual seat mouse and keyboard capabilities
    seat.add_pointer();
    //seat.add_keyboard(Default::default(), 200, 25).unwrap();
    seat.add_keyboard(
        smithay::input::keyboard::XkbConfig {
            rules: "",
            layout: "us",
            variant: "",
            options: None,
            model: "",
        },
        200, // Keyboard Repeat Delay (ms)
        25,  // Keyboard Repeat Rate
    ).unwrap();

    let output_manager_state = OutputManagerState::new();
    // Define our primary virtual CDE display bounds matching standard Winit specs
    let main_output = Output::new(
        "CDE-Monitor-1".to_string(),
        PhysicalProperties {
            size: (340, 270).into(), // Physical dimension in mm
            subpixel: Subpixel::Unknown,
            make: "Retro".to_string(),
            model: "CDE CRT".to_string(),
        },
    );
    // Allocate the screen into our Wayland display global context layout registry
    main_output.create_global::<CdeCompositorState>(&display_handle);

    let (mut backend, winit_loop) = winit::init()?;
    let vulkan_engine = VulkanEngine::init(&backend);
    let mut dmabuf_state = smithay::wayland::dmabuf::DmabufState::new();

    // Define the formats Vulkan backend expects.
    // Advertise multiple memory layouts to cover both transparent and opaque windows
    let supported_formats = vec![
        // For transparent applications (browsers, alpha-blended containers)
        smithay::backend::allocator::Format {
            modifier: drm_fourcc::DrmModifier::Linear,
            code: drm_fourcc::DrmFourcc::Argb8888, // AR24
        },
        // For standard opaque applications (like foot terminal window instances)
        smithay::backend::allocator::Format {
            modifier: drm_fourcc::DrmModifier::Linear,
            code: drm_fourcc::DrmFourcc::Xrgb8888, // XR24
        },
        // A common alternative fallback hardware layout mapping
        smithay::backend::allocator::Format {
            modifier: drm_fourcc::DrmModifier::Linear,
            code: drm_fourcc::DrmFourcc::Bgrx8888, // BX24
        },
    ];
    let default_feedback = smithay::wayland::dmabuf::DmabufFeedbackBuilder::new(
        vulkan_engine.render_node_or_gpu_device_handle,
        supported_formats
    ).build().expect("Failed to build default DMABUF feedback layout context");

    // Build the global advertisement token linked to core socket registry
    let dmabuf_global = dmabuf_state.create_global_with_default_feedback::<CdeCompositorState>(
        &display_handle,
        &default_feedback,
    );

    // Update virtual monitor modes based on the Winit window physical resolution
    let winit_size = backend.window().inner_size();
    let mode = smithay::output::Mode {
        size: (winit_size.width as i32, winit_size.height as i32).into(),
        refresh: 60_000, // 60kHz
    };
    main_output.change_current_state(Some(mode), None, None, Some((0, 0).into()));

    let listening_socket = smithay::wayland::socket::ListeningSocketSource::new_auto()?;
    let socket_name = listening_socket
        .socket_name()
        .to_string_lossy()
        .into_owned();

    let loop_handle = event_loop.handle();
    let closure_socket_name = socket_name.clone();

    // Register Client Connectivity Hook
    loop_handle.insert_source(
        listening_socket,
        move |client_stream, _, compositor_state| {
            // Automatically link incoming app windows with internal display handle reference
            compositor_state
                .display_handle
                .insert_client(
                    client_stream,
                    Arc::new(ClientState {
                        compositor_state:
                            smithay::wayland::compositor::CompositorClientState::default(),
                    }),
                )
                .expect("Failed to link incoming client application process context");
        },
    )?;

    // Register Winit Display Event Stream directly to Calloop
    loop_handle.insert_source(winit_loop, move |event, _, state| {
        match event {
            WinitEvent::Resized { .. } => {
                // React to compositor container size adjustments
            }

            // Route classic mouse/keyboard operations down to windows
            WinitEvent::Input(input_event) => {
                use smithay::backend::input::Event;
                use smithay::backend::input::InputEvent;
                use smithay::backend::input::ButtonState;
                use smithay::backend::input::AbsolutePositionEvent;
                use smithay::backend::input::PointerButtonEvent;


                // ----------------===========================================
                // HOTKEY INTERCEPTOR: Super + L (Lock Session)
                // ------------------------------------------------===========
                if let InputEvent::Keyboard { event: key_event } = &input_event {
                    use smithay::backend::input::KeyState;
                    use smithay::backend::input::KeyboardKeyEvent;

                    if key_event.state() == KeyState::Pressed {
                        let key_code = key_event.key_code();
                        //let modifiers = state.seat.get_keyboard().unwrap().modifier_state();

                        /// When X11 was originally written, the developers needed to isolate
                        /// software applications from varying hardware protocols. To do this,
                        /// Xorg enforced an arbitrary rule: take the raw hardware/kernel
                        /// keycode and add exactly 8 to it.
                        const X11_KKEY: u32 = 37u32 + 8u32;
                        const X11_UKEY: u32 = 22u32 + 8u32;

                        // Check if Mod4 (Super/Windows key) is active
                        //if modifiers.logo {
                        // Switch from modifiers.logo to Ctrl + Alt to bypass GNOME's global intercepts
                        //if modifiers.ctrl && modifiers.alt {
                            // Super + L -> LOCK SESSIONS
                            //if key_code == smithay::backend::input::Keycode::from(38_u32) {
                            println!("🔒 Hotkey Activated: Ctrl+Alt pressed. {:?}", key_code);

                            // Ctrl + Alt + K -> LOCK SESSIONS (Changed to avoid WM conflicts)
                            if key_code == smithay::backend::input::Keycode::from(X11_KKEY) {
                                if !state.is_session_locked {
                                    println!("🔒 Hotkey Activated: Super+L triggered! Locking compositor session.");
                                    state.is_session_locked = true;
                                    // Drop application window focus for security
                                    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                                    state.seat.get_keyboard().unwrap().set_focus(state, None, serial);
                                    return; // Absorb event
                                }
                            }

                            // Super + Shift + U -> UNLOCK SEED FOR BACKDOOR TESTING
                            //if modifiers.shift && key_code == smithay::backend::input::Keycode::from(22_u32) {

                            // Ctrl + Alt + U -> UNLOCK SEED FOR BACKDOOR TESTING (Scan code 22_u32)
                            if key_code == smithay::backend::input::Keycode::from(X11_UKEY) {
                                if state.is_session_locked {
                                    println!("🔓 Backdoor Hotkey Activated: Super+Shift+U triggered! Restoring session.");
                                    state.is_session_locked = false;
                                    return; // Absorb event
                                }
                            }
                        //}
                    }
                }
                // ----------------===========================================
                // SECURE INPUT ROUTING barrier IF LOCKED
                // ------------------------------------------------===========
                if state.is_session_locked {
                    match input_event {
                        InputEvent::Keyboard { event: key_event } => {
                            use smithay::backend::input::KeyState;
                            use smithay::backend::input::KeyboardKeyEvent;

                            if key_event.state() == KeyState::Pressed {
                                let key_code = key_event.key_code();
                                // Shifted raw X11 code mapping constraints
                                const X11_BACKSPACE: u32 = 22u32; // Standard Backspace evdev keycode + 8
                                const X11_ENTER: u32     = 36u32;

                                // CASE 1: BACKSPACE CLICKED
                                if key_code == smithay::backend::input::Keycode::from(X11_BACKSPACE) {
                                    if state.password_length > 0 {
                                        // De-encapsulate, pop item, and drop old traces instantly
                                        let mut temp_vec = state.secure_password_buffer.expose_secret().clone();
                                        temp_vec.pop();

                                        state.secure_password_buffer = SecretBox::new(Box::new(temp_vec));
                                        state.password_length = state.secure_password_buffer.expose_secret().len() as u32;
                                    }
                                }
                                // CASE 2: ENTER CLICKED -> RUN AUTHENTICATION
                                else if key_code == smithay::backend::input::Keycode::from(X11_ENTER) {
                                    println!("🔑 Verifying password via secure inline secrecy cells...");

                                    let is_valid = verify_current_user_inline_secure(&state.secure_password_buffer);

                                    // Wipe password traces completely out of memory allocation frames
                                    state.secure_password_buffer = SecretBox::new(Box::new(Vec::new()));
                                    state.password_length = 0;

                                    if is_valid {
                                        println!("✅ Access Granted!");
                                        state.is_session_locked = false;
                                    } else {
                                        println!("❌ Access Denied!");
                                    }
                                }
                                // CASE 3: CHARACTER TYPED
                                else {
                                    // Limit string length to 16 to fit nicely inside the input widget bounds
                                    if state.password_length < 16 {
                                        let raw_code: u32 = key_code.into();
                                        let typed_char = match raw_code {
                                            11 => '1', 12 => '2', 13 => '3', 14 => '4', 15 => '5',
                                            16 => '6', 17 => '7', 18 => '8', 19 => '9', 20 => '0',
                                            24 => 'q', 25 => 'w', 26 => 'e', 27 => 'r', 28 => 't',
                                            38 => 'a', 39 => 's', 40 => 'd', 41 => 'f', 42 => 'g',
                                            _ => 'x',
                                        };

                                        let mut temp_vec = state.secure_password_buffer.expose_secret().clone();
                                        temp_vec.push(typed_char as u8);

                                        state.secure_password_buffer = SecretBox::new(Box::new(temp_vec));
                                        state.password_length = state.secure_password_buffer.expose_secret().len() as u32;
                                    }
                                }
                            }
                        }
                        _ => {
                            // Drop pointer motion and mouse clicks completely so
                            // background windows cannot be manipulated through the lock screen.
                        }
                    }
                    return; // Short-circuit out of the standard input handler loop!
                }

                match input_event {
                    InputEvent::PointerMotionAbsolute { event } => {
                        // Track absolute mouse screen space movements
                        let location = event.position();
                        state.pointer_location = (location.x, location.y);

                        // Pass motion vectors down to Smithay's virtual seat engine
                        state.seat.get_pointer().unwrap().motion(
                            state,
                            None,
                            &smithay::input::pointer::MotionEvent {
                                location: smithay::utils::Point::from((location.x, location.y)),
                                serial: smithay::utils::SERIAL_COUNTER.next_serial(),
                                time: event.time() as u32,
                            },
                        );

                        // If we are actively dragging a window, calculate its new coordinate offsets
                        if let GrabState::Dragging { window_index, start_mouse, start_window } = state.current_grab {
                            let dx = (location.x - start_mouse.0) as f32;
                            let dy = (location.y - start_mouse.1) as f32;

                            if let Some(window) = state.open_windows.get_mut(window_index) {
                                window.x = start_window.0 + dx;
                                window.y = start_window.1 + dy;
                            }
                        } else if let GrabState::Resizing { window_index, edge, start_mouse, start_window_pos, start_window_size } = state.current_grab {
                            let dx = (location.x - start_mouse.0) as f32;
                            let dy = (location.y - start_mouse.1) as f32;

                            if let Some(window) = state.open_windows.get_mut(window_index) {
                                let mut new_x = window.x;
                                let mut new_y = window.y;
                                let mut new_w = window.width;
                                let mut new_h = window.height;

                                // Apply dimensional mapping based on which edge vector is pulled
                                match edge {
                                    ResizeEdge::Right => { new_w = start_window_size.0 + dx; }
                                    ResizeEdge::Bottom => { new_h = start_window_size.1 + dy; }
                                    ResizeEdge::Left => {
                                        new_x = start_window_pos.0 + dx;
                                        new_w = start_window_size.0 - dx;
                                    }
                                    ResizeEdge::Top => {
                                        new_y = start_window_pos.1 + dy;
                                        new_h = start_window_size.1 - dy;
                                    }
                                    ResizeEdge::BottomRight => {
                                        new_w = start_window_size.0 + dx;
                                        new_h = start_window_size.1 + dy;
                                    }
                                    ResizeEdge::BottomLeft => {
                                        new_x = start_window_pos.0 + dx;
                                        new_w = start_window_size.0 - dx;
                                        new_h = start_window_size.1 + dy;
                                    }
                                    ResizeEdge::TopRight => {
                                        new_y = start_window_pos.1 + dy;
                                        new_w = start_window_size.0 + dx;
                                        new_h = start_window_size.1 - dy;
                                    }
                                    ResizeEdge::TopLeft => {
                                        new_x = start_window_pos.0 + dx;
                                        new_y = start_window_pos.1 + dy;
                                        new_w = start_window_size.0 - dx;
                                        new_h = start_window_size.1 - dy;
                                    }
                                }

                                // Enforce basic minimum size bounds so windows don't collapse into inversion errors
                                if new_w > 150.0 && new_h > 100.0 {
                                    window.x = new_x;
                                    window.y = new_y;
                                    window.width = new_w;
                                    window.height = new_h;

                                    // Subtract our chiseled frame decorations padding before requesting down to Wayland client
                                    let inner_w = new_w - (crate::CDE_BORDER_WIDTH * 2.0);
                                    let inner_h = new_h - (crate::CDE_BORDER_WIDTH + crate::CDE_TITLEBAR_HEIGHT);

                                    // Instruct the application client surface to natively resize its hardware buffers
                                    window.toplevel.with_pending_state(|state| {
                                        state.size = Some((inner_w as i32, inner_h as i32).into());
                                    });
                                    window.toplevel.send_pending_configure();
                                }
                            }
                        }
                    }
                    InputEvent::PointerButton { event } => {
                        use smithay::backend::input::MouseButton;

                        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                        let button_state = event.state();

                        // Pass click configurations down to focus handlers
                        state.seat.get_pointer().unwrap().button(
                            state,
                            &smithay::input::pointer::ButtonEvent {
                                button: event.button_code(),
                                state: button_state,
                                serial,
                                time: event.time() as u32,
                            },
                        );

                        if event.button() == Some(MouseButton::Left) {
                            match button_state {
                                ButtonState::Pressed => {
                                    let mx = state.pointer_location.0 as f32;
                                    let my = state.pointer_location.1 as f32;

                                    // ActiveMenu Dropdown
                                    if let Some(menu) = state.active_menu {
                                        if mx >= menu.x && mx <= (menu.x + menu.width) &&
                                           my >= menu.y && my <= (menu.y + menu.height) {

                                            // Map Y offset to the targeted row index
                                            let relative_y = my - menu.y;
                                            let clicked_row = (relative_y / CDE_MENU_ROW_HEIGHT).floor() as u32;

                                            println!("CDE Menu Hook: Clicked Row Index [{}] on Window ID {}", clicked_row, menu.window_id);

                                            // Execute classic structural actions natively
                                            if let Some(w_idx) = state.open_windows.iter().position(|w| w.id == menu.window_id) {
                                                match clicked_row {
                                                    CDE_MENU_ITEM_RESTORE => {
                                                        let win = &mut state.open_windows[w_idx];
                                                        if win.is_minimized {
                                                            println!("CDE Layout Engine: Restoring Window ID {} from icon state", menu.window_id);
                                                            win.is_minimized = false;

                                                            // Restore its 700x500 geometry mapping boundaries
                                                            win.width = 700.0 + (CDE_BORDER_WIDTH * 2.0);
                                                            win.height = 500.0 + CDE_BORDER_WIDTH + CDE_TITLEBAR_HEIGHT;

                                                            win.toplevel.with_pending_state(|xdg_state| {
                                                                xdg_state.size = Some((700, 500).into());
                                                            });
                                                            win.toplevel.send_pending_configure();
                                                        }
                                                    }
                                                    CDE_MENU_ITEM_MINIMIZE => {
                                                        // Fallback into your existing button_type 2 minimization queue
                                                        state.current_grab = GrabState::ButtonPressed { window_index: w_idx, button_type: 2 };
                                                    }
                                                    CDE_MENU_ITEM_LOWER => {
                                                        // Lower: Shift target window to index 0 (absolute bottom of Z-stack)
                                                        let target_win = state.open_windows.remove(w_idx);
                                                        state.open_windows.insert(0, target_win);
                                                        println!("CDE Layout Engine: Lowered Window Context ID {} to base of Z-stack.", menu.window_id);
                                                    }
                                                    CDE_MENU_ITEM_CLOSE => {
                                                        if let Some(window) = state.open_windows.get(w_idx) {
                                                            window.toplevel.send_close();
                                                        }
                                                    }
                                                    _ => {} // Restore/Size placeholder steps
                                                }
                                            }
                                            state.active_menu = None; // Dimiss menu after an item is clicked
                                            return; // Intercepted: absorb click sequence step completely
                                        } else {
                                            // Clicked completely outside the active dropdown: Dismiss it instantly
                                            state.active_menu = None;
                                        }
                                    }

                                    // Get current screen window dimensions from the Vulkan swapchain context
                                    let screen_w = state.vulkan_engine.swapchain_extent.width as f32;
                                    let screen_h = state.vulkan_engine.swapchain_extent.height as f32;

                                    // Replicate the exact Front Panel metrics from vk_engine.rs
                                    let panel_x = (screen_w - CDE_PANEL_WIDTH) / 2.0;
                                    let panel_y = screen_h - CDE_PANEL_HEIGHT - 10.0;

                                    // FRONT PANEL DOCK INTERACTION INTERCEPTOR.
                                    if mx >= panel_x && mx <= (panel_x + CDE_PANEL_WIDTH) &&
                                       my >= panel_y && my <= (panel_y + CDE_PANEL_HEIGHT) {

                                        // Calculate boundaries for the central Workspace Switcher box
                                        let switch_left = panel_x + (CDE_PANEL_WIDTH / 2.0) - 50.0;
                                        let switch_right = panel_x + (CDE_PANEL_WIDTH / 2.0) + 50.0;
                                        let switch_top = panel_y + 8.0;
                                        let switch_bottom = panel_y + CDE_PANEL_HEIGHT - 8.0;

                                        if mx >= switch_left && mx <= switch_right &&
                                           my >= switch_top && my <= switch_bottom {

                                            let bx = mx - switch_left;
                                            let by = my - switch_top;
                                            let eth = 2.0;

                                            let bw = switch_right - switch_left;
                                            let bh = switch_bottom - switch_top;

                                            let inner_bw = bw - (eth * 2.0);
                                            let inner_bh = bh - (eth * 2.0);

                                            // Calculate cell column and row sizes
                                            let col_w = inner_bw / 2.0;
                                            let row_h = inner_bh / 2.0;

                                            let local_x = bx - eth;
                                            let local_y = by - eth;

                                            let col_idx = (local_x / col_w).floor() as i32;
                                            let row_idx = (local_y / row_h).floor() as i32;

                                            // Clamp cell coordinates
                                            let col_idx = col_idx.clamp(0, 1);
                                            let row_idx = row_idx.clamp(0, 1);

                                            // Resolve total grid target room index
                                            let clicked_workspace = (row_idx * 2) + col_idx;

                                            if clicked_workspace >= 0 && clicked_workspace < 4 {
                                                println!("Stacked Quad CDE Switcher: Switch to Room [{}]", clicked_workspace + 1);
                                                state.active_workspace = clicked_workspace as u32;
                                            }
                                        } else {
                                            // Calculate exact coordinates relative to the panel's left edge layout bounds
                                            let panel_local_x = mx - panel_x;
                                            let panel_local_y = my - panel_y;

                                            // Matches shader button boundaries: left=20.0, right=64.0, top=10.0, bottom=54.0
                                            // Left Button Bounds: 20.0 to 64.0
                                            if panel_local_x >= 20.0 && panel_local_x <= 64.0 &&
                                               panel_local_y >= 10.0 && panel_local_y <= 54.0 {
                                            // User clicked on the wings of the front panel dock!
                                            // Let's implement an absolute launch action if they clicked the left wing area.
                                            //if mx < switch_left {
                                                println!("CDE Shortcut Activated: Spawning Retro Terminal Console Application.");
                                                std::process::Command::new("foot")
                                                    .env("WAYLAND_DISPLAY", &closure_socket_name) // Inject live engine display socket!
                                                    .spawn().expect("Failed to execute client process thread");
                                            } else if panel_local_x >= 536.0 && panel_local_x <= 580.0 &&
                                                      panel_local_y >= 10.0 && panel_local_y <= 54.0 {
                                                // Right Button Bounds (600.0 - 64.0 = 536.0, and 600.0 - 20.0 = 580.0) TODO Use constants!
                                                println!("CDE Trash Clicked: Spawning File Manager.");
                                                // You can replace "pcmanfm" with "nautilus", "thunar", or a custom terminal runner
                                                std::process::Command::new("pcmanfm")
                                                    .env("WAYLAND_DISPLAY", &closure_socket_name) // Inject live engine display socket!
                                                    .spawn().expect("Failed to execute file manager client shortcut process");
                                            }
                                        }
                                        return; // Intercepted: skip trailing window tests entirely!
                                    }

                                    // Locate which window was clicked under Z-order stacking rules
                                    if let Some(w_idx) = state.open_windows.iter().position(|w| {
                                        if w.workspace != state.active_workspace { return false; }

                                        if w.is_minimized {
                                            // Enforce strict 64x64 icon box bounds to prevent phantom collisions
                                            mx >= w.x && mx <= (w.x + CDE_ICON_WIDTH) &&
                                                my >= w.y && my <= (w.y + CDE_ICON_HEIGHT)
                                        } else {
                                            // Regular open windows evaluate standard titlebar decoration metrics
                                            mx >= w.x && mx <= (w.x + w.width) &&
                                            my >= w.y && my <= (w.y + w.height)
                                        }
                                    }) {
                                        let window = &state.open_windows[w_idx];

                                        state.current_grab = GrabState::Dragging {
                                            window_index: w_idx,
                                            start_mouse: (mx.into(), my.into()),
                                            start_window: (window.x, window.y),
                                        };

                                        // Elevate the clicked window/icon to the top of the focus stack
                                        let target = state.open_windows.remove(w_idx);
                                        state.open_windows.push(target);
                                    }

                                    let mut hit_window = None;
                                    let mut closed_a_window = false;

                                    // Search backwards through open windows (Top-to-Bottom in Z-order stacking)
                                    for (index, window) in state.open_windows.iter().enumerate().rev() {
                                        // Skip click target calculation if the window is hidden on another pane or already an icon
                                        if window.workspace != state.active_workspace || window.is_minimized {
                                            continue;
                                        }

                                        // Evaluate if click hits within window bounding coordinates
                                        if mx >= window.x && mx <= (window.x + window.width) &&
                                           my >= window.y && my <= (window.y + window.height) {
                                                // Close Button location bounds
                                                let close_btn_x = window.x + window.width - CDE_BORDER_WIDTH - CDE_BUTTON_OFFSET - CDE_BUTTON_SIZE;
                                                let btn_y = window.y + ((CDE_TITLEBAR_HEIGHT - CDE_BUTTON_SIZE) / 2.0);

                                                // Minimize Button location bounds
                                                let min_btn_x = window.x + window.width - CDE_BORDER_WIDTH - CDE_MIN_BUTTON_OFFSET - CDE_BUTTON_SIZE;
                                                let menu_btn_x = window.x + CDE_BORDER_WIDTH + CDE_BUTTON_OFFSET;

                                                // HOVERING CLOSE BUTTON -> ENGAGE PRESSED LOOK
                                                if mx >= close_btn_x && mx <= (close_btn_x + CDE_BUTTON_SIZE) &&
                                                   my >= btn_y && my <= (btn_y + CDE_BUTTON_SIZE) {
                                                    state.current_grab = GrabState::ButtonPressed { window_index: index, button_type: 1 };
                                                    // Evict the structural tracking object from the layout stack
                                                    closed_a_window = true;
                                                    break;
                                                }

                                                // HOVERING MINIMIZE BUTTON -> ENGAGE PRESSED LOOK
                                                if mx >= min_btn_x && mx <= (min_btn_x + CDE_BUTTON_SIZE) &&
                                                   my >= btn_y && my <= (btn_y + CDE_BUTTON_SIZE) {
                                                    state.current_grab = GrabState::ButtonPressed { window_index: index, button_type: 2 };
                                                    closed_a_window = true;
                                                    break;
                                                }

                                                // HOVERING MENU DASH BUTTON -> ENGAGE PRESSED LOOK
                                                if mx >= menu_btn_x && mx <= (menu_btn_x + CDE_BUTTON_SIZE) &&
                                                   my >= btn_y && my <= (btn_y + CDE_BUTTON_SIZE) {
                                                    state.current_grab = GrabState::ButtonPressed { window_index: index, button_type: 3 };
                                                    closed_a_window = true;
                                                    break;
                                                }

                                                // Only allow window dragging if the user clicks inside the titlebar area
                                                // (between top of window and TITLEBAR_HEIGHT)
                                                if my <= (window.y + CDE_TITLEBAR_HEIGHT) {
                                                    hit_window = Some(index);
                                                } else if let Some(edge) = window.get_resize_edge(mx, my) {
                                                    // Check if mouse clicks on outer chiseled frame handles.
                                                    // Evict focus, bring window to front
                                                    state.seat.get_keyboard().unwrap().set_focus(state, None, serial);
                                                    let grabbed_window = state.open_windows.remove(index);
                                                    state.open_windows.push(grabbed_window);

                                                    let active_idx = state.open_windows.len() - 1;
                                                    let target = &state.open_windows[active_idx];

                                                    state.current_grab = GrabState::Resizing {
                                                        window_index: active_idx,
                                                        edge,
                                                        start_mouse: state.pointer_location,
                                                        start_window_pos: (target.x, target.y),
                                                        start_window_size: (target.width, target.height),
                                                    };

                                                    state.seat.get_keyboard().unwrap().set_focus(
                                                        state, Some(target.toplevel.wl_surface().clone()), serial,
                                                    );
                                                    closed_a_window = true; // Signals interception completed
                                                    break;
                                                }
                                                break;
                                        }
                                    }

                                    // If the button wasn't clicked, proceed to front-pop layer and engage dragging
                                    if !closed_a_window {
                                        if let Some(index) = hit_window {
                                            // Clear keyboard focus from everything else first
                                            state.seat.get_keyboard().unwrap().set_focus(state, None, serial);

                                            // Bring window to front of Z-order array stack and activate the grab
                                            let window = state.open_windows.remove(index);
                                            state.open_windows.push(window);
                                            let active_index = state.open_windows.len() - 1;
                                            let target = &state.open_windows[active_index];

                                            state.current_grab = GrabState::Dragging {
                                                window_index: active_index,
                                                start_mouse: state.pointer_location,
                                                start_window: (target.x, target.y),
                                            };

                                            // Set active focus rules
                                            state.seat.get_keyboard().unwrap().set_focus(
                                                state,
                                                Some(target.toplevel.wl_surface().clone()),
                                                serial,
                                            );
                                        }
                                    }
                                }
                                ButtonState::Released => {
                                    let screen_h = state.vulkan_engine.swapchain_extent.height as f32;

                                    // Dispatch tasks from a completed click sequence
                                    if let GrabState::ButtonPressed { window_index, button_type } = state.current_grab {
                                        if let Some(window) = state.open_windows.get(window_index) {
                                            match button_type {
                                                1 => {
                                                    if let Some(window) = state.open_windows.get(window_index) {
                                                        window.toplevel.send_close();
                                                    }
                                                }
                                                2 => {
                                                    // Desktop Grid Auto-Layout Engine Logic
                                                    //println!("Minimizing Window context sequence completed.");
                                                    println!("CDE Engine Command: Minimizing Window Context ID: {}", window.id);
                                                    // Calculate the read-only immutable count metric FIRST
                                                    let minimized_count = state.open_windows
                                                        .iter()
                                                        .filter(|w| w.is_minimized && w.workspace == state.active_workspace)
                                                        .count();

                                                    // Open the mutable borrow AFTER the read loop has finished
                                                    if let Some(target_win) = state.open_windows.get_mut(window_index) {
                                                        target_win.is_minimized = true;
                                                        // Line up the icons horizontally at the bottom using the clean variable
                                                        // Line up icons horizontally with a 10px spacing buffer step
                                                        target_win.x = 20.0 + (minimized_count as f32 * (CDE_ICON_WIDTH + 10.0));
                                                        // Parks neatly right above the front panel deck bar
                                                        target_win.y = screen_h - CDE_PANEL_HEIGHT - CDE_ICON_HEIGHT - 30.0;
                                                        println!("CDE Engine Command: Window ID {} minimized down to slot [{}].", target_win.id, minimized_count);
                                                    }

                                                    // Drop keyboard focus
                                                    state.seat.get_keyboard().unwrap().set_focus(state, None, serial);
                                                }
                                                3 => {
                                                    println!("CDE Menu Hook: Spawning Dropdown Menu Box under Window ID {}", window.id);

                                                    // Position the menu panel directly underneath the top-left titlebar edge arena
                                                    let menu_w = 120.0; // Canonical compact Motif menu box width
                                                    let menu_h = CDE_MENU_ROW_HEIGHT * 4.0; // 4 active action rows

                                                    state.active_menu = Some(ActiveMenu {
                                                        window_id: window.id,
                                                        x: window.x + CDE_BORDER_WIDTH,
                                                        y: window.y + CDE_TITLEBAR_HEIGHT,
                                                        width: menu_w,
                                                        height: menu_h,
                                                        item_count: 4,
                                                    });
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    if let GrabState::Dragging { window_index, start_mouse, .. } = state.current_grab {
                                        let mx = state.pointer_location.0 as f64;
                                        let my = state.pointer_location.1 as f64;

                                        // Calculate if the user simply clicked or actually dragged the item
                                        let delta_x = (mx - start_mouse.0).abs();
                                        let delta_y = (my - start_mouse.1).abs();
                                        let is_stationary_click = delta_x < 3.0 && delta_y < 3.0;

                                        if let Some(window) = state.open_windows.get(window_index) {
                                            if window.is_minimized && is_stationary_click {
                                                // Clicking a minimized icon pops up its operations menu!
                                                println!("CDE Menu Hook: Spawning Dropdown Menu Box under Icon Box ID {}", window.id);

                                                let menu_w = 120.0;
                                                let menu_h = (CDE_MENU_ROW_HEIGHT * 4.0) + 4.0;

                                                state.active_menu = Some(ActiveMenu {
                                                    window_id: window.id,
                                                    // Spawn the menu directly above or next to the icon box bounds
                                                    x: window.x,
                                                    y: window.y - menu_h,
                                                    width: menu_w,
                                                    height: menu_h,
                                                    item_count: 4,
                                                });
                                            }
                                        }
                                    }

                                    // Reset interactive state on release
                                    state.current_grab = GrabState::Idle;

                                    // TODO move into vk as GC helper.
                                    // Evict dead surface handles from GPU memory
                                    let mut active_textures = state.vulkan_engine.window_textures.lock().unwrap();

                                    // Retain only texture cache models whose companion window IDs are still physically open
                                    active_textures.retain(|&id, _| {
                                        let is_still_alive = state.open_windows.iter().any(|w| w.id == id);
                                        if !is_still_alive {
                                            // Fallback cleanup step if window tracker vanished from main stack mid-frame
                                            println!("Evicting destroyed window ID allocation mapping from GPU VRAM: {}", id);
                                        }
                                        is_still_alive
                                    });
                                }
                            }
                        }
                    }
                    InputEvent::Keyboard { event: key_event } => {
                        // Intercept raw keyboard hardware device updates from Winit
                        use smithay::backend::input::KeyState;
                        use smithay::backend::input::KeyboardKeyEvent;

                        let keyboard = state.seat.get_keyboard().unwrap();
                        let serial = smithay::utils::SERIAL_COUNTER.next_serial();

                        // Look up the exact millisecond timing parameter for Wayland protocol compliance
                        // Create a simple, robust timestamp in milliseconds from an Instant boot baseline
                        static BOOT_TIME: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
                        let boot = BOOT_TIME.get_or_init(std::time::Instant::now);
                        let time = boot.elapsed().as_millis() as u32;

                        // Extract raw hardware scan code tokens via the KeyboardKeyEvent trait methods
                        let key_code = key_event.key_code();
                        let key_state = key_event.state();

                        // Convert key press state indicators cleanly
                        let state_enum = match key_state {
                            KeyState::Pressed => smithay::backend::input::KeyState::Pressed,
                            KeyState::Released => smithay::backend::input::KeyState::Released,
                        };

                        // Forward input to Smithay's keyboard layout engine.
                        keyboard.input::<(), _>(
                            state,
                            key_code,
                            state_enum,
                            serial,
                            time,

                            |_, _, _| {
                                smithay::input::keyboard::FilterResult::Forward
                            },
                        );
                    }
                    _ => {}
                }
            }
            WinitEvent::Redraw => {
                // Bring TraversalAction explicitly into scope
//                use smithay::wayland::compositor::TraversalAction;

        //        for window in &state.open_windows {
        //            let surface = window.toplevel.wl_surface();

        //            smithay::wayland::compositor::with_states(surface, |states| {
        //                let mut guard = states
        //                    .cached_state
        //                    .get::<smithay::wayland::compositor::SurfaceAttributes>();
        //                let attrs = guard.current(); // XXX .pending() is already cleared..
//                    let root_surface = window.toplevel.wl_surface();
//
//                    smithay::wayland::compositor::with_surface_tree_upward(
//                        root_surface,
//                        (), // Initial state tracking user-data token
//                        // Pre-order traversal closure
//                        |surface, states, _| {
//                            let mut guard = states
//                                .cached_state
//                                .get::<smithay::wayland::compositor::SurfaceAttributes>(
//                            );
//                            let attrs = guard.current();

                            // Check if the application has committed a valid image frame buffer
        //                    if let Some(
        //                        smithay::wayland::compositor::BufferAssignment::NewBuffer(
        //                            wl_buffer,
        //                        ),
        //                    ) = &attrs.buffer
        //                    {
        //                        println!("SUCCESS: Buffer received from application window!");
        //                        // Safely read the raw shared memory (SHM) bytes of the running application window
        //                        let _ = smithay::wayland::shm::with_buffer_contents(
        //                            wl_buffer,
        //                            |ptr, len, data| {
        //                                let pixel_slice: &[u8] =
        //                                    unsafe { std::slice::from_raw_parts(ptr, len) };

        //                                // Only copy and process if the buffer isn't completely empty
        //                                if pixel_slice.iter().any(|&b| b != 0) {
        //                                    println!(
        //                                        "First 16 live bytes: {:?}",
        //                                        &pixel_slice[0..16]
        //                                    );
        //                                    // Send the pixel bytes, size metrics, and window coordinates directly to Vulkan loop!
        //                                    state.vulkan_engine.process_client_pixels(
        //                                        pixel_slice,
        //                                        data.width as u32,
        //                                        data.height as u32,
        //                                    );
        //                                    // EXECUTE DRAW: Paint this specific window frame using its active width and height attributes!
        //                                    //let active_cmd_buf = state.vulkan_engine.command_buffers[0];
        //                                    //state.vulkan_engine.draw_element(
        //                                    //    active_cmd_buf,
        //                                    //    data.width as f32,
        //                                    //    data.height as f32,
        //                                    //);
        //                                }
        //                            },
        //                        );
        //                    }

        //                    // Unconditionally return the expected TraversalAction token
        //                    // telling Smithay to continue visiting subsurfaces!
//      //                      TraversalAction::DoChildren(())
        //                },
//      //                  // Post-order traversal closure
//      //                  |_, _, _| {},
//      //                  // Subsurface Filter Closure
//      //                  |_, _, _| true,
        //            );
        //        }

                // Execute unified chiseled CDE border drawing pass loop sequentially
                state.vulkan_engine.draw_frame(
                    &state.open_windows,
                    state.current_grab,
                    state.active_workspace,
                    state.is_session_locked,
                    state.password_length,
                    state.active_menu
                );
                backend.bind().unwrap();
            }
            _ => {}
        }
    })?;

    let mut state = CdeCompositorState {
        display_handle: display_handle,
        compositor_state,
        xdg_shell_state,
        xdg_decoration_state,
        data_device_state,
        shm_state,
        dmabuf_state,
        dmabuf_global,
        output_manager_state,
        main_output,
        seat_state,
        seat,
        vulkan_engine,
        open_windows: Vec::new(),
        cascade_step_index: 0,
        pointer_location: (0.0, 0.0),
        current_grab: GrabState::Idle,
        active_workspace: 0,
        active_menu: None,
        session_lock_state,
        is_session_locked: false,
        password_length: 0,
        secure_password_buffer: SecretBox::new(Box::new(Vec::new())),
    };

    // Expose the local display server socket path so apps know how to talk to us
    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", &socket_name);
    };
    println!(
        "CDE Wayland Compositor online! Listening on socket: {:?}",
        socket_name
    );

    let mut display = display;
    // Keep Event Loop Alive while consistently flushing display pipelines :/
    loop {
        // Run any Calloop tasks (like drawing timelines or catching mouse focus changes)
        // Set timeout to None or a minimal duration to prevent locking the network socket
        event_loop.dispatch(std::time::Duration::from_millis(1), &mut state)?;
        // XXX figure out how to let the CPU sleep more without freezing the screen?
        //event_loop.dispatch(None, &mut state)?;

        // Drive Vulkan rendering pipeline at the display's native pace.
        // Because this loop runs continuously, open windows (like weston-simple-egl)
        // are repainted on every single cycle at full speed without needing mouse movement.
        state.vulkan_engine.draw_frame(
            &state.open_windows,
            state.current_grab,
            state.active_workspace,
            state.is_session_locked,
            state.password_length,
            state.active_menu,
        );

        // Natively drain and parse incoming client data on the thread context directly!
        // This solves the immutable borrow problem from the event source closure wrapper.
        display
            .dispatch_clients(&mut state)
            .expect("Failed to dispatch Wayland streams");

        // Flush out configuration tokens to the socket layout stream
        let _ = state.display_handle.flush_clients();

        // Run Calloop system events with a strict 0-timeout so it NEVER blocks the thread
        event_loop.dispatch(std::time::Duration::ZERO, &mut state)?;

        // Minimal frame throttling step to keep the loop from spinning at 100% CPU
        // when completely idle, while still remaining fully responsive.
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
