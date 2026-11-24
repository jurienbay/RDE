use std::process::Command;

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

const BUTTON_COUNT: u32 = 4;
const BUTTON_SIZE: u32 = 44;
const PADDING: u32 = 4;
const BAR_W: u32 = (BUTTON_COUNT * BUTTON_SIZE) + ((BUTTON_COUNT + 1) * PADDING);
const BAR_H: u32 = BUTTON_SIZE + (PADDING * 2);

struct FvwmButtonsApp {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    pointer: Option<wl_pointer::WlPointer>,
    configured: bool,
    exit: bool,
    pressed_button: Option<u32>,
    active_workspace: u32,
    pager_manager: Option<ZfvwmPagerManagerV1>,
}

impl FvwmButtonsApp {
    fn button_at(&self, px: f64, py: f64) -> Option<u32> {
        let y0 = PADDING as f64;
        let y1 = (PADDING + BUTTON_SIZE) as f64;
        if py < y0 || py > y1 {
            return None;
        }
        for i in 0..BUTTON_COUNT {
            let x0 = (PADDING + i * (BUTTON_SIZE + PADDING)) as f64;
            let x1 = x0 + BUTTON_SIZE as f64;
            if px >= x0 && px <= x1 {
                return Some(i);
            }
        }
        None
    }

    fn trigger_action(&mut self, index: u32) {
        match index {
            0 => {
                println!("FvwmButtons: Launching foot terminal");
                let _ = Command::new("foot").spawn();
            }
            1 => {
                println!("FvwmButtons: Launching pcmanfm file manager");
                let _ = Command::new("pcmanfm").spawn();
            }
            2 => {
                let next_ws = (self.active_workspace + 1) % 4;
                println!("FvwmButtons: Cycling workspace -> {}", next_ws);
                if let Some(ref mgr) = self.pager_manager {
                    mgr.set_workspace(next_ws);
                }
            }
            3 => {
                println!("FvwmButtons: Exiting module");
                self.exit = true;
            }
            _ => {}
        }
    }

    fn draw(&mut self, _qh: &QueueHandle<Self>) {
        if !self.configured {
            return;
        }

        let stride = BAR_W as i32 * 4;
        let (buffer, canvas) = self
            .pool
            .create_buffer(BAR_W as i32, BAR_H as i32, stride, wl_shm::Format::Argb8888)
            .expect("Failed to allocate SHM buffer");

        // Classic Motif / FVWM Gray Palette (BGRA little-endian)
        let face_gray: [u8; 4] = [0xB8, 0xB8, 0xB8, 0xFF];
        let btn_gray: [u8; 4] = [0xC6, 0xC6, 0xC6, 0xFF];
        let btn_pressed: [u8; 4] = [0x9E, 0x9E, 0x9E, 0xFF];
        let highlight: [u8; 4] = [0xF2, 0xF2, 0xF2, 0xFF];
        let shadow: [u8; 4] = [0x63, 0x63, 0x63, 0xFF];
        let etch_black: [u8; 4] = [0x18, 0x18, 0x18, 0xFF];

        // Distinct classic icon accent colors per button
        let icon_colors: [[u8; 4]; 4] = [
            [0x22, 0x22, 0x22, 0xFF], // Terminal screen (dark charcoal)
            [0x55, 0xA8, 0xE0, 0xFF], // Folder icon (classic Motif amber/gold)
            [0xA6, 0x6E, 0x4A, 0xFF], // Pager/Desk switcher (slate blue)
            [0x44, 0x44, 0xB8, 0xFF], // Exit button (muted crimson)
        ];

        // 1. Fill outer FvwmButtons frame with 2px raised bevel
        let b = 2u32;
        for y in 0..BAR_H {
            for x in 0..BAR_W {
                let pixel = if x < b || y < b {
                    highlight
                } else if x >= BAR_W - b || y >= BAR_H - b {
                    shadow
                } else {
                    face_gray
                };
                let offset = ((y * BAR_W + x) * 4) as usize;
                canvas[offset..offset + 4].copy_from_slice(&pixel);
            }
        }

        // 2. Draw each 3D beveled button and a simple centered retro glyph
        for i in 0..BUTTON_COUNT {
            let bx0 = PADDING + i * (BUTTON_SIZE + PADDING);
            let by0 = PADDING;
            let is_down = self.pressed_button == Some(i);
            let (top_left, bot_right, bg) = if is_down {
                (shadow, highlight, btn_pressed)
            } else {
                (highlight, shadow, btn_gray)
            };

            for dy in 0..BUTTON_SIZE {
                for dx in 0..BUTTON_SIZE {
                    let pixel =
                        if dx == 0 || dy == 0 || dx == BUTTON_SIZE - 1 || dy == BUTTON_SIZE - 1 {
                            etch_black
                        } else if dx <= 2 || dy <= 2 {
                            top_left
                        } else if dx >= BUTTON_SIZE - 3 || dy >= BUTTON_SIZE - 3 {
                            bot_right
                        } else {
                            bg
                        };
                    let offset = (((by0 + dy) * BAR_W + (bx0 + dx)) * 4) as usize;
                    canvas[offset..offset + 4].copy_from_slice(&pixel);
                }
            }

            // Draw a crisp 16x14 mini-icon inside the button
            let shift = if is_down { 1 } else { 0 };
            let ix0 = bx0 + 14 + shift;
            let iy0 = by0 + 15 + shift;
            for iy in 0..14u32 {
                for ix in 0..16u32 {
                    let draw_pixel = match i {
                        // Button 0: Terminal box with prompt '>'
                        0 => {
                            ix == 0
                                || ix == 15
                                || iy == 0
                                || iy == 13
                                || (iy >= 3 && iy <= 9 && ix >= 3 && ix <= 12)
                        }
                        // Button 1: Folder tab + body
                        1 => (iy < 3 && ix >= 1 && ix <= 7) || (iy >= 3 && ix >= 1 && ix <= 14),
                        // Button 2: 2x2 Workspace grid icon
                        2 => ix == 0 || ix == 7 || ix == 15 || iy == 0 || iy == 6 || iy == 13,
                        // Button 3: 'X' close/quit glyph
                        3 => {
                            (ix as i32 - iy as i32).abs() <= 1
                                || ((14 - ix as i32) - iy as i32).abs() <= 1
                        }
                        _ => false,
                    };
                    if draw_pixel {
                        let offset = (((iy0 + iy) * BAR_W + (ix0 + ix)) * 4) as usize;
                        canvas[offset..offset + 4].copy_from_slice(&icon_colors[i as usize]);
                    }
                }
            }
        }

        let wl_surface = self.layer.wl_surface();
        wl_surface.damage_buffer(0, 0, BAR_W as i32, BAR_H as i32);
        buffer.attach_to(wl_surface).expect("Buffer attach failed");
        wl_surface.commit();
    }
}

