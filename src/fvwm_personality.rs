use ash::vk;
use smithay::backend::input::MouseButton;
use smithay::utils::SERIAL_COUNTER;

use crate::constants::*;
use crate::personality::{DesktopPersonality, ScreenEdge};
use crate::vk_engine::VulkanEngine;
use crate::CdeCompositorState;

pub struct FvwmPersonality;

impl DesktopPersonality for FvwmPersonality {
    fn name(&self) -> &'static str { "FVWM" }

    fn clear_color(&self, _active_workspace: u32) -> [f32; 4] {
        [0.18, 0.25, 0.28, 1.0]
    }

    fn on_pointer_hover(&self, state: &mut CdeCompositorState, mx: f32, my: f32) {
        if !state.config.sloppy_focus { return; }

        // Sloppy Focus: focus topmost window under cursor without raising it
        if let Some(hovered) = state.open_windows.iter().rev().find(|w| {
            w.workspace == state.active_workspace
                && !w.is_minimized
                && mx >= w.x
                && mx <= w.x + w.width
                && my >= w.y
                && my <= w.y + if w.is_shaded { CDE_TITLEBAR_HEIGHT } else { w.height }
        }) {
            let surf = hovered.toplevel.wl_surface().clone();
            let serial = SERIAL_COUNTER.next_serial();
            state.seat.get_keyboard().unwrap().set_focus(state, Some(surf), serial);
        }
    }

    fn on_edge_hit(&self, state: &mut CdeCompositorState, edge: ScreenEdge) {
        // Edge Panning: Flip across the 2x2 workspace grid when hitting screen edges
        let can_pan = state
            .last_edge_pan
            .map_or(true, |t| t.elapsed().as_millis() > state.config.edge_pan_cooldown_ms);
        if !can_pan {
            return;
        }

        let mut col = (state.active_workspace % 2) as i32;
        let mut row = (state.active_workspace / 2) as i32;

        match edge {
            ScreenEdge::Left if col > 0 => col -= 1,
            ScreenEdge::Right if col < 1 => col += 1,
            ScreenEdge::Top if row > 0 => row -= 1,
            ScreenEdge::Bottom if row < 1 => row += 1,
            _ => return,
        }

        state.active_workspace = ((row * 2) + col) as u32;
        state.last_edge_pan = Some(std::time::Instant::now());
        state.pager_protocol_state.notify_workspace_changed(state.active_workspace);
    }

    fn is_root_canvas(&self, state: &CdeCompositorState, mx: f32, my: f32) -> bool {
        !state.hits_any_window(mx, my)
    }

    fn on_root_click(&self, state: &mut CdeCompositorState, button: MouseButton, mx: f32, my: f32) {
        match button {
            MouseButton::Left | MouseButton::Right => state.open_root_apps_menu(mx, my),
            MouseButton::Middle => state.open_window_list_menu(mx, my),
            _ => {}
        }
    }

    fn on_chrome_click(
        &self,
        _state: &mut CdeCompositorState,
        _mx: f32,
        _my: f32,
        _socket_name: &str,
    ) -> bool {
        false
    }

    fn on_titlebar_double_click(&self, state: &mut CdeCompositorState, window_index: usize) -> bool {
        state.open_windows[window_index].is_shaded = !state.open_windows[window_index].is_shaded;
        state.sync_pager_windows();
        true
    }

    fn render_chrome(
        &self,
        _engine: &VulkanEngine,
        _cmd_buf: vk::CommandBuffer,
        _screen_w: f32,
        _screen_h: f32,
        _active_workspace: u32,
    ) {
        // Rendered externally by the fvwm-pager layer-shell module
    }
}
