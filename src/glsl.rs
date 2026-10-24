// SPDX-License-Identifier: BSD-3-Clause
// Copyright (C) <2026>, <Edward O'Callaghan>, All rights reserved.

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct CdePushConstants {
    pub mvp: [f32; 16],         // 4x4 matrix for geometry projection
    pub element_size: [f32; 2], // [width, height] in pixels
    pub is_focused: u32,
    pub is_dragging: u32,       // 0 = Normal, 1 = Window Dragging
    pub is_pressed: u32,        // 0 = Idle, 1 = Close Pressed, 2 = Minimize Pressed, 3 = Menu Pressed
    pub title_chars: [u32; 16], // Stores up to 16 ASCII character tokens for the app name.
}

impl CdePushConstants {
    pub fn new_ui_element(mvp: [f32; 16], size: [f32; 2], state_flag: u32) -> Self {
        let is_focused = state_flag & 0x0F; // TODO make into const and have encoding helper fn
        let is_pressed = state_flag >> 4; // Upper bits to get the password_length.
        Self {
            mvp,
            element_size: size,
            is_focused,
            is_dragging: 0,
            is_pressed,
            title_chars: [0u32; 16],
        }
    }
}
