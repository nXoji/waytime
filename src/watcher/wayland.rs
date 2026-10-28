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

#[derive(Default, Debug, Clone, PartialEq, Eq)]
struct ToplevelWindowInfo {
    app_id: String,
    title: String,
    pending_app_id: Option<String>,
    pending_title: Option<String>,
    pending_active: Option<bool>,
    is_active: bool,
}

impl ToplevelWindowInfo {
    fn new() -> Self {
        Self::default()
    }

    fn set_app_id(&mut self, app_id: String) {
        self.pending_app_id = Some(app_id);
    }

    fn set_title(&mut self, title: String) {
        self.pending_title = Some(title);
    }

    fn set_active(&mut self, active: bool) {
        self.pending_active = Some(active);
    }

    fn commit(&mut self) -> bool {
        if let Some(app_id) = self.pending_app_id.take() {
            self.app_id = app_id;
        }
        if let Some(title) = self.pending_title.take() {
            self.title = title;
        }
        if let Some(active) = self.pending_active.take() {
            self.is_active = active;
        }
        self.is_active
    }
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
    fn new(event_tx: UnboundedSender<WindowEvent>) -> Self {
        Self {
            event_tx,
            last_active: None,
            ext_global: None,
            zwlr_global: None,
            active_protocol: ActiveProtocol::None,
            ext_windows: HashMap::new(),
            zwlr_windows: HashMap::new(),
        }
    }

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

#[cfg(test)]
impl WaylandState {
    fn last_active(&self) -> Option<&(String, String)> {
        self.last_active.as_ref()
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
                    .insert(toplevel, ToplevelWindowInfo::new());
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
                    info.set_app_id(app_id);
                }
            }
            ext_foreign_toplevel_handle_v1::Event::Title { title } => {
                if let Some(info) = state.ext_windows.get_mut(proxy) {
                    info.set_title(title);
                }
            }
            ext_foreign_toplevel_handle_v1::Event::Done => {
                if let Some(info) = state.ext_windows.get_mut(proxy) {
                    info.commit();
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
                    .insert(toplevel, ToplevelWindowInfo::new());
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
                    info.set_app_id(app_id);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                if let Some(info) = state.zwlr_windows.get_mut(proxy) {
                    info.set_title(title);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::State { state: raw_state } => {
                let is_active = raw_state
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|chunk| u32::from_ne_bytes(*chunk) == ZwlrState::Activated as u32);
                if let Some(info) = state.zwlr_windows.get_mut(proxy) {
                    info.set_active(is_active);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                if let Some(info) = state.zwlr_windows.get_mut(proxy)
                    && info.commit()
                {
                    let app_id = info.app_id.clone();
                    let title = info.title.clone();
                    state.maybe_emit(app_id, title);
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

    let mut state = WaylandState::new(event_tx.clone());

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

    #[tokio::test]
    async fn test_focus_change_and_event_dispatch() {
        let (tx, mut rx) = unbounded_channel();
        let mut state = WaylandState::new(tx);

        state.maybe_emit("org.mozilla.firefox".to_string(), "Firefox".to_string());
        assert_eq!(
            state.last_active(),
            Some(&("org.mozilla.firefox".to_string(), "Firefox".to_string()))
        );

        let event = rx.recv().await.expect("Expected WindowEvent");
        assert_eq!(event.app_id, "org.mozilla.firefox");
        assert_eq!(event.title, "Firefox");

        state.maybe_emit("kitty".to_string(), "Terminal".to_string());
        assert_eq!(
            state.last_active(),
            Some(&("kitty".to_string(), "Terminal".to_string()))
        );

        let event2 = rx.recv().await.expect("Expected second WindowEvent");
        assert_eq!(event2.app_id, "kitty");
        assert_eq!(event2.title, "Terminal");
    }

    #[tokio::test]
    async fn test_window_metadata_edge_cases() {
        let (tx, mut rx) = unbounded_channel();
        let mut state = WaylandState::new(tx);

        // Missing values: both empty
        state.maybe_emit(String::new(), String::new());
        assert!(rx.try_recv().is_err(), "Missing values should not emit");
        assert_eq!(state.last_active(), None);

        // Empty app_id with non-empty title
        state.maybe_emit(String::new(), "Document 1".to_string());
        let event = rx.recv().await.expect("Expected event for empty app_id");
        assert_eq!(event.app_id, "");
        assert_eq!(event.title, "Document 1");
        assert_eq!(
            state.last_active(),
            Some(&(String::new(), "Document 1".to_string()))
        );

        // Empty title with non-empty app_id
        state.maybe_emit("com.example.app".to_string(), String::new());
        let event2 = rx.recv().await.expect("Expected event for empty title");
        assert_eq!(event2.app_id, "com.example.app");
        assert_eq!(event2.title, "");
        assert_eq!(
            state.last_active(),
            Some(&("com.example.app".to_string(), String::new()))
        );

        // Whitespace-only title
        state.maybe_emit("org.alacritty".to_string(), "   \t   ".to_string());
        let event3 = rx
            .recv()
            .await
            .expect("Expected event for whitespace title");
        assert_eq!(event3.app_id, "org.alacritty");
        assert_eq!(event3.title, "   \t   ");
        assert_eq!(
            state.last_active(),
            Some(&("org.alacritty".to_string(), "   \t   ".to_string()))
        );
    }

    #[tokio::test]
    async fn test_event_deduplication() {
        let (tx, mut rx) = unbounded_channel();
        let mut state = WaylandState::new(tx);

        state.maybe_emit("firefox".to_string(), "Rust Documentation".to_string());
        let event = rx.recv().await.expect("Expected initial event");
        assert_eq!(event.app_id, "firefox");
        assert_eq!(event.title, "Rust Documentation");

        // Repeated focus events with identical app_id and title should be ignored
        for _ in 0..5 {
            state.maybe_emit("firefox".to_string(), "Rust Documentation".to_string());
        }
        assert!(
            rx.try_recv().is_err(),
            "Duplicate events should be deduplicated"
        );

        // Focus changes to different app
        state.maybe_emit("code".to_string(), "wayland.rs - waytime".to_string());
        let event2 = rx.recv().await.expect("Expected event on focus change");
        assert_eq!(event2.app_id, "code");

        // Duplicate code event
        state.maybe_emit("code".to_string(), "wayland.rs - waytime".to_string());
        assert!(
            rx.try_recv().is_err(),
            "Duplicate code event should be deduplicated"
        );

        // Switch back to original window
        state.maybe_emit("firefox".to_string(), "Rust Documentation".to_string());
        let event3 = rx
            .recv()
            .await
            .expect("Expected event when returning to window");
        assert_eq!(event3.app_id, "firefox");
    }

    #[tokio::test]
    async fn test_window_lifecycle_handling() {
        let (tx, mut rx) = unbounded_channel();
        let mut state = WaylandState::new(tx);
        let mut windows = HashMap::new();

        // 1. Window creation
        let win_id = 1u32;
        let mut win = ToplevelWindowInfo::new();
        assert_eq!(win.app_id, "");
        assert_eq!(win.title, "");
        assert!(!win.is_active);

        // 2. Property updates
        win.set_app_id("org.gnome.TextEditor".to_string());
        win.set_title("Untitled Document".to_string());
        assert_eq!(win.app_id, "");
        assert_eq!(win.title, "");

        // 3. Commit properties without focus
        let active = win.commit();
        assert!(!active);
        assert_eq!(win.app_id, "org.gnome.TextEditor");
        assert_eq!(win.title, "Untitled Document");
        assert!(!win.is_active);
        assert!(rx.try_recv().is_err());
        windows.insert(win_id, win);

        // 4. Focus gain
        let win = windows.get_mut(&win_id).unwrap();
        win.set_active(true);
        if win.commit() {
            state.maybe_emit(win.app_id.clone(), win.title.clone());
        }
        assert!(win.is_active);
        let event = rx.recv().await.expect("Expected event on focus gain");
        assert_eq!(event.app_id, "org.gnome.TextEditor");
        assert_eq!(event.title, "Untitled Document");

        // 5. Title update while remaining active
        win.set_title("notes.txt - Text Editor".to_string());
        if win.commit() {
            state.maybe_emit(win.app_id.clone(), win.title.clone());
        }
        assert_eq!(win.title, "notes.txt - Text Editor");
        let event2 = rx
            .recv()
            .await
            .expect("Expected event on active title change");
        assert_eq!(event2.title, "notes.txt - Text Editor");

        // 6. Focus loss
        win.set_active(false);
        if win.commit() {
            state.maybe_emit(win.app_id.clone(), win.title.clone());
        }
        assert!(!win.is_active);
        assert!(
            rx.try_recv().is_err(),
            "Focus loss should not emit redundant event"
        );

        // 7. Window destruction (closed)
        let removed = windows.remove(&win_id);
        assert!(removed.is_some());
        assert!(!windows.contains_key(&win_id));
    }

    #[tokio::test]
    async fn test_multi_window_lifecycle_and_switching() {
        let (tx, mut rx) = unbounded_channel();
        let mut state = WaylandState::new(tx);
        let mut windows = HashMap::new();

        let mut win1 = ToplevelWindowInfo::new();
        win1.set_app_id("browser".to_string());
        win1.set_title("Home".to_string());
        win1.set_active(true);
        if win1.commit() {
            state.maybe_emit(win1.app_id.clone(), win1.title.clone());
        }
        windows.insert(1u32, win1);

        let event1 = rx.recv().await.expect("Expected event for win1");
        assert_eq!(event1.app_id, "browser");

        let mut win2 = ToplevelWindowInfo::new();
        win2.set_app_id("editor".to_string());
        win2.set_title("main.rs".to_string());
        windows.insert(2u32, win2);

        // Switch focus from win1 to win2
        windows.get_mut(&1u32).unwrap().set_active(false);
        windows.get_mut(&1u32).unwrap().commit();

        let w2 = windows.get_mut(&2u32).unwrap();
        w2.set_active(true);
        if w2.commit() {
            state.maybe_emit(w2.app_id.clone(), w2.title.clone());
        }

        let event2 = rx.recv().await.expect("Expected event for win2");
        assert_eq!(event2.app_id, "editor");

        // Destroy win2
        windows.remove(&2u32);

        // win1 regains focus
        let w1 = windows.get_mut(&1u32).unwrap();
        w1.set_active(true);
        if w1.commit() {
            state.maybe_emit(w1.app_id.clone(), w1.title.clone());
        }

        let event3 = rx
            .recv()
            .await
            .expect("Expected event when win1 regains focus");
        assert_eq!(event3.app_id, "browser");
    }

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
