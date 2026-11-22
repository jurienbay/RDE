use ash::vk;
use smithay::backend::input::MouseButton;

use crate::constants::*;
use crate::personality::{DesktopPersonality, ScreenEdge};
use crate::vk_engine::VulkanEngine;
use crate::CdeCompositorState;

pub struct CdePersonality;

impl DesktopPersonality for CdePersonality {
    fn name(&self) -> &'static str { "CDE" }

    fn clear_color(&self, active_workspace: u32) -> [f32; 4] {
        // Classic 1990s CDE Desktop Teal Clear color palette
        // Authentic 1996 CDE Room Palette Scheme Matrix
        match active_workspace {
            0 => [0.00, 0.47, 0.47, 1.0], // Room One: Canonical Desktop Teal
            1 => [0.40, 0.20, 0.30, 1.0], // Room Two: Classic Motif Maroon
            2 => [0.25, 0.35, 0.45, 1.0], // Room Three: Workstation Slate Blue
            3 => [0.30, 0.30, 0.35, 1.0], // Room Four: Unix Terminal Charcoal
            _ => [0.00, 0.47, 0.47, 1.0], // Fallback
        }
    }

    fn on_pointer_hover(&self, _state: &mut CdeCompositorState, _mx: f32, _my: f32) {
        // CDE uses click-to-focus by default
    }

    fn on_edge_hit(&self, _state: &mut CdeCompositorState, _edge: ScreenEdge) {
        // CDE does not pan workspaces at screen edges
    }

    fn is_root_canvas(&self, state: &CdeCompositorState, mx: f32, my: f32) -> bool {
        let screen_w = state.vulkan_engine.swapchain_extent.width as f32;
        let screen_h = state.vulkan_engine.swapchain_extent.height as f32;
        let panel_x = (screen_w - CDE_PANEL_WIDTH) / 2.0;
        let panel_y = screen_h - CDE_PANEL_HEIGHT - 10.0;

        if mx >= panel_x && mx <= panel_x + CDE_PANEL_WIDTH
            && my >= panel_y && my <= panel_y + CDE_PANEL_HEIGHT
        {
            return false;
        }
        !state.hits_any_window(mx, my)
    }

    fn on_root_click(&self, state: &mut CdeCompositorState, button: MouseButton, mx: f32, my: f32) {
        if button == MouseButton::Right {
            state.open_root_apps_menu(mx, my);
        }
    }

    fn on_chrome_click(
        &self,
        state: &mut CdeCompositorState,
        mx: f32,
        my: f32,
        socket_name: &str,
    ) -> bool {
        // Move the Front Panel switcher + launcher hit-test logic from main.rs here
        state.handle_cde_panel_click(mx, my, socket_name)
    }

    fn on_titlebar_double_click(&self, _state: &mut CdeCompositorState, _window_index: usize) -> bool {
        false
    }

    fn render_chrome(
        &self,
        engine: &VulkanEngine,
        cmd_buf: vk::CommandBuffer,
        screen_w: f32,
        screen_h: f32,
        active_workspace: u32,
        _has_external_pager: bool,
    ) {
        engine.draw_cde_front_panel(cmd_buf, screen_w, screen_h, active_workspace);
    }
}
