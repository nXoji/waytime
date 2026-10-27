use std::collections::HashMap;
use std::os::fd::AsFd;
use std::time::Duration;
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use tokio::time::sleep;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::{
    self, ExtForeignToplevelHandleV1,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_list_v1::{
    self, ExtForeignToplevelListV1,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::{
    self, State as ZwlrState, ZwlrForeignToplevelHandleV1,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::{
    self, ZwlrForeignToplevelManagerV1,
};

use crate::watcher::{WindowEvent, WindowWatcher};

#[derive(Default)]
struct ToplevelWindowInfo {
    app_id: String,
    title: String,
    pending_app_id: Option<String>,
    pending_title: Option<String>,
    pending_active: Option<bool>,
    is_active: bool,
}

#[allow(dead_code)]
enum ActiveProtocol {
    None,
    Ext(ExtForeignToplevelListV1),
    Zwlr(ZwlrForeignToplevelManagerV1),
}

struct WaylandState {
    event_tx: UnboundedSender<WindowEvent>,
    last_active: Option<(String, String)>,
    ext_global: Option<(u32, u32)>,
    zwlr_global: Option<(u32, u32)>,
    active_protocol: ActiveProtocol,
    ext_windows: HashMap<ExtForeignToplevelHandleV1, ToplevelWindowInfo>,
    zwlr_windows: HashMap<ZwlrForeignToplevelHandleV1, ToplevelWindowInfo>,
}

impl WaylandState {
    fn maybe_emit(&mut self, app_id: String, title: String) {
        if app_id.is_empty() && title.is_empty() {
            return;
        }
        let current = (app_id.clone(), title.clone());
        if self.last_active.as_ref() != Some(&current) {
            self.last_active = Some(current);
            let _ = self.event_tx.send(WindowEvent { app_id, title });
        }
    }
}

impl Dispatch<WlRegistry, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _proxy: &WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == ExtForeignToplevelListV1::interface().name {
                state.ext_global = Some((name, version));
            } else if interface == ZwlrForeignToplevelManagerV1::interface().name {
                state.zwlr_global = Some((name, version));
            }
        }
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _proxy: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } => {
                state
                    .ext_windows
                    .insert(toplevel, ToplevelWindowInfo::default());
            }
            ext_foreign_toplevel_list_v1::Event::Finished => {}
            _ => {}
        }
    }
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        proxy: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(info) = state.ext_windows.get_mut(proxy) {
                    info.pending_app_id = Some(app_id);
                }
            }
            ext_foreign_toplevel_handle_v1::Event::Title { title } => {
                if let Some(info) = state.ext_windows.get_mut(proxy) {
                    info.pending_title = Some(title);
                }
            }
            ext_foreign_toplevel_handle_v1::Event::Done => {
                if let Some(info) = state.ext_windows.get_mut(proxy) {
                    if let Some(app_id) = info.pending_app_id.take() {
                        info.app_id = app_id;
                    }
                    if let Some(title) = info.pending_title.take() {
                        info.title = title;
                    }
                    let app_id = info.app_id.clone();
                    let title = info.title.clone();
                    state.maybe_emit(app_id, title);
                }
            }
            ext_foreign_toplevel_handle_v1::Event::Closed => {
                state.ext_windows.remove(proxy);
                proxy.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                state
                    .zwlr_windows
                    .insert(toplevel, ToplevelWindowInfo::default());
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => {}
            _ => {}
        }
    }
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        proxy: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(info) = state.zwlr_windows.get_mut(proxy) {
                    info.pending_app_id = Some(app_id);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                if let Some(info) = state.zwlr_windows.get_mut(proxy) {
                    info.pending_title = Some(title);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::State { state: raw_state } => {
                let is_active = raw_state
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|chunk| u32::from_ne_bytes(*chunk) == ZwlrState::Activated as u32);
                if let Some(info) = state.zwlr_windows.get_mut(proxy) {
                    info.pending_active = Some(is_active);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                if let Some(info) = state.zwlr_windows.get_mut(proxy) {
                    if let Some(app_id) = info.pending_app_id.take() {
                        info.app_id = app_id;
                    }
                    if let Some(title) = info.pending_title.take() {
                        info.title = title;
                    }
                    if let Some(active) = info.pending_active.take() {
                        info.is_active = active;
                    }
                    if info.is_active {
                        let app_id = info.app_id.clone();
                        let title = info.title.clone();
                        state.maybe_emit(app_id, title);
                    }
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                state.zwlr_windows.remove(proxy);
                proxy.destroy();
            }
            _ => {}
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WaylandWatcher;

impl WaylandWatcher {
    pub fn new() -> Self {
        Self
    }
}

enum SessionResult {
    Stopped,
    Disconnected,
}

async fn run_session(
    event_tx: &UnboundedSender<WindowEvent>,
    stop_rx: &mut oneshot::Receiver<()>,
) -> SessionResult {
    if event_tx.is_closed() {
        return SessionResult::Stopped;
    }

    tokio::select! {
        biased;
        _ = &mut *stop_rx => return SessionResult::Stopped,
        _ = event_tx.closed() => return SessionResult::Stopped,
        else => {}
    }

    let conn = match Connection::connect_to_env() {
        Ok(c) => c,
        Err(_) => return SessionResult::Disconnected,
    };
    let display = conn.display();
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();

    let mut state = WaylandState {
        event_tx: event_tx.clone(),
        last_active: None,
        ext_global: None,
        zwlr_global: None,
        active_protocol: ActiveProtocol::None,
        ext_windows: HashMap::new(),
        zwlr_windows: HashMap::new(),
    };

    let registry = display.get_registry(&qh, ());
    if event_queue.roundtrip(&mut state).is_err() {
        return SessionResult::Disconnected;
    }

    if let Some((name, version)) = state.ext_global {
        let proxy = registry.bind::<ExtForeignToplevelListV1, _, _>(name, version.min(1), &qh, ());
        state.active_protocol = ActiveProtocol::Ext(proxy);
    } else if let Some((name, version)) = state.zwlr_global {
        let proxy =
            registry.bind::<ZwlrForeignToplevelManagerV1, _, _>(name, version.min(3), &qh, ());
        state.active_protocol = ActiveProtocol::Zwlr(proxy);
    } else {
        return SessionResult::Disconnected;
    }

    if event_queue.roundtrip(&mut state).is_err() {
        return SessionResult::Disconnected;
    }

    let owned_fd = match conn.as_fd().try_clone_to_owned() {
        Ok(fd) => fd,
        Err(_) => return SessionResult::Disconnected,
    };
    let async_fd = match unsafe { AsyncFd::register(owned_fd) } {
        Ok(fd) => fd,
        Err(_) => return SessionResult::Disconnected,
    };

    loop {
        if event_tx.is_closed() {
            return SessionResult::Stopped;
        }

        if conn.flush().is_err() {
            return SessionResult::Disconnected;
        }
        if event_queue.dispatch_pending(&mut state).is_err() {
            return SessionResult::Disconnected;
        }

        let read_guard = match event_queue.prepare_read() {
            Some(guard) => guard,
            None => {
                if event_queue.dispatch_pending(&mut state).is_err() {
                    return SessionResult::Disconnected;
                }
                continue;
            }
        };

        tokio::select! {
            _ = &mut *stop_rx => {
                drop(read_guard);
                return SessionResult::Stopped;
            }
            _ = event_tx.closed() => {
                drop(read_guard);
                return SessionResult::Stopped;
            }
            res = async_fd.readable() => {
                let mut guard = match res {
                    Ok(g) => g,
                    Err(_) => return SessionResult::Disconnected,
                };
                guard.clear_ready();
                if let Err(e) = read_guard.read() {
                    if matches!(e, wayland_client::backend::WaylandError::Io(ref io_err) if io_err.kind() == std::io::ErrorKind::WouldBlock) {
                        continue;
                    }
                    return SessionResult::Disconnected;
                }
                if event_queue.dispatch_pending(&mut state).is_err() {
                    return SessionResult::Disconnected;
                }
            }
        }
    }
}

impl WindowWatcher for WaylandWatcher {
    fn start(
        self: Box<Self>,
        event_tx: UnboundedSender<WindowEvent>,
        mut stop_rx: oneshot::Receiver<()>,
    ) -> tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>> {
        tokio::spawn(async move {
            loop {
                if event_tx.is_closed() {
                    break;
                }

                match run_session(&event_tx, &mut stop_rx).await {
                    SessionResult::Stopped => break,
                    SessionResult::Disconnected => {
                        eprintln!(
                            "[waytime] Wayland connection lost or failed, attempting reconnect in 2s..."
                        );
                        tokio::select! {
                            _ = &mut stop_rx => break,
                            _ = event_tx.closed() => break,
                            _ = sleep(Duration::from_secs(2)) => {}
                        }
                    }
                }
            }

            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;
    use tokio::sync::oneshot;

    #[test]
    fn test_wayland_watcher_creation() {
        let watcher = WaylandWatcher::new();
        let _ = watcher;
        let watcher_default: WaylandWatcher = Default::default();
        let _ = watcher_default;
    }

    #[tokio::test]
    async fn test_wayland_watcher_start_no_display() {
        let watcher = Box::new(WaylandWatcher::new());
        let (tx, _rx) = unbounded_channel();
        let (stop_tx, stop_rx) = oneshot::channel();
        let handle = watcher.start(tx, stop_rx);
        let _ = stop_tx.send(());
        let res = handle.await.expect("join handle failed");
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_wayland_watcher_stop_on_dropped_sender() {
        let watcher = Box::new(WaylandWatcher::new());
        let (tx, _rx) = unbounded_channel();
        let (stop_tx, stop_rx) = oneshot::channel();
        let handle = watcher.start(tx, stop_rx);
        drop(stop_tx);
        let res = handle.await.expect("join handle failed");
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_wayland_watcher_stop_on_closed_event_tx() {
        let watcher = Box::new(WaylandWatcher::new());
        let (tx, rx) = unbounded_channel();
        let (_stop_tx, stop_rx) = oneshot::channel();
        drop(rx);
        let handle = watcher.start(tx, stop_rx);
        let res = handle.await.expect("join handle failed");
        assert!(res.is_ok());
    }
}
