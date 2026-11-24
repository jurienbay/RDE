use std::collections::HashMap;

//use smithay_client_toolkit::compositor::FrameCallbackData;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

mod pager_proto {
    use wayland_client;

    pub mod __interfaces {
        use wayland_client::backend as wayland_backend;
        wayland_scanner::generate_interfaces!("protocols/fvwm_pager_v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/fvwm_pager_v1.xml");
}

use pager_proto::zfvwm_pager_manager_v1::{self, ZfvwmPagerManagerV1};

const PAGER_W: u32 = 160;
const PAGER_H: u32 = 120;

struct MiniWindow {
    workspace: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    is_focused: bool,
}

struct FvwmPagerApp {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    pointer: Option<wl_pointer::WlPointer>,
    configured: bool,
    exit: bool,
    active_page: u32,
    pager_manager: Option<ZfvwmPagerManagerV1>,
    screen_w: u32,
    screen_h: u32,
    windows: std::collections::HashMap<u32, MiniWindow>,
}

impl FvwmPagerApp {
    fn draw(&mut self, _qh: &QueueHandle<Self>) {
        if !self.configured {
            return;
        }

        let stride = PAGER_W as i32 * 4;
        let (buffer, canvas) = self
            .pool
            .create_buffer(
                PAGER_W as i32,
                PAGER_H as i32,
                stride,
                wl_shm::Format::Argb8888,
            )
            .expect("Failed to allocate SHM buffer");

        // BGRA little-endian pixel colors matching your GLSL shader palette
        let shadow: [u8; 4] = [0x6B, 0x6B, 0x6B, 0xFF];
        let highlight: [u8; 4] = [0xF2, 0xF2, 0xF2, 0xFF];
        let etch_black: [u8; 4] = [0x00, 0x00, 0x00, 0xFF];
        let active_bg: [u8; 4] = [0xA6, 0x6E, 0x4A, 0xFF]; // CDE/FVWM Slate Blue
        let inactive_bg: [u8; 4] = [0x61, 0x57, 0x52, 0xFF]; // Dark Pager Slate

        let b = 2u32;
        let inner_w = PAGER_W - (b * 2);
        let inner_h = PAGER_H - (b * 2);
        let col_w = inner_w / 2;
        let row_h = inner_h / 2;

        for y in 0..PAGER_H {
            for x in 0..PAGER_W {
                let pixel = if x < b || y < b {
                    shadow
                } else if x >= PAGER_W - b || y >= PAGER_H - b {
                    highlight
                } else {
                    let lx = x - b;
                    let ly = y - b;
                    let col = (lx / col_w).min(1);
                    let row = (ly / row_h).min(1);
                    let cell = (row * 2) + col;
                    let cx = lx % col_w;
                    let cy = ly % row_h;

                    if cx == 0 || cy == 0 {
                        etch_black
                    } else if cell == self.active_page {
                        active_bg
                    } else {
                        inactive_bg
                    }
                };

                let offset = ((y * PAGER_W + x) * 4) as usize;
                canvas[offset..offset + 4].copy_from_slice(&pixel);
            }
        }

        let win_focused: [u8; 4] = [0xD9, 0xD9, 0xD9, 0xFF];   // Bright classic FVWM mini-window
        let win_unfocused: [u8; 4] = [0x99, 0x99, 0x99, 0xFF]; // Muted gray mini-window
        let scale_x = col_w as f32 / self.screen_w as f32;
        let scale_y = row_h as f32 / self.screen_h as f32;

        for win in self.windows.values() {
            if win.workspace >= 4 {
                continue;
            }
            let q_col = win.workspace % 2;
            let q_row = win.workspace / 2;
            let q_x0 = (b + q_col * col_w) as i32;
            let q_y0 = (b + q_row * row_h) as i32;
            let q_x1 = q_x0 + col_w as i32;
            let q_y1 = q_y0 + row_h as i32;

            let mx0 = q_x0 + (win.x as f32 * scale_x) as i32;
            let my0 = q_y0 + (win.y as f32 * scale_y) as i32;
            let mw = ((win.width as f32 * scale_x) as i32).max(4);
            let mh = ((win.height as f32 * scale_y) as i32).max(4);
            let mx1 = mx0 + mw;
            let my1 = my0 + mh;

            let fill = if win.is_focused { win_focused } else { win_unfocused };

            for py in my0.max(q_y0 + 1)..my1.min(q_y1) {
                for px in mx0.max(q_x0 + 1)..mx1.min(q_x1) {
                    let is_edge = px == mx0 || px == mx1 - 1 || py == my0 || py == my1 - 1;
                    let color = if is_edge { etch_black } else { fill };
                    let offset = ((py as u32 * PAGER_W + px as u32) * 4) as usize;
                    canvas[offset..offset + 4].copy_from_slice(&color);
                }
            }
        }

        let wl_surface = self.layer.wl_surface();
        wl_surface.damage_buffer(0, 0, PAGER_W as i32, PAGER_H as i32);
        // don't draw every vsync
        //wl_surface.frame(qh, FrameCallbackData(wl_surface.clone()));
        buffer.attach_to(wl_surface).expect("Buffer attach failed");
        wl_surface.commit();
    }
}

impl CompositorHandler for FvwmPagerApp {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.draw(qh);
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

impl OutputHandler for FvwmPagerApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for FvwmPagerApp {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &LayerSurface,
        _configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        self.configured = true;
        self.draw(qh);
    }
}

impl SeatHandler for FvwmPagerApp {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = Some(
                self.seat_state
                    .get_pointer(qh, &seat)
                    .expect("Failed to get pointer"),
            );
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for FvwmPagerApp {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if let PointerEventKind::Press { button: 0x110, .. } = event.kind {
                let (px, py) = event.position;
                let col = ((px / (PAGER_W as f64 / 2.0)).floor() as u32).clamp(0, 1);
                let row = ((py / (PAGER_H as f64 / 2.0)).floor() as u32).clamp(0, 1);
                self.active_page = (row * 2) + col;
                println!(
                    "External FvwmPager clicked -> Page ({}, {}) [Workspace {}]",
                    col, row, self.active_page
                );
                if let Some(ref mgr) = self.pager_manager {
                    mgr.set_workspace(self.active_page);
                }
                self.draw(qh);
            }
        }
    }
}

