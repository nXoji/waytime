use crate::config::Config;
use crate::db::Database;
use crate::watcher::WindowEvent;
use chrono::{Local, NaiveDate, TimeZone};
use std::collections::HashSet;

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
    pub notified_limits: HashSet<String>,
    pub last_alert_day: Option<NaiveDate>,
}

impl Tracker {
    pub fn new(db: Database, config: Config) -> Self {
        Self {
            db,
            config,
            active_session: None,
            is_paused: false,
            notified_limits: HashSet::new(),
            last_alert_day: None,
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
                self.check_limits(now);
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

        self.check_limits(now);
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

        self.check_limits(now);
    }

    pub fn check_limits(&mut self, now_ts: i64) {
        let today = chrono::DateTime::from_timestamp(now_ts, 0)
            .map(|dt| dt.with_timezone(&Local).date_naive())
            .unwrap_or_else(|| Local::now().date_naive());

        if Some(today) != self.last_alert_day {
            self.notified_limits.clear();
            self.last_alert_day = Some(today);
        }

        if self.is_paused {
            return;
        }

        let session = match &self.active_session {
            Some(s) if !s.app_id.trim().is_empty() && !self.config.is_ignored(&s.app_id) => s,
            _ => return,
        };

        let app_id = session.app_id.clone();

        let midnight_ts = today
            .and_hms_opt(0, 0, 0)
            .and_then(|naive_dt| Local.from_local_datetime(&naive_dt).earliest())
            .map(|dt| dt.timestamp())
            .unwrap_or(0);

        let db_duration = self
            .db
            .get_app_duration_range(&app_id, midnight_ts, now_ts)
            .unwrap_or(0);

        let current_interval = if session.db_id.is_some() {
            (now_ts - session.ended_at).max(0)
        } else {
            (now_ts - session.started_at).max(0)
        };

        let app_duration = db_duration + current_interval;

        // 1. App-level limit check
        if let Some(app_limit) = self.config.get_limit_seconds(&app_id)
            && app_duration >= app_limit as i64
            && !self.notified_limits.contains(&app_id)
        {
            self.notified_limits.insert(app_id.clone());
            let target = app_id.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let summary = format!("Time limit reached: {target}");
                    let body =
                        format!("You have reached your daily screen time limit for {target}.");
                    let _ = crate::notify::send_notification(&summary, &body).await;
                });
            }
        }

        // 2. Category-level limit check
        if let Some(cat) = self.config.get_category(&app_id) {
            let cat_name = cat.to_string();
            if let Some(cat_limit) = self.config.get_limit_seconds(&cat_name) {
                let cat_duration = match self.db.get_category_summary_range(
                    midnight_ts,
                    now_ts,
                    &self.config.categories,
                ) {
                    Ok(summaries) => {
                        let db_dur = summaries
                            .iter()
                            .find(|c| {
                                c.category.eq_ignore_ascii_case(&cat_name)
                                    || c.category.to_lowercase() == cat_name.to_lowercase()
                            })
                            .map(|c| c.total_duration_sec)
                            .unwrap_or(0);
                        db_dur + current_interval
                    }
                    Err(_) => app_duration,
                };

                if cat_duration >= cat_limit as i64 && !self.notified_limits.contains(&cat_name) {
                    self.notified_limits.insert(cat_name.clone());
                    let target = cat_name;
                    if let Ok(handle) = tokio::runtime::Handle::try_current() {
                        handle.spawn(async move {
                            let summary = format!("Time limit reached: {target}");
                            let body = format!(
                                "You have reached your daily screen time limit for category {target}."
                            );
                            let _ = crate::notify::send_notification(&summary, &body).await;
                        });
                    }
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

    #[tokio::test]
    async fn test_tracker_limit_tracking_and_notification() {
        let (db, db_path) = setup_test_db();
        let mut config = Config::default();
        config.categories.insert(
            "Development".to_string(),
            vec!["code".to_string(), "nvim".to_string()],
        );
        config.limits.insert("code".to_string(), "100s".to_string());
        config
            .limits
            .insert("Development".to_string(), "250s".to_string());

        let mut tracker = Tracker::new(db, config);
        let now = Local::now().timestamp();

        tracker.handle_window_event(WindowEvent {
            app_id: "code".to_string(),
            title: "main.rs".to_string(),
        });

        // 50s: limit not reached yet (100s)
        tracker.heartbeat(now + 50);
        assert!(!tracker.notified_limits.contains("code"));
        assert!(!tracker.notified_limits.contains("Development"));

        // 100s: code limit reached
        tracker.heartbeat(now + 100);
        assert!(tracker.notified_limits.contains("code"));
        assert!(!tracker.notified_limits.contains("Development"));

        // 250s: Development category limit reached
        tracker.heartbeat(now + 250);
        assert!(tracker.notified_limits.contains("code"));
        assert!(tracker.notified_limits.contains("Development"));

        let _ = fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn test_tracker_notification_deduplication() {
        let (db, db_path) = setup_test_db();
        let mut config = Config::default();
        config.limits.insert("code".to_string(), "100s".to_string());

        let mut tracker = Tracker::new(db, config);
        let now = Local::now().timestamp();

        tracker.handle_window_event(WindowEvent {
            app_id: "code".to_string(),
            title: "main.rs".to_string(),
        });

        // Reach limit
        tracker.heartbeat(now + 120);
        assert_eq!(tracker.notified_limits.len(), 1);
        assert!(tracker.notified_limits.contains("code"));

        // Subsequent heartbeats on the same day must not duplicate
        tracker.heartbeat(now + 150);
        tracker.heartbeat(now + 200);
        assert_eq!(tracker.notified_limits.len(), 1);
        assert!(tracker.notified_limits.contains("code"));

        let _ = fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn test_tracker_day_transition_clears_alerts() {
        let (db, db_path) = setup_test_db();
        let mut config = Config::default();
        config.limits.insert("code".to_string(), "60s".to_string());

        let mut tracker = Tracker::new(db, config);
        let now = Local::now().timestamp();

        tracker.handle_window_event(WindowEvent {
            app_id: "code".to_string(),
            title: "main.rs".to_string(),
        });

        // Trigger limit on Day 1
        tracker.heartbeat(now + 70);
        assert!(tracker.notified_limits.contains("code"));
        let day1 = tracker.last_alert_day;
        assert!(day1.is_some());

        // Close Day 1 session
        tracker.flush();

        // Simulate day transition by advancing timestamp by 24 hours (86400s)
        let next_day_ts = now + 90000;
        tracker.check_limits(next_day_ts);

        // Day transition must clear notified_limits and update last_alert_day
        assert!(tracker.notified_limits.is_empty());
        let day2 = tracker.last_alert_day;
        assert!(day2.is_some());
        assert_ne!(day1, day2);

        let _ = fs::remove_file(db_path);
    }
}