impl CompositorHandler for FvwmButtonsApp {
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

impl OutputHandler for FvwmButtonsApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for FvwmButtonsApp {
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

impl SeatHandler for FvwmButtonsApp {
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

impl PointerHandler for FvwmButtonsApp {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            match event.kind {
                PointerEventKind::Press { button: 0x110, .. } => {
                    let (px, py) = event.position;
                    self.pressed_button = self.button_at(px, py);
                    self.draw(qh);
                }
                PointerEventKind::Release { button: 0x110, .. } => {
                    let (px, py) = event.position;
                    if let Some(pressed) = self.pressed_button.take() {
                        if self.button_at(px, py) == Some(pressed) {
                            self.trigger_action(pressed);
                        }
                    }
                    self.draw(qh);
                }
                PointerEventKind::Leave { .. } => {
                    if self.pressed_button.take().is_some() {
                        self.draw(qh);
                    }
                }
                _ => {}
            }
        }
    }
}

impl ShmHandler for FvwmButtonsApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for FvwmButtonsApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

impl wayland_client::Dispatch<ZfvwmPagerManagerV1, ()> for FvwmButtonsApp {
    fn event(
        state: &mut Self,
        _proxy: &ZfvwmPagerManagerV1,
        event: zfvwm_pager_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let zfvwm_pager_manager_v1::Event::WorkspaceChanged { workspace } = event {
            state.active_workspace = workspace;
        }
    }
}

delegate_registry!(FvwmButtonsApp);
smithay_client_toolkit::delegate_dispatch2!(FvwmButtonsApp);

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
        layer_shell.create_layer_surface(&qh, surface, Layer::Top, Some("fvwm-buttons"), None);
    layer.set_anchor(Anchor::BOTTOM | Anchor::LEFT);
    layer.set_size(BAR_W, BAR_H);
    layer.set_margin(0, 0, 12, 12);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();
    let pool =
        SlotPool::new((BAR_W * BAR_H * 4 * 2) as usize, &shm).expect("Failed to create SHM pool");
    let mut app = FvwmButtonsApp {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        layer,
        pointer: None,
        configured: false,
        exit: false,
        pressed_button: None,
        active_workspace: 0,
        pager_manager,
    };
    while !app.exit {
        event_queue
            .blocking_dispatch(&mut app)
            .expect("Wayland dispatch failed");
    }
}
