mod afk;
mod cli;
pub mod config;
mod db;
mod notify;
mod tracker;
mod watcher;

use afk::{AfkEvent, AfkWatcher};
use chrono::Local;
use clap::Parser;
use cli::Cli;
use config::Config;
use db::Database;
use std::time::Duration;
use tokio::signal::unix::{SignalKind, signal};
use tracker::Tracker;
use watcher::WindowWatcher;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();

    if args.config_path {
        let config_path = Config::resolve_path(args.config.as_deref())?;
        println!("{}", config_path.display());
        return Ok(());
    }

    if args.is_daemon() {
        let config_path = Config::resolve_path(args.config.as_deref())?;
        let config = Config::load(args.config.as_deref())?;
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        println!("[{timestamp}] Config: {}", config_path.display());

        let db = Database::open()?;
        let mut tracker = Tracker::new(db, config);
        let mut watcher = WindowWatcher::new().await?;
        let mut afk_watcher = AfkWatcher::new().await?;
        let mut sigterm = signal(SignalKind::terminate())?;

        let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(30));
        heartbeat_interval.tick().await;

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    tracker.flush();
                    let _ = watcher.unload().await;
                    break;
                }
                _ = sigterm.recv() => {
                    tracker.flush();
                    let _ = watcher.unload().await;
                    break;
                }
                Some(event) = watcher.next_event() => {
                    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
                    println!("[{timestamp}] App: {} | Title: {}", event.app_id, event.title);
                    tracker.handle_window_event(event);
                }
                Some(afk_event) = afk_watcher.next_event() => {
                    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
                    match afk_event {
                        AfkEvent::Pause => {
                            println!("[{timestamp}] State: Paused (AFK / Screen Locked / Sleep)");
                            tracker.pause();
                        }
                        AfkEvent::Resume => {
                            println!("[{timestamp}] State: Resumed");
                            tracker.resume();
                        }
                    }
                }
                _ = heartbeat_interval.tick() => {
                    tracker.heartbeat(Local::now().timestamp());
                }
            }
        }
    } else {
        let (start_ts, end_ts, label) = args.resolve_filter()?;
        let db = Database::open()?;
        if args.json {
            if args.details {
                let summaries = db.get_detailed_summary_range(start_ts, end_ts)?;
                println!("{}", serde_json::to_string_pretty(&summaries)?);
            } else {
                let summaries = db.get_summary_range(start_ts, end_ts)?;
                println!("{}", serde_json::to_string_pretty(&summaries)?);
            }
        } else {
            cli::run_report(&db, start_ts, end_ts, &label, args.details)?;
        }
    }

    Ok(())
}
