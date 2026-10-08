use chrono::{Local, NaiveDate, NaiveTime};
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
#[command(
    name = "waytime",
    version,
    about = "Minimal screen time tracker for Wayland"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Show report for yesterday
    #[arg(short = 'y', long, conflicts_with_all = ["week", "month", "date", "from", "to", "since"])]
    pub yesterday: bool,

    /// Show report for the last 7 days
    #[arg(short = 'w', long, conflicts_with_all = ["yesterday", "month", "date", "from", "to", "since"])]
    pub week: bool,

    /// Show report for the last 30 days
    #[arg(short = 'm', long, conflicts_with_all = ["yesterday", "week", "date", "from", "to", "since"])]
    pub month: bool,

    /// Show report for a specific date (YYYY-MM-DD, today, yesterday)
    #[arg(long, value_name = "DATE", conflicts_with_all = ["yesterday", "week", "month", "from", "to", "since"])]
    pub date: Option<String>,

    /// Show report starting from a date (YYYY-MM-DD)
    #[arg(long, value_name = "YYYY-MM-DD", conflicts_with_all = ["yesterday", "week", "month", "date", "since"])]
    pub from: Option<String>,

    /// Show report ending at a date (YYYY-MM-DD)
    #[arg(long, value_name = "YYYY-MM-DD", requires = "from", conflicts_with_all = ["yesterday", "week", "month", "date", "since"])]
    pub to: Option<String>,

    /// Show report since duration ago (e.g. 3d, 12h, 45m)
    #[arg(long, value_name = "DURATION", conflicts_with_all = ["yesterday", "week", "month", "date", "from", "to"])]
    pub since: Option<String>,

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

    pub fn resolve_filter(&self) -> Result<(i64, i64, String), String> {
        resolve_date_filter(self)
    }

    #[allow(dead_code)]
    pub fn resolve_date_filter(&self) -> Result<(i64, i64, String), String> {
        resolve_date_filter(self)
    }

    #[allow(dead_code)]
    pub fn resolve_range(&self) -> Result<(i64, i64, String), String> {
        resolve_date_filter(self)
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

pub fn parse_duration(s: &str) -> Result<i64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("Duration string cannot be empty".to_string());
    }

    let mut total_secs: i64 = 0;
    let mut num_str = String::new();
    let mut matched_any = false;

    for ch in s.chars() {
        if ch.is_ascii_whitespace() {
            continue;
        } else if ch.is_ascii_digit() {
            num_str.push(ch);
        } else {
            let unit = ch.to_ascii_lowercase();
            if num_str.is_empty() {
                return Err(format!(
                    "Invalid duration format '{s}': unit '{ch}' without preceding number"
                ));
            }
            let val: i64 = num_str
                .parse()
                .map_err(|_| format!("Invalid number '{num_str}' in duration"))?;
            num_str.clear();

            let unit_secs = match unit {
                'd' => 86400,
                'h' => 3600,
                'm' => 60,
                's' => 1,
                _ => {
                    return Err(format!(
                        "Unknown duration unit '{ch}' in '{s}'. Supported units: d, h, m (or s)"
                    ));
                }
            };

            total_secs = total_secs
                .checked_add(
                    val.checked_mul(unit_secs)
                        .ok_or_else(|| "Duration value too large".to_string())?,
                )
                .ok_or_else(|| "Duration value too large".to_string())?;
            matched_any = true;
        }
    }

    if !num_str.is_empty() {
        return Err(format!(
            "Invalid duration format '{s}': missing unit for number '{num_str}'"
        ));
    }

    if !matched_any || total_secs <= 0 {
        return Err(format!("Duration must be greater than zero, got '{s}'"));
    }

    Ok(total_secs)
}

