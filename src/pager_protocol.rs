use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};

pub mod generated {
    use smithay::reexports::wayland_server;

    pub mod __interfaces {
        use smithay::reexports::wayland_server::backend as wayland_backend;
        wayland_scanner::generate_interfaces!("protocols/fvwm_pager_v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_server_code!("protocols/fvwm_pager_v1.xml");
}

pub use generated::zfvwm_pager_manager_v1::{self, ZfvwmPagerManagerV1};

pub struct PagerProtocolState {
    pub clients: Vec<ZfvwmPagerManagerV1>,
}

pub struct PagerWindowInfo {
    pub id: u32,
    pub workspace: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_focused: bool,
}

pub struct WindowIdentInfo {
    pub id: u32,
    pub title: String,
    pub app_id: String,
    pub workspace: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub flags: u32,
}

impl PagerProtocolState {
    pub fn new(display: &DisplayHandle) -> Self {
        display.create_global::<crate::CdeCompositorState, ZfvwmPagerManagerV1, ()>(1, ());
        Self {
            clients: Vec::new(),
        }
    }

    pub fn notify_workspace_changed(&mut self, workspace: u32) {
        self.clients.retain(|c| c.is_alive());
        for client in &self.clients {
            client.workspace_changed(workspace);
        }
    }

    pub fn sync_windows(&mut self, screen_w: u32, screen_h: u32, windows: &[PagerWindowInfo]) {
        self.clients.retain(|c| c.is_alive());
        if self.clients.is_empty() {
            return;
        }
        for client in &self.clients {
            client.screen_size(screen_w, screen_h);
            for w in windows {
                client.window_update(
                    w.id,
                    w.workspace,
                    w.x,
                    w.y,
                    w.width,
                    w.height,
                    if w.is_focused { 1 } else { 0 },
                );
            }
            client.done();
        }
    }

    pub fn notify_window_removed(&mut self, id: u32) {
        self.clients.retain(|c| c.is_alive());
        for client in &self.clients {
            client.window_remove(id);
            client.done();
        }
    }

    pub fn notify_window_ident(&mut self, info: &WindowIdentInfo) {
        self.clients.retain(|c| c.is_alive());
        for client in &self.clients {
            client.window_ident(
                info.id,
                info.title.clone(),
                info.app_id.clone(),
                info.workspace,
                info.x,
                info.y,
                info.width,
                info.height,
                info.flags,
            );
        }
    }
}

impl GlobalDispatch<ZfvwmPagerManagerV1, ()> for crate::CdeCompositorState {
    fn bind(
        state: &mut Self,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<ZfvwmPagerManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let instance = data_init.init(resource, ());
        instance.workspace_changed(state.active_workspace);
        state.pager_protocol_state.clients.push(instance);
        state.sync_pager_windows();
    }
}

impl Dispatch<ZfvwmPagerManagerV1, ()> for crate::CdeCompositorState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ZfvwmPagerManagerV1,
        request: zfvwm_pager_manager_v1::Request,
        _data: &(),
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zfvwm_pager_manager_v1::Request::SetWorkspace { workspace } => {
                if workspace < 4 && state.active_workspace != workspace {
                    state.active_workspace = workspace;
                    state.pager_protocol_state.notify_workspace_changed(workspace);
                    println!("IPC: External FvwmPager switched workspace to {}", workspace);
                }
            }
            zfvwm_pager_manager_v1::Request::PickWindow => {
                println!("IPC: FvwmIdent requested window pick — click any window!");
                state.current_grab = crate::GrabState::PickingWindow;
            }
            zfvwm_pager_manager_v1::Request::Destroy => {}
        }
    }
}
