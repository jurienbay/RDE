use ash::vk;
use smithay::backend::input::MouseButton;

use crate::vk_engine::VulkanEngine;
use crate::CdeCompositorState;

#[derive(Debug, Clone, Copy)]
pub enum ScreenEdge {
    Left,
    Right,
    Top,
    Bottom,
}

pub trait DesktopPersonality {
    /// Name of the personality
    fn name(&self) -> &'static str;

    /// Background clear color for the active workspace/page.
    fn clear_color(&self, active_workspace: u32) -> [f32; 4];

    /// Called on pointer motion when no grab is active (e.g., Sloppy Focus in FVWM).
    fn on_pointer_hover(&self, state: &mut CdeCompositorState, mx: f32, my: f32);

    /// Called when the cursor hits a 1px screen boundary.
    fn on_edge_hit(&self, state: &mut CdeCompositorState, edge: ScreenEdge);

    /// Returns true if (mx, my) is on the bare desktop (not a window, icon, panel, or pager).
    fn is_root_canvas(&self, state: &CdeCompositorState, mx: f32, my: f32) -> bool;

    /// Handles clicks on the bare desktop background.
    fn on_root_click(&self, state: &mut CdeCompositorState, button: MouseButton, mx: f32, my: f32);

    /// Handles clicks on the panel or pager. Returns true if the click was consumed.
    fn on_chrome_click(
        &self,
        state: &mut CdeCompositorState,
        mx: f32,
        my: f32,
        socket_name: &str,
    ) -> bool;

    /// Handles titlebar double-clicks (e.g., WindowShade in FVWM). Returns true if consumed.
    fn on_titlebar_double_click(&self, state: &mut CdeCompositorState, window_index: usize) -> bool;

    /// Renders desktop-level chrome (CDE Front Panel + Clock vs. FvwmPager).
    fn render_chrome(
        &self,
        engine: &VulkanEngine,
        cmd_buf: vk::CommandBuffer,
        screen_w: f32,
        screen_h: f32,
        active_workspace: u32,
        has_external_pager: bool,
    );
}
