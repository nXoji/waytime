use crate::config::Config;
use crate::db::Database;
use crate::watcher::WindowEvent;
use chrono::Local;

#[derive(Debug)]
struct ActiveSession {
    db_id: Option<i64>,
    app_id: String,
    title: String,
    started_at: i64,
    ended_at: i64,
}

pub struct Tracker {
    db: Database,
    config: Config,
    active_session: Option<ActiveSession>,
    is_paused: bool,
}

impl Tracker {
    pub fn new(db: Database, config: Config) -> Self {
        Self {
            db,
            config,
            active_session: None,
            is_paused: false,
        }
    }

    #[allow(dead_code)]
    pub fn config(&self) -> &Config {
        &self.config
    }

    fn close_active_session(&mut self, now: i64) {
        if let Some(mut prev) = self.active_session.take() {
            if now > prev.ended_at {
                prev.ended_at = now;
            }
            let duration = prev.ended_at - prev.started_at;
            if duration >= 1 {
                if let Some(id) = prev.db_id {
                    let _ = self.db.update_interval_end(id, prev.ended_at);
                } else {
                    let _ = self.db.insert_interval(
                        &prev.app_id,
                        &prev.title,
                        prev.started_at,
                        prev.ended_at,
                    );
                }
            }
        }
    }

    pub fn pause(&mut self) {
        if self.is_paused {
            return;
        }

        if let Some(session) = &mut self.active_session {
            let now = Local::now().timestamp();
            session.ended_at = now;
            let duration = session.ended_at - session.started_at;
            if duration >= 1 {
                if let Some(id) = session.db_id {
                    let _ = self.db.update_interval_end(id, session.ended_at);
                } else if let Ok(id) = self.db.insert_interval(
                    &session.app_id,
                    &session.title,
                    session.started_at,
                    session.ended_at,
                ) {
                    session.db_id = Some(id);
                }
            }
        }

        self.is_paused = true;
    }

    pub fn resume(&mut self) {
        if !self.is_paused {
            return;
        }

        self.is_paused = false;
        let now = Local::now().timestamp();
        if let Some(session) = &mut self.active_session {
            session.started_at = now;
            session.ended_at = now;
            session.db_id = None;
        }
    }

    pub fn handle_window_event(&mut self, event: WindowEvent) {
        if self.is_paused {
            self.is_paused = false;
        }

        let now = Local::now().timestamp();

        if event.app_id.trim().is_empty() {
            self.close_active_session(now);
            return;
        }

        if self.config.is_ignored(&event.app_id) {
            self.close_active_session(now);
            return;
        }

        #[allow(clippy::collapsible_if)]
        if let Some(session) = &mut self.active_session {
            if session.app_id == event.app_id && session.title == event.title {
                session.ended_at = now;
                return;
            }
        }

        self.close_active_session(now);

        self.active_session = Some(ActiveSession {
            db_id: None,
            app_id: event.app_id,
            title: event.title,
            started_at: now,
            ended_at: now,
        });
    }

    pub fn heartbeat(&mut self, now: i64) {
        if self.is_paused {
            return;
        }

        if let Some(session) = &mut self.active_session {
            session.ended_at = now;
            if session.ended_at - session.started_at >= 1 {
                if let Some(id) = session.db_id {
                    let _ = self.db.update_interval_end(id, session.ended_at);
                } else if let Ok(id) = self.db.insert_interval(
                    &session.app_id,
                    &session.title,
                    session.started_at,
                    session.ended_at,
                ) {
                    session.db_id = Some(id);
                }
            }
        }
    }

    pub fn flush(&mut self) {
        let now = Local::now().timestamp();
        self.close_active_session(now);
    }

    #[cfg(test)]
    pub fn is_idle(&self) -> bool {
        self.active_session.is_none()
    }

    #[cfg(test)]
    pub fn active_app_id(&self) -> Option<&str> {
        self.active_session.as_ref().map(|s| s.app_id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn setup_test_db() -> (Database, std::path::PathBuf) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("waytime_test_tracker_{nonce}.db"));
        let db = Database::open_at(&db_path).expect("failed to open test database");
        (db, db_path)
    }

