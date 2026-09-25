mod db;
mod tracker;
mod watcher;

use chrono::Local;
use db::Database;
use std::time::Duration;
use tracker::Tracker;
use watcher::WindowWatcher;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::open()?;
    let mut tracker = Tracker::new(db);
    let mut watcher = WindowWatcher::new().await?;

    let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(30));
    heartbeat_interval.tick().await;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracker.flush();
                let _ = watcher.unload().await;
                break;
            }
            Some(event) = watcher.next_event() => {
                let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
                println!("[{timestamp}] App: {} | Title: {}", event.app_id, event.title);
                tracker.handle_window_event(event);
            }
            _ = heartbeat_interval.tick() => {
                tracker.heartbeat(Local::now().timestamp());
            }
        }
    }

    Ok(())
}
