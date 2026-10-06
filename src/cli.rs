use chrono::{Local, NaiveTime};
use clap::{Parser, Subcommand};
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, CellAlignment, Table};

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

    /// Show detailed window title breakdown
    #[arg(short = 'd', long, global = true)]
    pub details: bool,

    /// Custom path to configuration file
    #[arg(short = 'c', long, global = true, value_name = "PATH")]
    pub config: Option<std::path::PathBuf>,

    /// Print resolved configuration file path and exit
    #[arg(long, global = true)]
    pub config_path: bool,

    /// Output report in JSON format
    #[arg(long, global = true)]
    pub json: bool,
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
    format!("{hours}h {minutes:02}m {secs:02}s")
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

pub fn run_report(
    db: &Database,
    period: ReportPeriod,
    details: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (start_ts, end_ts, title_label) = get_range_for_period(period);

    println!("Report: {title_label}");

    if details {
        let summaries = db.get_detailed_summary_range(start_ts, end_ts)?;
        if summaries.is_empty() {
            println!("No activity recorded for {}.", title_label.to_lowercase());
            return Ok(());
        }

        let total_sec: i64 = summaries.iter().map(|s| s.total_duration_sec).sum();

        let mut table = Table::new();
        table.load_style(UTF8_FULL);
        table.set_header(vec![
            Cell::new("Application / Window Title"),
            Cell::new("Time Spent").set_alignment(CellAlignment::Right),
            Cell::new("Share (%)").set_alignment(CellAlignment::Right),
        ]);
        if let Some(col) = table.column_mut(1) {
            col.set_cell_alignment(CellAlignment::Right);
        }
        if let Some(col) = table.column_mut(2) {
            col.set_cell_alignment(CellAlignment::Right);
        }

        for app in &summaries {
            let app_share = if total_sec > 0 {
                (app.total_duration_sec as f64 / total_sec as f64) * 100.0
            } else {
                0.0
            };

            table.add_row(vec![
                Cell::new(&app.app_id),
                Cell::new(format_duration(app.total_duration_sec))
                    .set_alignment(CellAlignment::Right),
                Cell::new(format!("{app_share:.1}%")).set_alignment(CellAlignment::Right),
            ]);

            let valid_titles: Vec<_> = app.titles.iter().filter(|t| t.duration_sec >= 2).collect();
            for (i, t) in valid_titles.iter().enumerate() {
                let clean_title = if t.title.trim().is_empty() {
                    "[Untitled]".to_string()
                } else if t.title.chars().count() > 60 {
                    let truncated: String = t.title.chars().take(57).collect();
                    format!("{truncated}...")
                } else {
                    t.title.clone()
                };

                let prefix = if i == valid_titles.len() - 1 {
                    "└── "
                } else {
                    "├── "
                };
                let title_display = format!("{prefix}{clean_title}");

                let title_share = if app.total_duration_sec > 0 {
                    (t.duration_sec as f64 / app.total_duration_sec as f64) * 100.0
                } else {
                    0.0
                };

                table.add_row(vec![
                    Cell::new(title_display),
                    Cell::new(format_duration(t.duration_sec)).set_alignment(CellAlignment::Right),
                    Cell::new(format!("({title_share:.1}% of app)"))
                        .set_alignment(CellAlignment::Right),
                ]);
            }
        }

        println!("{table}");
        println!("Total Screen Time: {}", format_duration(total_sec));
    } else {
        let summaries = db.get_summary_range(start_ts, end_ts)?;
        if summaries.is_empty() {
            println!("No activity recorded for {}.", title_label.to_lowercase());
            return Ok(());
        }

        let total_sec: i64 = summaries.iter().map(|s| s.duration_sec).sum();

        let mut table = Table::new();
        table.load_style(UTF8_FULL);
        table.set_header(vec![
            Cell::new("Application"),
            Cell::new("Time Spent").set_alignment(CellAlignment::Right),
            Cell::new("Share (%)").set_alignment(CellAlignment::Right),
        ]);
        if let Some(col) = table.column_mut(1) {
            col.set_cell_alignment(CellAlignment::Right);
        }
        if let Some(col) = table.column_mut(2) {
            col.set_cell_alignment(CellAlignment::Right);
        }

        for app in &summaries {
            let share = if total_sec > 0 {
                (app.duration_sec as f64 / total_sec as f64) * 100.0
            } else {
                0.0
            };

            table.add_row(vec![
                Cell::new(&app.app_id),
                Cell::new(format_duration(app.duration_sec)).set_alignment(CellAlignment::Right),
                Cell::new(format!("{share:.1}%")).set_alignment(CellAlignment::Right),
            ]);
        }

        println!("{table}");
        println!("Total Screen Time: {}", format_duration(total_sec));
    }

    Ok(())
}

#[allow(dead_code)]
pub fn run_today_report(db: &Database) -> Result<(), Box<dyn std::error::Error>> {
    run_report(db, ReportPeriod::Today, false)
}
