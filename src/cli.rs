use chrono::{Local, NaiveTime};
use clap::{Parser, Subcommand};
use comfy_table::presets::ASCII_FULL;
use comfy_table::{Cell, Table};

use crate::db::Database;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportPeriod {
    Today,
    Yesterday,
    Week,
    Month,
}

#[derive(Parser, Debug)]
#[command(name = "waytime", about = "Minimal screen time tracker for Wayland")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Show report for yesterday
    #[arg(short = 'y', long, conflicts_with_all = ["week", "month"])]
    pub yesterday: bool,

    /// Show report for the last 7 days
    #[arg(short = 'w', long, conflicts_with_all = ["yesterday", "month"])]
    pub week: bool,

    /// Show report for the last 30 days
    #[arg(short = 'm', long, conflicts_with_all = ["yesterday", "week"])]
    pub month: bool,
}

impl Cli {
    pub fn is_daemon(&self) -> bool {
        matches!(self.command, Some(Commands::Daemon))
    }

    pub fn period(&self) -> ReportPeriod {
        if self.yesterday {
            ReportPeriod::Yesterday
        } else if self.week {
            ReportPeriod::Week
        } else if self.month {
            ReportPeriod::Month
        } else {
            match self.command {
                Some(Commands::Yesterday) => ReportPeriod::Yesterday,
                Some(Commands::Week) => ReportPeriod::Week,
                Some(Commands::Month) => ReportPeriod::Month,
                _ => ReportPeriod::Today,
            }
        }
    }
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum Commands {
    /// Run the window tracking daemon
    Daemon,
    /// Show screen time summary for today (default)
    Today,
    /// Show report for yesterday
    Yesterday,
    /// Show report for the last 7 days
    Week,
    /// Show report for the last 30 days
    Month,
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

pub fn get_range_for_period(period: ReportPeriod) -> (i64, i64, &'static str) {
    let now = Local::now().timestamp();
    let today_midnight = local_midnight_timestamp();

    match period {
        ReportPeriod::Today => (today_midnight, now, "Today"),
        ReportPeriod::Yesterday => (today_midnight - 86400, today_midnight, "Yesterday"),
        ReportPeriod::Week => (today_midnight - (6 * 86400), now, "Week"),
        ReportPeriod::Month => (today_midnight - (29 * 86400), now, "Month"),
    }
}

pub fn run_report(db: &Database, period: ReportPeriod) -> Result<(), Box<dyn std::error::Error>> {
    let (start_ts, end_ts, title_label) = get_range_for_period(period);
    let summaries = db.get_summary_range(start_ts, end_ts)?;

    println!("Report: {title_label}");
    if summaries.is_empty() {
        println!("No activity recorded for {}.", title_label.to_lowercase());
        return Ok(());
    }

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

#[allow(dead_code)]
pub fn run_today_report(db: &Database) -> Result<(), Box<dyn std::error::Error>> {
    run_report(db, ReportPeriod::Today)
}
