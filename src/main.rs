mod afk;
mod cli;
mod db;
mod tracker;
mod watcher;

use afk::{AfkEvent, AfkWatcher};
use chrono::Local;
use clap::Parser;
use cli::{Cli, Commands};
use db::Database;
use std::time::Duration;
use tracker::Tracker;
use watcher::WindowWatcher;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();
    let command = args.command.unwrap_or(Commands::Today);

    match command {
        Commands::Today => {
            let db = Database::open()?;
            cli::run_today_report(&db)?;
        }
        Commands::Daemon => {
            let db = Database::open()?;
            let mut tracker = Tracker::new(db);
            let mut watcher = WindowWatcher::new().await?;
            let mut afk_watcher = AfkWatcher::new().await?;

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
        }
    }

    Ok(())
}
