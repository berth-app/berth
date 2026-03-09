use chrono::{DateTime, Utc};

/// Simple cron-like expression parser.
/// Supports: "@every <N>s|m|h", "@hourly", "@daily", "@weekly", or "M H * * *" (minute hour).
pub fn parse_next_run(expr: &str, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let expr = expr.trim();

    if let Some(interval) = expr.strip_prefix("@every ") {
        let interval = interval.trim();
        let secs = parse_duration_secs(interval)?;
        return Some(from + chrono::Duration::seconds(secs));
    }

    match expr {
        "@hourly" => Some(from + chrono::Duration::hours(1)),
        "@daily" => Some(from + chrono::Duration::days(1)),
        "@weekly" => Some(from + chrono::Duration::weeks(1)),
        _ => {
            // Simple "M H * * *" parsing (minute, hour)
            let parts: Vec<&str> = expr.split_whitespace().collect();
            if parts.len() >= 2 {
                let min: u32 = parts[0].parse().ok()?;
                let hour: u32 = parts[1].parse().ok()?;
                if min < 60 && hour < 24 {
                    let today = from.date_naive();
                    let time = chrono::NaiveTime::from_hms_opt(hour, min, 0)?;
                    let candidate = today.and_time(time).and_utc();
                    if candidate > from {
                        return Some(candidate);
                    }
                    // Next day
                    let tomorrow = today + chrono::Duration::days(1);
                    return Some(tomorrow.and_time(time).and_utc());
                }
            }
            None
        }
    }
}

fn parse_duration_secs(s: &str) -> Option<i64> {
    if let Some(n) = s.strip_suffix('s') {
        return n.trim().parse().ok();
    }
    if let Some(n) = s.strip_suffix('m') {
        return n.trim().parse::<i64>().ok().map(|n| n * 60);
    }
    if let Some(n) = s.strip_suffix('h') {
        return n.trim().parse::<i64>().ok().map(|n| n * 3600);
    }
    // Bare number = seconds
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_parse_every_seconds() {
        let now = Utc::now();
        let next = parse_next_run("@every 30s", now).unwrap();
        assert!((next - now).num_seconds() == 30);
    }

    #[test]
    fn test_parse_every_minutes() {
        let now = Utc::now();
        let next = parse_next_run("@every 5m", now).unwrap();
        assert!((next - now).num_seconds() == 300);
    }

    #[test]
    fn test_parse_every_hours() {
        let now = Utc::now();
        let next = parse_next_run("@every 2h", now).unwrap();
        assert!((next - now).num_seconds() == 7200);
    }

    #[test]
    fn test_parse_hourly() {
        let now = Utc::now();
        let next = parse_next_run("@hourly", now).unwrap();
        assert!((next - now).num_seconds() == 3600);
    }

    #[test]
    fn test_parse_daily() {
        let now = Utc::now();
        let next = parse_next_run("@daily", now).unwrap();
        assert!((next - now).num_seconds() == 86400);
    }
}
