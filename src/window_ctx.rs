// SPDX-License-Identifier: BSD-3-Clause
// Copyright (C) <2026>, <Edward O'Callaghan>, All rights reserved.

use smithay::wayland::shell::xdg::ToplevelSurface;

// Explicit structural tags matching standard Wayland layout rules
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResizeEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

// Structural context representing a single floating CDE application window
pub struct WindowContext {
    pub id: u32, // Unique window instance tag identifier
    pub toplevel: ToplevelSurface,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub workspace: u32,
    pub is_minimized: bool,
}

impl WindowContext {
    /// Detects if a coordinate hits the chiseled borders and maps the appropriate drag handle edge
    pub fn get_resize_edge(&self, mx: f32, my: f32) -> Option<ResizeEdge> {
        let border = crate::CDE_BORDER_WIDTH;
        let corner = 24.0; // Matches shader corner_size layout metric

        // Explicit checking bounds relative to window coordinates
        let is_left   = mx >= self.x && mx < self.x + border;
        let is_right  = mx > (self.x + self.width - border) && mx <= self.x + self.width;
        let is_top    = my >= self.y && my < self.y + border;
        let is_bottom = my > (self.y + self.height - border) && my <= self.y + self.height;

        // Ensure the click actually hits inside the total bounding perimeter frame
        let inside_x = mx >= self.x && mx <= self.x + self.width;
        let inside_y = my >= self.y && my <= self.y + self.height;
        if !inside_x || !inside_y { return None; }

        // Corner intersections
        if (is_left || mx < self.x + corner) && (is_top || my < self.y + corner) { return Some(ResizeEdge::TopLeft); }
        if (is_right || mx > self.x + self.width - corner) && (is_top || my < self.y + corner) { return Some(ResizeEdge::TopRight); }
        if (is_left || mx < self.x + corner) && (is_bottom || my > self.y + self.height - corner) { return Some(ResizeEdge::BottomLeft); }
        if (is_right || mx > self.x + self.width - corner) && (is_bottom || my > self.y + self.height - corner) { return Some(ResizeEdge::BottomRight); }

        // Edge segments fallback checks
        if is_left   { return Some(ResizeEdge::Left); }
        if is_right  { return Some(ResizeEdge::Right); }
        if is_top    { return Some(ResizeEdge::Top); }
        if is_bottom { return Some(ResizeEdge::Bottom); }

        None
    }
}