pub fn date_to_day_bounds(date: NaiveDate) -> (i64, i64) {
    let start_naive = date.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    let end_naive = date.and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap());

    let start_ts = start_naive
        .and_local_timezone(Local)
        .earliest()
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| {
            let now = Local::now();
            let offset = now.offset().local_minus_utc() as i64;
            start_naive.and_utc().timestamp() - offset
        });

    let end_ts = end_naive
        .and_local_timezone(Local)
        .latest()
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| {
            let now = Local::now();
            let offset = now.offset().local_minus_utc() as i64;
            end_naive.and_utc().timestamp() - offset
        });

    (start_ts, end_ts)
}

fn parse_date_argument(date_str: &str) -> Result<(i64, i64, String), String> {
    let trimmed = date_str.trim();
    if trimmed.eq_ignore_ascii_case("today") {
        let today = Local::now().date_naive();
        let (start, end) = date_to_day_bounds(today);
        Ok((start, end, "Today".to_string()))
    } else if trimmed.eq_ignore_ascii_case("yesterday") {
        let yesterday = Local::now().date_naive() - chrono::Duration::days(1);
        let (start, end) = date_to_day_bounds(yesterday);
        Ok((start, end, "Yesterday".to_string()))
    } else {
        match NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
            Ok(date) => {
                let (start, end) = date_to_day_bounds(date);
                Ok((start, end, trimmed.to_string()))
            }
            Err(_) => Err(format!(
                "Invalid date '{}'. Expected 'YYYY-MM-DD', 'today', or 'yesterday'",
                date_str
            )),
        }
    }
}

fn parse_from_to_arguments(
    from_str: &str,
    to_str: Option<&str>,
) -> Result<(i64, i64, String), String> {
    let from_trimmed = from_str.trim();
    let from_date = NaiveDate::parse_from_str(from_trimmed, "%Y-%m-%d").map_err(|_| {
        format!(
            "Invalid date for --from: '{}'. Expected 'YYYY-MM-DD'",
            from_str
        )
    })?;
    let (start_ts, _) = date_to_day_bounds(from_date);

    match to_str {
        Some(to_val) => {
            let to_trimmed = to_val.trim();
            let to_date = NaiveDate::parse_from_str(to_trimmed, "%Y-%m-%d").map_err(|_| {
                format!("Invalid date for --to: '{}'. Expected 'YYYY-MM-DD'", to_val)
            })?;
            let (_, end_ts) = date_to_day_bounds(to_date);

            if start_ts > end_ts {
                return Err(format!(
                    "--from date ({from_trimmed}) cannot be after --to date ({to_trimmed})"
                ));
            }

            Ok((start_ts, end_ts, format!("{from_trimmed} to {to_trimmed}")))
        }
        None => {
            let end_ts = Local::now().timestamp();
            if start_ts > end_ts {
                return Err(format!(
                    "--from date ({from_trimmed}) cannot be in the future"
                ));
            }
            Ok((start_ts, end_ts, format!("{from_trimmed} to now")))
        }
    }
}

fn parse_since_argument(since_str: &str) -> Result<(i64, i64, String), String> {
    let trimmed = since_str.trim();
    let duration_secs = parse_duration(trimmed)?;
    let now = Local::now().timestamp();
    let start_ts = now
        .checked_sub(duration_secs)
        .ok_or_else(|| "Duration exceeds timestamp range".to_string())?;
    Ok((start_ts, now, trimmed.to_string()))
}

pub fn resolve_date_filter(cli: &Cli) -> Result<(i64, i64, String), String> {
    let mut options_count = 0;
    if cli.date.is_some() {
        options_count += 1;
    }
    if cli.since.is_some() {
        options_count += 1;
    }
    if cli.from.is_some() || cli.to.is_some() {
        options_count += 1;
    }
    if cli.yesterday || matches!(cli.command, Some(Commands::Yesterday)) {
        options_count += 1;
    }
    if cli.week || matches!(cli.command, Some(Commands::Week)) {
        options_count += 1;
    }
    if cli.month || matches!(cli.command, Some(Commands::Month)) {
        options_count += 1;
    }

    if options_count > 1 {
        return Err("Conflicting date filter options provided".to_string());
    }

    if let Some(ref date_str) = cli.date {
        parse_date_argument(date_str)
    } else if let Some(ref since_str) = cli.since {
        parse_since_argument(since_str)
    } else if let Some(ref from_str) = cli.from {
        parse_from_to_arguments(from_str, cli.to.as_deref())
    } else if cli.to.is_some() {
        Err("--to requires --from".to_string())
    } else {
        let (start, end, label) = get_range_for_period(cli.period());
        Ok((start, end, label.to_string()))
    }
}

