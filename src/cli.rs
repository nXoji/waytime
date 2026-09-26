use chrono::{Local, NaiveTime};
use clap::{Parser, Subcommand};
use comfy_table::presets::ASCII_FULL;
use comfy_table::{Cell, Table};

use crate::db::Database;

#[derive(Parser, Debug)]
#[command(name = "waytime", about = "Minimal screen time tracker for Wayland")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum Commands {
    /// Run the window tracking daemon
    Daemon,
    /// Show screen time summary for today (default)
    Today,
}

pub fn format_duration(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    format!("{hours}h {minutes}m {secs}s")
}

pub fn local_midnight_timestamp() -> i64 {
    let now = Local::now();
    now.date_naive()
        .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
        .and_local_timezone(Local)
        .single()
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| {
            let offset = now.offset().local_minus_utc() as i64;
            let utc_ts = now.timestamp();
            let local_day_start = (utc_ts + offset) - ((utc_ts + offset) % 86400);
            local_day_start - offset
        })
}

pub fn run_today_report(db: &Database) -> Result<(), Box<dyn std::error::Error>> {
    let midnight_ts = local_midnight_timestamp();
    let summaries = db.get_summary_since(midnight_ts)?;

    let total_sec: i64 = summaries.iter().map(|s| s.duration_sec).sum();

    let mut table = Table::new();
    table.load_style(ASCII_FULL);
    table.set_header(vec![
        Cell::new("Application"),
        Cell::new("Time Spent"),
        Cell::new("Share (%)"),
    ]);

    for app in &summaries {
        let share = if total_sec > 0 {
            (app.duration_sec as f64 / total_sec as f64) * 100.0
        } else {
            0.0
        };

        table.add_row(vec![
            Cell::new(&app.app_id),
            Cell::new(format_duration(app.duration_sec)),
            Cell::new(format!("{share:.1}%")),
        ]);
    }

    println!("{table}");
    println!("Total Screen Time: {}", format_duration(total_sec));

    Ok(())
}
