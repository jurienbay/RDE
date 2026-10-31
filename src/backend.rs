// SPDX-License-Identifier: BSD-3-Clause
// Copyright (C) <2026>, <Edward O'Callaghan>, All rights reserved.

use gbm::BufferObjectFlags;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBuffer, GbmDevice};
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmDeviceNotifier, DrmEventMetadata};
use smithay::backend::session::Session;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::reexports::calloop::{LoopHandle, RegistrationToken};
use smithay::reexports::drm::control::{Device, Mode, PageFlipFlags, connector, crtc, framebuffer};
use smithay::utils::DeviceFd;

/// Sealed configuration wrapper executing naked hardware platform routines
pub struct HardwareBackendContext {
    pub drm_device: DrmDevice,
    pub drm_notifier: Option<DrmDeviceNotifier>,
    pub gbm_allocator: GbmAllocator<DrmDeviceFd>,
    pub primary_crtc: crtc::Handle,
    pub primary_connector: connector::Handle,
    pub active_mode: Mode,
    pub current_fb: Option<framebuffer::Handle>,
    token: Option<RegistrationToken>,
}

impl HardwareBackendContext {
    /// Discovers active display lines and initializes raw KMS/GBM drivers rootlessly via an open session
    pub fn init(
        session: &mut LibSeatSession,
        drm_node: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Request the unprivileged session manager to open our GPU card device handle safely
        let device_id = session.open(
            std::path::Path::new(drm_node),
            smithay::reexports::rustix::fs::OFlags::RDWR,
        )?;
        let base_device_fd = DeviceFd::from(device_id);
        let drm_device_fd = DrmDeviceFd::new(base_device_fd);

        let (drm_device, drm_notifier) = DrmDevice::new(drm_device_fd, true)?;
        let gbm_device = GbmDevice::new(drm_device.device_fd().clone())?;
        let gbm_allocator = GbmAllocator::new(
            gbm_device,
            BufferObjectFlags::SCANOUT | BufferObjectFlags::RENDERING,
        );

        // Find active monitor lines to pipe graphics downstream
        let resources = drm_device.resource_handles()?;
        let connector = resources
            .connectors()
            .iter()
            .copied()
            .find(|&conn| {
                drm_device
                    .get_connector(conn, true)
                    .map(|info| info.state() == connector::State::Connected)
                    .unwrap_or(false)
            })
            .ok_or(
                "Hardware Backend Error: Failed to locate an active connected physical monitor",
            )?;

        let connector_info = drm_device.get_connector(connector, true)?;
        let active_mode = connector_info.modes().get(0)
            .copied()
            .ok_or("Hardware Backend Error: Target display panel does not advertise valid resolution modes")?;

        // Locate appropriate CRTC pipeline block matching selected connector links
        let crtc = resources.crtcs().iter().copied().next().ok_or(
            "Hardware Backend Error: Failed to allocate a valid scanout CRTC pipeline slice",
        )?;

        println!(
            "Master KMS Console Linked! Native Target Panel Resolution: {}x{}@{}Hz",
            active_mode.size().0,
            active_mode.size().1,
            active_mode.vrefresh()
        );

        Ok(Self {
            drm_device,
            drm_notifier: Some(drm_notifier),
            gbm_allocator,
            primary_crtc: crtc,
            primary_connector: connector,
            active_mode,
            current_fb: None,
            token: None,
        })
    }

    /// Registers internal VBlank / Page-Flip completion streams directly with Calloop loops
    pub fn register_event_source<State: 'static>(
        &mut self,
        loop_handle: &LoopHandle<'static, State>,
        mut handler: impl FnMut(crtc::Handle, &mut State) + 'static,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let notifier = self
            .drm_notifier
            .take()
            .ok_or("Hardware Backend Error: Event source notifier already registered")?;
        let token = loop_handle.insert_source(notifier, move |event, _, state| {
            match event {
                smithay::backend::drm::DrmEvent::VBlank(crtc_handle) => {
                    handler(crtc_handle, state);
                }
                smithay::backend::drm::DrmEvent::Error(err) => {
                    println!("Hardware Backend Error: DRM device event stream threw an unhandled error: {:?}", err);
                }
            }
        })?;

        self.token = Some(token);
        Ok(())
    }

    /// Submits a completed GBM surface directly to physical screen panels via KMS PageFlips
    pub fn scanout_buffer(
        &mut self,
        gbm_buffer: &GbmBuffer,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let fb = self
            .drm_device
            .add_planar_framebuffer(
                gbm_buffer,
                smithay::reexports::drm::control::FbCmd2Flags::empty(),
            )
            .map_err(|e| {
                println!("DRM Framebuffer Error: {:?}", e);
                "Hardware Backend Error: Failed to add framebuffer tracking layout index"
            })?;

        self.drm_device
            .page_flip(self.primary_crtc, fb, PageFlipFlags::EVENT, None)?;

        self.current_fb = Some(fb);
        Ok(())
    }
}
