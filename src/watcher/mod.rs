pub mod kwin;
pub mod wayland;

pub use kwin::KwinWatcher;
pub use wayland::WaylandWatcher;

use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowEvent {
    pub app_id: String,
    pub title: String,
}

pub trait WindowWatcher: Send + Sync {
    fn start(
        self: Box<Self>,
        event_tx: UnboundedSender<WindowEvent>,
        stop_rx: oneshot::Receiver<()>,
    ) -> tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopEnvironment {
    Kde,
    Other(String),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatcherBackendKind {
    Kwin,
    Other(String),
}

pub fn detect_desktop_environment() -> DesktopEnvironment {
    let xdg = std::env::var("XDG_CURRENT_DESKTOP").ok();
    let session = std::env::var("DESKTOP_SESSION").ok();
    detect_desktop_environment_from_env(xdg.as_deref(), session.as_deref())
}

pub fn detect_desktop_environment_from_env(
    xdg_current_desktop: Option<&str>,
    desktop_session: Option<&str>,
) -> DesktopEnvironment {
    let raw = xdg_current_desktop
        .filter(|s| !s.trim().is_empty())
        .or_else(|| desktop_session.filter(|s| !s.trim().is_empty()));

    match raw {
        Some(de) => {
            let lower = de.to_ascii_lowercase();
            if lower.contains("kde") || lower.contains("kwin") || lower.contains("plasma") {
                DesktopEnvironment::Kde
            } else {
                DesktopEnvironment::Other(de.to_string())
            }
        }
        None => DesktopEnvironment::Unknown,
    }
}

pub fn detect_backend_from_env(
    xdg_current_desktop: Option<&str>,
    desktop_session: Option<&str>,
) -> WatcherBackendKind {
    match detect_desktop_environment_from_env(xdg_current_desktop, desktop_session) {
        DesktopEnvironment::Kde => WatcherBackendKind::Kwin,
        DesktopEnvironment::Other(de) => WatcherBackendKind::Other(de),
        DesktopEnvironment::Unknown => WatcherBackendKind::Other("unknown".to_string()),
    }
}

pub fn create_watcher() -> Result<Box<dyn WindowWatcher>, Box<dyn std::error::Error + Send + Sync>>
{
    match detect_desktop_environment() {
        DesktopEnvironment::Kde => create_watcher_with_env(Some("kde"), None),
        DesktopEnvironment::Other(name) => create_watcher_with_env(Some(&name), None),
        DesktopEnvironment::Unknown => create_watcher_with_env(None, None),
    }
}

pub fn create_watcher_with_env(
    xdg_current_desktop: Option<&str>,
    desktop_session: Option<&str>,
) -> Result<Box<dyn WindowWatcher>, Box<dyn std::error::Error + Send + Sync>> {
    match detect_backend_from_env(xdg_current_desktop, desktop_session) {
        WatcherBackendKind::Kwin => Ok(Box::new(KwinWatcher::new())),
        WatcherBackendKind::Other(_) => Ok(Box::new(WaylandWatcher::new())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn test_window_event_creation_and_equality() {
        let event1 = WindowEvent {
            app_id: "org.mozilla.firefox".to_string(),
            title: "Mozilla Firefox".to_string(),
        };
        let event2 = WindowEvent {
            app_id: "org.mozilla.firefox".to_string(),
            title: "Mozilla Firefox".to_string(),
        };
        let event3 = WindowEvent {
            app_id: "org.kde.konsole".to_string(),
            title: "Konsole".to_string(),
        };

        assert_eq!(event1.app_id, "org.mozilla.firefox");
        assert_eq!(event1.title, "Mozilla Firefox");
        assert_eq!(event1, event2);
        assert_ne!(event1, event3);
        assert_eq!(event1.clone(), event1);
    }

    struct MockWatcher {
        events_to_emit: Vec<WindowEvent>,
    }

    impl WindowWatcher for MockWatcher {
        fn start(
            self: Box<Self>,
            event_tx: UnboundedSender<WindowEvent>,
            mut stop_rx: oneshot::Receiver<()>,
        ) -> tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>> {
            tokio::spawn(async move {
                for event in self.events_to_emit {
                    let _ = event_tx.send(event);
                }
                tokio::select! {
                    _ = event_tx.closed() => {}
                    _ = &mut stop_rx => {}
                }
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn test_trait_object_compatibility() {
        let mock = MockWatcher {
            events_to_emit: vec![
                WindowEvent {
                    app_id: "test_app".to_string(),
                    title: "Test Title".to_string(),
                },
                WindowEvent {
                    app_id: "test_app_2".to_string(),
                    title: "Second Title".to_string(),
                },
            ],
        };

        let watcher: Box<dyn WindowWatcher> = Box::new(mock);
        let (tx, mut rx) = unbounded_channel();
        let (stop_tx, stop_rx) = oneshot::channel();

        let handle = watcher.start(tx, stop_rx);

        let event1 = rx.recv().await.expect("Expected first WindowEvent");
        assert_eq!(event1.app_id, "test_app");
        assert_eq!(event1.title, "Test Title");

        let event2 = rx.recv().await.expect("Expected second WindowEvent");
        assert_eq!(event2.app_id, "test_app_2");
        assert_eq!(event2.title, "Second Title");

        let _ = stop_tx.send(());
        let result = handle.await.expect("JoinHandle panicked");
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_watcher_shutdown_and_cleanup() {
        let mock = MockWatcher {
            events_to_emit: vec![],
        };
        let watcher: Box<dyn WindowWatcher> = Box::new(mock);
        let (tx, _rx) = unbounded_channel();
        let (stop_tx, stop_rx) = oneshot::channel();

        let handle = watcher.start(tx, stop_rx);
        let _ = stop_tx.send(());

        let result = handle.await.expect("JoinHandle panicked");
        assert!(result.is_ok());
    }

    #[test]
    fn test_kwin_watcher_reexport() {
        let watcher: Box<dyn WindowWatcher> = Box::new(KwinWatcher::new());
        drop(watcher);
    }

    #[test]
    fn test_wayland_watcher_reexport() {
        let watcher: Box<dyn WindowWatcher> = Box::new(WaylandWatcher::new());
        drop(watcher);
    }

    #[test]
    fn test_detect_backend_from_env_kde() {
        assert_eq!(
            detect_backend_from_env(Some("KDE"), None),
            WatcherBackendKind::Kwin
        );
        assert_eq!(
            detect_backend_from_env(Some("kde"), None),
            WatcherBackendKind::Kwin
        );
        assert_eq!(
            detect_backend_from_env(Some("plasma"), None),
            WatcherBackendKind::Kwin
        );
        assert_eq!(
            detect_backend_from_env(Some("KWin"), None),
            WatcherBackendKind::Kwin
        );
        assert_eq!(
            detect_backend_from_env(Some("KDE:Plasma"), None),
            WatcherBackendKind::Kwin
        );
    }

    #[test]
    fn test_detect_backend_from_env_non_kde() {
        assert_eq!(
            detect_backend_from_env(Some("Hyprland"), None),
            WatcherBackendKind::Other("Hyprland".to_string())
        );
        assert_eq!(
            detect_backend_from_env(Some("sway"), None),
            WatcherBackendKind::Other("sway".to_string())
        );
        assert_eq!(
            detect_backend_from_env(Some("cosmic"), None),
            WatcherBackendKind::Other("cosmic".to_string())
        );
        assert_eq!(
            detect_backend_from_env(Some("river"), None),
            WatcherBackendKind::Other("river".to_string())
        );
        assert_eq!(
            detect_backend_from_env(Some("GNOME"), None),
            WatcherBackendKind::Other("GNOME".to_string())
        );
    }

    #[test]
    fn test_detect_backend_from_env_fallback() {
        assert_eq!(
            detect_backend_from_env(None, Some("plasma")),
            WatcherBackendKind::Kwin
        );
        assert_eq!(
            detect_backend_from_env(None, Some("sway")),
            WatcherBackendKind::Other("sway".to_string())
        );
        assert_eq!(
            detect_backend_from_env(Some(""), Some("plasma")),
            WatcherBackendKind::Kwin
        );
        assert_eq!(
            detect_backend_from_env(Some("   "), Some("KDE")),
            WatcherBackendKind::Kwin
        );
        assert_eq!(
            detect_backend_from_env(Some(""), Some("Hyprland")),
            WatcherBackendKind::Other("Hyprland".to_string())
        );
        assert_eq!(
            detect_backend_from_env(None, None),
            WatcherBackendKind::Other("unknown".to_string())
        );
        assert_eq!(
            detect_backend_from_env(Some(""), Some("")),
            WatcherBackendKind::Other("unknown".to_string())
        );
    }

    #[test]
    fn test_detect_desktop_environment() {
        let _ = detect_desktop_environment();
    }

    #[test]
    fn test_create_watcher_error_handling() {
        let ok_sway = create_watcher_with_env(Some("sway"), None);
        assert!(ok_sway.is_ok());

        let ok_hyprland = create_watcher_with_env(Some("Hyprland"), None);
        assert!(ok_hyprland.is_ok());

        let ok_unknown = create_watcher_with_env(None, None);
        assert!(ok_unknown.is_ok());

        let ok_kwin = create_watcher_with_env(Some("KDE"), None);
        assert!(ok_kwin.is_ok());
    }

    #[test]
    fn test_desktop_environment_equality_and_debugging() {
        let kde = DesktopEnvironment::Kde;
        let other = DesktopEnvironment::Other("sway".to_string());
        let unknown = DesktopEnvironment::Unknown;

        assert_eq!(kde, DesktopEnvironment::Kde);
        assert_eq!(other, DesktopEnvironment::Other("sway".to_string()));
        assert_eq!(unknown, DesktopEnvironment::Unknown);
        assert_ne!(kde, other);
        assert_ne!(kde, unknown);
        assert_ne!(other, unknown);
        assert_ne!(other, DesktopEnvironment::Other("hyprland".to_string()));

        assert_eq!(kde.clone(), kde);
        assert_eq!(other.clone(), other);
        assert_eq!(unknown.clone(), unknown);

        assert_eq!(format!("{kde:?}"), "Kde");
        assert!(format!("{other:?}").contains("sway"));
        assert_eq!(format!("{unknown:?}"), "Unknown");

        assert_eq!(
            detect_desktop_environment_from_env(Some("   \t  \n  "), None),
            DesktopEnvironment::Unknown
        );
        assert_eq!(
            detect_desktop_environment_from_env(Some("   "), Some("   ")),
            DesktopEnvironment::Unknown
        );
        assert_eq!(
            detect_desktop_environment_from_env(Some("!@#$%^&*()"), None),
            DesktopEnvironment::Other("!@#$%^&*()".to_string())
        );
        assert_eq!(
            detect_desktop_environment_from_env(Some("  \t kDe \n "), None),
            DesktopEnvironment::Kde
        );
    }

    #[test]
    fn test_watcher_backend_kind_debug_and_matching() {
        let kwin = WatcherBackendKind::Kwin;
        let other = WatcherBackendKind::Other("cosmic".to_string());

        assert_eq!(kwin, WatcherBackendKind::Kwin);
        assert_eq!(other, WatcherBackendKind::Other("cosmic".to_string()));
        assert_ne!(kwin, other);
        assert_eq!(kwin.clone(), kwin);
        assert_eq!(other.clone(), other);

        assert_eq!(format!("{kwin:?}"), "Kwin");
        assert!(format!("{other:?}").contains("cosmic"));

        let is_kwin = match kwin {
            WatcherBackendKind::Kwin => true,
            WatcherBackendKind::Other(_) => false,
        };
        assert!(is_kwin);

        let other_name = match other {
            WatcherBackendKind::Kwin => None,
            WatcherBackendKind::Other(name) => Some(name),
        };
        assert_eq!(other_name.as_deref(), Some("cosmic"));
    }
}
