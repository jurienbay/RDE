use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },

    shell::{
        xdg::{
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgShell,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
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

struct IdentData {
    id: u32,
    title: String,
    app_id: String,
    workspace: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    flags: u32,
}

struct FvwmIdentApp {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    xdg_shell: XdgShell,
    shm: Shm,
    pool: SlotPool,
    window: Option<Window>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    width: u32,
    height: u32,
    configured: bool,
    exit: bool,
    ident: Option<IdentData>,
}

/// Compact 5x7 bitmap glyphs for uppercase ASCII, digits, and punctuation
fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        ':' => [0x00, 0x04, 0x04, 0x00, 0x04, 0x04, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '+' => [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        '/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        '|' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        _ => [0x00; 7],
    }
}

fn draw_text(
    canvas: &mut [u8],
    buf_w: u32,
    buf_h: u32,
    x0: u32,
    y0: u32,
    text: &str,
    color: [u8; 4],
) {
    for (ci, ch) in text.chars().enumerate() {
        let gx = x0 + (ci as u32 * 6);
        if gx + 5 >= buf_w {
            break;
        }
        let rows = glyph_5x7(ch);
        for (ry, &bits) in rows.iter().enumerate() {
            let py = y0 + ry as u32;
            if py >= buf_h {
                break;
            }
            for rx in 0..5u32 {
                if (bits & (1 << (4 - rx))) != 0 {
                    let offset = ((py * buf_w + gx + rx) * 4) as usize;
                    canvas[offset..offset + 4].copy_from_slice(&color);
                }
            }
        }
    }
}

impl FvwmIdentApp {
    fn open_ident_window(&mut self, qh: &QueueHandle<Self>) {
        if self.window.is_some() {
            return;
        }
        let surface = self.compositor.create_surface(qh);
        let win = self
            .xdg_shell
            .create_window(surface, WindowDecorations::ServerDefault, qh);
        win.set_title("FvwmIdent");
        win.set_app_id("fvwm-ident");
        win.set_min_size(Some((self.width, self.height)));
        win.set_max_size(Some((self.width, self.height)));
        win.commit();
        self.window = Some(win);
    }

    fn draw(&mut self) {
        if !self.configured {
            return;
        }
        let (Some(win), Some(info)) = (&self.window, &self.ident) else {
            return;
        };

        let w = self.width;
        let h = self.height;
        let stride = w as i32 * 4;
        let (buffer, canvas) = self
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
            .expect("Failed to allocate SHM buffer");

        // Classic Motif / FVWM Gray & Navy Text Palette (BGRA)
        let bg_gray: [u8; 4] = [0xC2, 0xC2, 0xC2, 0xFF];
        let inset_bg: [u8; 4] = [0xAE, 0xAE, 0xAE, 0xFF];
        let highlight: [u8; 4] = [0xF2, 0xF2, 0xF2, 0xFF];
        let shadow: [u8; 4] = [0x63, 0x63, 0x63, 0xFF];
        let text_black: [u8; 4] = [0x10, 0x10, 0x10, 0xFF];
        let label_navy: [u8; 4] = [0x7A, 0x3A, 0x1A, 0xFF];

        // Fill background + sunken inner panel
        for y in 0..h {
            for x in 0..w {
                let in_box = x >= 8 && x < w - 8 && y >= 8 && y < h - 24;
                let color = if in_box {
                    if x < 10 || y < 10 {
                        shadow
                    } else if x >= w - 10 || y >= h - 26 {
                        highlight
                    } else {
                        inset_bg
                    }
                } else {
                    bg_gray
                };
                let offset = ((y * w + x) * 4) as usize;
                canvas[offset..offset + 4].copy_from_slice(&color);
            }
        }

        let mut flag_parts = Vec::new();
        if (info.flags & 1) != 0 {
            flag_parts.push("FOCUSED");
        }
        if (info.flags & 2) != 0 {
            flag_parts.push("SHADED");
        }
        if (info.flags & 4) != 0 {
            flag_parts.push("ICONIC");
        }
        let flag_str = if flag_parts.is_empty() {
            "NORMAL".to_string()
        } else {
            flag_parts.join(" | ")
        };

        let rows = [
            ("TITLE:", info.title.clone()),
            ("APP ID:", info.app_id.clone()),
            ("WIN ID:", format!("{}", info.id)),
            (
                "DESK:",
                format!(
                    "{} (COL {}, ROW {})",
                    info.workspace,
                    info.workspace % 2,
                    info.workspace / 2
                ),
            ),
            (
                "GEOMETRY:",
                format!("{}X{}+{}+{}", info.width, info.height, info.x, info.y),
            ),
            ("FLAGS:", flag_str),
        ];

        for (i, (label, val)) in rows.iter().enumerate() {
            let y = 18 + (i as u32 * 22);
            draw_text(canvas, w, h, 16, y, label, label_navy);
            draw_text(canvas, w, h, 82, y, val, text_black);
        }

        draw_text(
            canvas,
            w,
            h,
            16,
            h - 16,
            "CLICK OR PRESS ESC TO CLOSE",
            shadow,
        );

        let wl_surface = win.wl_surface();
        wl_surface.damage_buffer(0, 0, w as i32, h as i32);
        buffer.attach_to(wl_surface).expect("Buffer attach failed");
        wl_surface.commit();
    }
}

impl CompositorHandler for FvwmIdentApp {
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
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
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

impl OutputHandler for FvwmIdentApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl WindowHandler for FvwmIdentApp {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        if let (Some(w), Some(h)) = configure.new_size {
            self.width = w.get();
            self.height = h.get();
        }
        self.configured = true;
        self.draw();
    }
}

impl SeatHandler for FvwmIdentApp {
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
            self.pointer = Some(self.seat_state.get_pointer(qh, &seat).unwrap());
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = Some(self.seat_state.get_keyboard(qh, &seat, None).unwrap());
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
        if capability == Capability::Keyboard {
            if let Some(k) = self.keyboard.take() {
                k.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for FvwmIdentApp {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if let PointerEventKind::Press { .. } = event.kind {
                self.exit = true;
            }
        }
    }
}

impl KeyboardHandler for FvwmIdentApp {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if event.keysym == Keysym::Escape
            || event.keysym == Keysym::Return
            || event.keysym == Keysym::q
        {
            self.exit = true;
        }
    }
    fn repeat_key(
    &mut self,
    _: &Connection,
    _: &QueueHandle<Self>,
    _: &wl_keyboard::WlKeyboard,
    _: u32,
    _: KeyEvent,
    ) {}

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
    }
}

impl ShmHandler for FvwmIdentApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for FvwmIdentApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

impl wayland_client::Dispatch<ZfvwmPagerManagerV1, ()> for FvwmIdentApp {
    fn event(
        state: &mut Self,
        _proxy: &ZfvwmPagerManagerV1,
        event: zfvwm_pager_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let zfvwm_pager_manager_v1::Event::WindowIdent {
            id,
            title,
            app_id,
            workspace,
            x,
            y,
            width,
            height,
            flags,
        } = event
        {
            state.ident = Some(IdentData {
                id,
                title,
                app_id,
                workspace,
                x,
                y,
                width,
                height,
                flags,
            });
            state.open_ident_window(qh);
            // Wait for WindowHandler::configure before drawing so buffer dimensions match!
            //state.draw();
        }
    }
}

delegate_registry!(FvwmIdentApp);
smithay_client_toolkit::delegate_dispatch2!(FvwmIdentApp);

fn main() {
    let conn = Connection::connect_to_env().expect("Failed to connect to WAYLAND_DISPLAY");
    let (globals, mut event_queue) = registry_queue_init(&conn).expect("Registry init failed");
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor unavailable");
    let xdg_shell = XdgShell::bind(&globals, &qh).expect("xdg_wm_base unavailable");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm unavailable");
    let pager_manager = globals
        .bind::<ZfvwmPagerManagerV1, _, _>(&qh, 1..=1, ())
        .expect("zfvwm_pager_manager_v1 unavailable");
    // Request interactive window selection from the compositor
    pager_manager.pick_window();
    let pool =
        SlotPool::new((400 * 250 * 4 * 2) as usize, &shm).expect("Failed to create SHM pool");
    let mut app = FvwmIdentApp {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        compositor,
        xdg_shell,
        shm,
        pool,
        window: None,
        pointer: None,
        keyboard: None,
        width: 340,
        height: 185,
        configured: false,
        exit: false,
        ident: None,
    };
    while !app.exit {
        event_queue
            .blocking_dispatch(&mut app)
            .expect("Wayland dispatch failed");
    }
}
