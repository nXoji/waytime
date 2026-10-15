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
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let watcher = watcher::create_watcher().map_err(|e| {
            eprintln!("[{timestamp}] {e}");
            e as Box<dyn std::error::Error>
        })?;
        let watcher_handle = watcher.start(event_tx, stop_rx);
        let mut afk_watcher = AfkWatcher::new().await?;
        let mut sigterm = signal(SignalKind::terminate())?;

        let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(30));
        heartbeat_interval.tick().await;

        let mut stop_tx = Some(stop_tx);

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    tracker.flush();
                    if let Some(tx) = stop_tx.take() {
                        let _ = tx.send(());
                    }
                    break;
                }
                _ = sigterm.recv() => {
                    tracker.flush();
                    if let Some(tx) = stop_tx.take() {
                        let _ = tx.send(());
                    }
                    break;
                }
                Some(event) = event_rx.recv() => {
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

        if let Some(tx) = stop_tx.take() {
            let _ = tx.send(());
        }
        drop(event_rx);
        let _ = watcher_handle.await;
    } else {
        let (start_ts, end_ts, label) = args.resolve_filter()?;
        let db = Database::open()?;
        let config = Config::load(args.config.as_deref())?;
        if args.json {
            if args.categories {
                let summaries =
                    db.get_category_summary_range(start_ts, end_ts, &config.categories)?;
                println!("{}", serde_json::to_string_pretty(&summaries)?);
            } else if args.details {
                let summaries = db.get_detailed_summary_range(start_ts, end_ts)?;
                println!("{}", serde_json::to_string_pretty(&summaries)?);
            } else {
                let summaries = db.get_summary_range(start_ts, end_ts)?;
                println!("{}", serde_json::to_string_pretty(&summaries)?);
            }
        } else {
            cli::run_report(
                &db,
                &config,
                start_ts,
                end_ts,
                &label,
                args.details,
                args.categories,
            )?;
        }
    }

    Ok(())
}