impl ShmHandler for FvwmPagerApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for FvwmPagerApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

impl wayland_client::Dispatch<ZfvwmPagerManagerV1, ()> for FvwmPagerApp {
    fn event(
        state: &mut Self,
        _proxy: &ZfvwmPagerManagerV1,
        event: zfvwm_pager_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zfvwm_pager_manager_v1::Event::WorkspaceChanged { workspace } => {
                state.active_page = workspace;
                state.draw(qh);
            }
            zfvwm_pager_manager_v1::Event::ScreenSize { width, height } => {
                state.screen_w = width.max(1);
                state.screen_h = height.max(1);
            }
            zfvwm_pager_manager_v1::Event::WindowUpdate {
                id,
                workspace,
                x,
                y,
                width,
                height,
                is_focused,
            } => {
                state.windows.insert(
                    id,
                    MiniWindow {
                        workspace,
                        x,
                        y,
                        width,
                        height,
                        is_focused: is_focused != 0,
                    },
                );
            }
            zfvwm_pager_manager_v1::Event::WindowRemove { id } => {
                state.windows.remove(&id);
            }
            zfvwm_pager_manager_v1::Event::WindowIdent { .. } => todo!(),
            zfvwm_pager_manager_v1::Event::Done => {
                state.draw(qh);
            }
        }
    }
}

delegate_registry!(FvwmPagerApp);
smithay_client_toolkit::delegate_dispatch2!(FvwmPagerApp);

fn main() {
    let conn = Connection::connect_to_env().expect("Failed to connect to WAYLAND_DISPLAY");
    let (globals, mut event_queue) = registry_queue_init(&conn).expect("Registry init failed");
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor unavailable");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("zwlr_layer_shell_v1 unavailable");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm unavailable");
    let pager_manager = globals
        .bind::<ZfvwmPagerManagerV1, _, _>(&qh, 1..=1, ())
        .ok();

    let surface = compositor.create_surface(&qh);
    let layer =
        layer_shell.create_layer_surface(&qh, surface, Layer::Top, Some("fvwm-pager"), None);

    layer.set_anchor(Anchor::BOTTOM | Anchor::RIGHT);
    layer.set_size(PAGER_W, PAGER_H);
    layer.set_margin(0, 12, 12, 0);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();

    let pool = SlotPool::new((PAGER_W * PAGER_H * 4 * 2) as usize, &shm)
        .expect("Failed to create SHM pool");

    let mut app = FvwmPagerApp {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        layer,
        pointer: None,
        configured: false,
        exit: false,
        active_page: 0,
        pager_manager,
        screen_w: 1280, screen_h: 800,
        windows: HashMap::new(),
    };

    while !app.exit {
        event_queue
            .blocking_dispatch(&mut app)
            .expect("Wayland dispatch failed");
    }
}