    #[test]
    fn test_tracker_normal_flow() {
        let (db, db_path) = setup_test_db();
        let config = Config::default();
        let mut tracker = Tracker::new(db, config);

        assert!(tracker.is_idle());

        tracker.handle_window_event(WindowEvent {
            app_id: "firefox".to_string(),
            title: "Mozilla Firefox".to_string(),
        });

        assert!(!tracker.is_idle());
        assert_eq!(tracker.active_app_id(), Some("firefox"));

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn test_tracker_ignore_apps_enters_idle() {
        let (db, db_path) = setup_test_db();
        let mut config = Config::default();
        config.ignore_apps.insert("keepassxc".to_string());
        config.ignore_apps.insert("swaylock".to_string());

        let mut tracker = Tracker::new(db, config);

        // Focusing ignored app directly starts in idle state
        tracker.handle_window_event(WindowEvent {
            app_id: "KeePassXC".to_string(),
            title: "Passwords".to_string(),
        });
        assert!(tracker.is_idle());

        // Focusing tracked app enters active tracking
        tracker.handle_window_event(WindowEvent {
            app_id: "kitty".to_string(),
            title: "terminal".to_string(),
        });
        assert!(!tracker.is_idle());
        assert_eq!(tracker.active_app_id(), Some("kitty"));

        // Focusing ignored app closes active session and enters idle state
        tracker.handle_window_event(WindowEvent {
            app_id: "swaylock".to_string(),
            title: "Lockscreen".to_string(),
        });
        assert!(tracker.is_idle());
        assert_eq!(tracker.active_app_id(), None);

        // Heartbeat while on ignored app does not crash or create session
        tracker.heartbeat(Local::now().timestamp() + 30);
        assert!(tracker.is_idle());

        // Focusing another ignored app remains idle
        tracker.handle_window_event(WindowEvent {
            app_id: "keepassxc".to_string(),
            title: "Passwords".to_string(),
        });
        assert!(tracker.is_idle());

        // Switching back to normal app tracks again
        tracker.handle_window_event(WindowEvent {
            app_id: "code".to_string(),
            title: "VS Code".to_string(),
        });
        assert!(!tracker.is_idle());
        assert_eq!(tracker.active_app_id(), Some("code"));

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn test_tracker_commits_session_on_ignored_focus() {
        let (db, db_path) = setup_test_db();
        let mut config = Config::default();
        config.ignore_apps.insert("keepassxc".to_string());

        let mut tracker = Tracker::new(db, config);

        let now = Local::now().timestamp();

        // Start tracking firefox
        tracker.handle_window_event(WindowEvent {
            app_id: "firefox".to_string(),
            title: "Mozilla Firefox".to_string(),
        });

        // Advance session time via heartbeat so duration >= 1
        tracker.heartbeat(now + 10);

        // Switch to ignored app
        tracker.handle_window_event(WindowEvent {
            app_id: "KeePassXC".to_string(),
            title: "Database".to_string(),
        });
        assert!(tracker.is_idle());

        // Check that firefox was recorded in the database
        let summaries = tracker.db.get_summary_range(now - 10, now + 30).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].app_id, "firefox");
        assert!(summaries[0].duration_sec >= 10);

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn test_tracker_empty_app_id_enters_idle() {
        let (db, db_path) = setup_test_db();
        let config = Config::default();
        let mut tracker = Tracker::new(db, config);

        let now = Local::now().timestamp();

        tracker.handle_window_event(WindowEvent {
            app_id: "firefox".to_string(),
            title: "Mozilla Firefox".to_string(),
        });
        tracker.heartbeat(now + 10);
        assert!(!tracker.is_idle());

        // Empty / whitespace app_id closes active session and sets idle
        tracker.handle_window_event(WindowEvent {
            app_id: "   ".to_string(),
            title: "".to_string(),
        });
        assert!(tracker.is_idle());

        let summaries = tracker.db.get_summary_range(now - 10, now + 30).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].app_id, "firefox");
        assert!(summaries[0].duration_sec >= 10);

        let _ = fs::remove_file(db_path);
    }
}