#[allow(dead_code)]
pub fn resolve_filter(cli: &Cli) -> Result<(i64, i64, String), String> {
    resolve_date_filter(cli)
}

#[allow(dead_code)]
pub fn resolve_range(cli: &Cli) -> Result<(i64, i64, String), String> {
    resolve_date_filter(cli)
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
    start_ts: i64,
    end_ts: i64,
    title_label: &str,
    details: bool,
) -> Result<(), Box<dyn std::error::Error>> {
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
pub fn run_report_for_period(
    db: &Database,
    period: ReportPeriod,
    details: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (start_ts, end_ts, label) = get_range_for_period(period);
    run_report(db, start_ts, end_ts, label, details)
}

#[allow(dead_code)]
pub fn run_today_report(db: &Database) -> Result<(), Box<dyn std::error::Error>> {
    run_report_for_period(db, ReportPeriod::Today, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_units() {
        assert_eq!(parse_duration("3d").unwrap(), 3 * 86400);
        assert_eq!(parse_duration("12h").unwrap(), 12 * 3600);
        assert_eq!(parse_duration("45m").unwrap(), 45 * 60);
        assert_eq!(parse_duration("30s").unwrap(), 30);
        assert_eq!(
            parse_duration("1d 2h 30m").unwrap(),
            86400 + 2 * 3600 + 30 * 60
        );
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("0m").is_err());
        assert!(parse_duration("10").is_err());
    }

    #[test]
    fn test_date_to_day_bounds() {
        let date = NaiveDate::from_ymd_opt(2026, 5, 15).unwrap();
        let (start, end) = date_to_day_bounds(date);
        assert_eq!(end - start, 86399);
    }

    #[test]
    fn test_resolve_date_filter_specific_date() {
        let cli = Cli::try_parse_from(["waytime", "--date", "2026-05-15"]).unwrap();
        let (start, end, label) = cli.resolve_filter().unwrap();
        assert_eq!(label, "2026-05-15");
        let date = NaiveDate::from_ymd_opt(2026, 5, 15).unwrap();
        let (expected_start, expected_end) = date_to_day_bounds(date);
        assert_eq!(start, expected_start);
        assert_eq!(end, expected_end);
    }

    #[test]
    fn test_resolve_date_filter_today_and_yesterday() {
        let cli_today = Cli::try_parse_from(["waytime", "--date", "today"]).unwrap();
        let (start_t, end_t, label_t) = cli_today.resolve_filter().unwrap();
        assert_eq!(label_t, "Today");
        let today = Local::now().date_naive();
        let (expected_start, expected_end) = date_to_day_bounds(today);
        assert_eq!(start_t, expected_start);
        assert_eq!(end_t, expected_end);

        let cli_yesterday = Cli::try_parse_from(["waytime", "--date", "yesterday"]).unwrap();
        let (start_y, end_y, label_y) = cli_yesterday.resolve_filter().unwrap();
        assert_eq!(label_y, "Yesterday");
        let yesterday = Local::now().date_naive() - chrono::Duration::days(1);
        let (expected_start_y, expected_end_y) = date_to_day_bounds(yesterday);
        assert_eq!(start_y, expected_start_y);
        assert_eq!(end_y, expected_end_y);
    }

    #[test]
    fn test_resolve_date_filter_invalid() {
        let cli = Cli {
            command: None,
            yesterday: false,
            week: false,
            month: false,
            date: Some("not-a-date".to_string()),
            from: None,
            to: None,
            since: None,
            details: false,
            config: None,
            config_path: false,
            json: false,
        };
        assert!(cli.resolve_filter().is_err());
    }

    #[test]
    fn test_resolve_from_to() {
        let cli =
            Cli::try_parse_from(["waytime", "--from", "2026-05-01", "--to", "2026-05-10"]).unwrap();
        let (start, end, label) = cli.resolve_filter().unwrap();
        assert_eq!(label, "2026-05-01 to 2026-05-10");
        let d1 = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
        assert_eq!(start, date_to_day_bounds(d1).0);
        assert_eq!(end, date_to_day_bounds(d2).1);

        // from without to
        let cli_no_to = Cli::try_parse_from(["waytime", "--from", "2026-05-01"]).unwrap();
        let (start_no_to, end_no_to, label_no_to) = cli_no_to.resolve_filter().unwrap();
        assert_eq!(label_no_to, "2026-05-01 to now");
        assert_eq!(start_no_to, date_to_day_bounds(d1).0);
        assert!(end_no_to <= Local::now().timestamp());

        // from after to
        let cli_invalid_range = Cli {
            command: None,
            yesterday: false,
            week: false,
            month: false,
            date: None,
            from: Some("2026-05-10".to_string()),
            to: Some("2026-05-01".to_string()),
            since: None,
            details: false,
            config: None,
            config_path: false,
            json: false,
        };
        assert!(cli_invalid_range.resolve_filter().is_err());
    }

    #[test]
    fn test_resolve_since() {
        let before = Local::now().timestamp();
        let cli = Cli::try_parse_from(["waytime", "--since", "3d"]).unwrap();
        let (start, end, label) = cli.resolve_filter().unwrap();
        let after = Local::now().timestamp();
        assert_eq!(label, "3d");
        assert!(start <= end - (3 * 86400));
        assert!(end >= before && end <= after);
    }

    #[test]
    fn test_fallback_periods() {
        let cli_default = Cli::try_parse_from(["waytime"]).unwrap();
        let (_, _, label) = cli_default.resolve_filter().unwrap();
        assert_eq!(label, "Today");

        let cli_week = Cli::try_parse_from(["waytime", "-w"]).unwrap();
        let (_, _, label_w) = cli_week.resolve_filter().unwrap();
        assert_eq!(label_w, "Week");

        let cli_month = Cli::try_parse_from(["waytime", "-m"]).unwrap();
        let (_, _, label_m) = cli_month.resolve_filter().unwrap();
        assert_eq!(label_m, "Month");

        let cli_yesterday = Cli::try_parse_from(["waytime", "-y"]).unwrap();
        let (_, _, label_y) = cli_yesterday.resolve_filter().unwrap();
        assert_eq!(label_y, "Yesterday");
    }

    #[test]
    fn test_clap_mutual_exclusion() {
        assert!(Cli::try_parse_from(["waytime", "--date", "2026-05-15", "-w"]).is_err());
        assert!(Cli::try_parse_from(["waytime", "--date", "2026-05-15", "--since", "3d"]).is_err());
        assert!(
            Cli::try_parse_from(["waytime", "--date", "2026-05-15", "--from", "2026-05-01"])
                .is_err()
        );
        assert!(Cli::try_parse_from(["waytime", "--since", "3d", "-w"]).is_err());
        assert!(Cli::try_parse_from(["waytime", "--from", "2026-05-01", "-w"]).is_err());
        assert!(Cli::try_parse_from(["waytime", "--to", "2026-05-01"]).is_err());
        assert!(Cli::try_parse_from(["waytime", "-w", "-m"]).is_err());
        assert!(
            Cli::try_parse_from(["waytime", "--from", "2026-05-01", "--to", "2026-05-05"]).is_ok()
        );
    }
}
