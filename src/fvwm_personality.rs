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
        if !state.has_external_pager() {
            let screen_w = state.vulkan_engine.swapchain_extent.width as f32;
            let screen_h = state.vulkan_engine.swapchain_extent.height as f32;
            let pager_x = screen_w - FVWM_PAGER_WIDTH - 12.0;
            let pager_y = screen_h - FVWM_PAGER_HEIGHT - 12.0;

            if mx >= pager_x && mx <= pager_x + FVWM_PAGER_WIDTH
                && my >= pager_y && my <= pager_y + FVWM_PAGER_HEIGHT
            {
                return false;
            }
        }
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
        state: &mut CdeCompositorState,
        mx: f32,
        my: f32,
        _socket_name: &str,
    ) -> bool {
        if state.has_external_pager() {
            return false;
        }
        let screen_w = state.vulkan_engine.swapchain_extent.width as f32;
        let screen_h = state.vulkan_engine.swapchain_extent.height as f32;
        let pager_x = screen_w - FVWM_PAGER_WIDTH - 12.0;
        let pager_y = screen_h - FVWM_PAGER_HEIGHT - 12.0;

        if mx >= pager_x && mx <= pager_x + FVWM_PAGER_WIDTH
            && my >= pager_y && my <= pager_y + FVWM_PAGER_HEIGHT
        {
            let col = (((mx - pager_x) / (FVWM_PAGER_WIDTH / 2.0)).floor() as u32).clamp(0, 1);
            let row = (((my - pager_y) / (FVWM_PAGER_HEIGHT / 2.0)).floor() as u32).clamp(0, 1);
            state.active_workspace = (row * 2) + col;
            return true;
        }
        false
    }

    fn on_titlebar_double_click(&self, state: &mut CdeCompositorState, window_index: usize) -> bool {
        state.open_windows[window_index].is_shaded = !state.open_windows[window_index].is_shaded;
        state.sync_pager_windows();
        true
    }

    fn render_chrome(
        &self,
        engine: &VulkanEngine,
        cmd_buf: vk::CommandBuffer,
        screen_w: f32,
        screen_h: f32,
        active_workspace: u32,
        has_external_pager: bool,
    ) {
        if has_external_pager {
            return;
        }
        let pager_x = screen_w - FVWM_PAGER_WIDTH - 12.0;
        let pager_y = screen_h - FVWM_PAGER_HEIGHT - 12.0;
        engine.draw_procedural_ui_element(
            cmd_buf,
            screen_h,
            screen_w,
            (pager_x, pager_y, FVWM_PAGER_WIDTH, FVWM_PAGER_HEIGHT),
            7u32 | (active_workspace << 4),
        );
    }
}
