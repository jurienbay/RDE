// SPDX-License-Identifier: BSD-3-Clause
// Copyright (C) <2026>, <Edward O'Callaghan>, All rights reserved.

// Classic CDE Visual Style Sizing Metrics
pub const CDE_BORDER_WIDTH: f32 = 6.0;   // Chiseled side and bottom frame borders
pub const CDE_TITLEBAR_HEIGHT: f32 = 22.0; // Padded top titlebar arena

/// For Close Button
pub const CDE_BUTTON_SIZE: f32 = 16.0;  // Square button dimension
pub const CDE_BUTTON_OFFSET: f32 = 6.0;  // Pixels from the right border edge

pub const CDE_MIN_BUTTON_OFFSET: f32 = 26.0; // Pushes the minimize button to the left of the close button
pub const CDE_ICON_WIDTH: f32 = 64.0;         // Size of minimized window box on the desktop background
pub const CDE_ICON_HEIGHT: f32 = 64.0;

/// Authentic CDE Front Panel Dock Layout Metrics Constants
pub const CDE_PANEL_HEIGHT: f32 = 64.0;
pub const CDE_PANEL_WIDTH: f32 = 600.0;

/// Authentic CDE Lock Dialog Box Geometry Layout Metrics
pub const CDE_LOCK_WIDTH: f32 = 340.0;
pub const CDE_LOCK_HEIGHT: f32 = 180.0;
pub const CDE_RECESSED_ENTRY_HEIGHT: f32 = 24.0;

/// Traditional CDE step metrics baseline shifts window diagonal bounds by 24px increments
pub const CDE_CASCADE_OFFSET_STEP: f32 = 24.0;
pub const BASELINE_ORIGIN_X: f32 = 30.0;
pub const BASELINE_ORIGIN_Y: f32 = 40.0;

/// CDE Window Menu Row Items ordered sequentially
pub const CDE_MENU_ITEM_RESTORE: u32  = 0;
pub const CDE_MENU_ITEM_MINIMIZE: u32 = 1;
pub const CDE_MENU_ITEM_LOWER: u32    = 2;
pub const CDE_MENU_ITEM_CLOSE: u32    = 3;
pub const CDE_MENU_ROW_HEIGHT: f32    = 20.0; // Padded text lane line metric

// CDE Workspace Root Menu Row Items ordered sequentially
pub const CDE_ROOT_MENU_ITEM_TERMINAL: u32 = 0;
pub const CDE_ROOT_MENU_ITEM_FILEMGR: u32  = 1;
pub const CDE_ROOT_MENU_ITEM_LOCK: u32     = 2;
pub const CDE_ROOT_MENU_ITEM_CANCEL: u32   = 3;
