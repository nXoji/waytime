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
    active_session: Option<ActiveSession>,
    is_paused: bool,
}

impl Tracker {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            active_session: None,
            is_paused: false,
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

        if let Some(session) = &mut self.active_session {
            if session.app_id == event.app_id && session.title == event.title {
                session.ended_at = now;
                return;
            }
        }

        if let Some(mut prev) = self.active_session.take() {
            prev.ended_at = now;
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
        if let Some(mut session) = self.active_session.take() {
            let now = Local::now().timestamp();
            if now > session.ended_at {
                session.ended_at = now;
            }
            let duration = session.ended_at - session.started_at;
            if duration >= 1 {
                if let Some(id) = session.db_id {
                    let _ = self.db.update_interval_end(id, session.ended_at);
                } else {
                    let _ = self.db.insert_interval(
                        &session.app_id,
                        &session.title,
                        session.started_at,
                        session.ended_at,
                    );
                }
            }
        }
    }
}
