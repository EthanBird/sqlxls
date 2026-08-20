//! 日期区间展开（Source 层）与查询层标量：`parse_date` / `from_unix` 等。
//!
//! 查询函数失败返回 NULL，不中断整条 SELECT。未知格式名才会报错。

use crate::syntax::ForDateStep;
use anyhow::{bail, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime};
use rusqlite::functions::{Context, FunctionFlags};
use rusqlite::types::ValueRef;
use rusqlite::Connection;

const MAX_VALUES: usize = 4000;
const UNIX_MS_THRESHOLD: f64 = 1_000_000_000_000.0;

#[derive(Clone, Copy, Debug)]
enum DateStyle {
    IsoDay,
    SlashDay,
    CompactDay,
    IsoMonth,
    SlashMonth,
    CompactMonth,
}

#[derive(Clone, Copy, Debug)]
enum Endpoint {
    Day(NaiveDate),
    Month { year: i32, month: u32 },
}

/// 展开 `FOR d IN start..end [STEP …]`。输出格式跟起始值一致（ISO / 紧凑 / 斜杠）。
pub fn expand_range(start: &str, end: &str, step: ForDateStep) -> Result<Vec<String>> {
    let start = start.trim();
    let end = end.trim();
    let a = parse_endpoint(start)?;
    let b = parse_endpoint(end)?;
    let style = detect_style(start).unwrap_or(style_for(&a));
    match (a, b) {
        (Endpoint::Day(s), Endpoint::Day(e)) => {
            let dates = expand_days(s, e, resolve_day_step(step)?)?;
            Ok(dates.into_iter().map(|d| format_day(d, style)).collect())
        }
        (
            Endpoint::Month {
                year: ys,
                month: ms,
            },
            Endpoint::Month {
                year: ye,
                month: me,
            },
        ) => {
            let months = expand_months(ys, ms, ye, me, resolve_month_step(step)?)?;
            Ok(months
                .into_iter()
                .map(|(y, m)| format_month(y, m, style))
                .collect())
        }
        _ => bail!("日期区间两端粒度必须相同：要么都是日（2024-01-15），要么都是月（2024-01）"),
    }
}

fn resolve_day_step(step: ForDateStep) -> Result<DayStep> {
    match step {
        ForDateStep::Default => Ok(DayStep::Days(1)),
        ForDateStep::Count(n) => {
            if n == 0 {
                bail!("FOR 范围的 STEP 不能为 0");
            }
            Ok(DayStep::Days(n))
        }
        ForDateStep::Months(n) => {
            if n == 0 {
                bail!("FOR 范围的 STEP 不能为 0");
            }
            Ok(DayStep::Months(n))
        }
    }
}

fn resolve_month_step(step: ForDateStep) -> Result<i64> {
    let n = match step {
        ForDateStep::Default => 1,
        ForDateStep::Count(n) | ForDateStep::Months(n) => n,
    };
    if n == 0 {
        bail!("FOR 范围的 STEP 不能为 0");
    }
    Ok(n)
}

enum DayStep {
    Days(i64),
    Months(i64),
}

fn expand_days(start: NaiveDate, end: NaiveDate, step: DayStep) -> Result<Vec<NaiveDate>> {
    let mut out = Vec::new();
    match step {
        DayStep::Days(n) => {
            check_direction(start <= end, n > 0, start, end)?;
            let mut d = start;
            while if n > 0 { d <= end } else { d >= end } {
                push_date(&mut out, d)?;
                let Some(next) = d.checked_add_signed(Duration::days(n)) else {
                    bail!("日期溢出");
                };
                if next == d {
                    bail!("日期步长没有前进");
                }
                d = next;
            }
        }
        DayStep::Months(n) => {
            check_direction(start <= end, n > 0, start, end)?;
            let n_i32 = i32::try_from(n).map_err(|_| anyhow::anyhow!("STEP MONTH 过大"))?;
            let anchor = start.day();
            let mut i: i32 = 0;
            loop {
                let Some(d) = add_months_anchored(start, i * n_i32, anchor) else {
                    bail!("日期溢出");
                };
                if n > 0 {
                    if d > end {
                        break;
                    }
                } else if d < end {
                    break;
                }
                push_date(&mut out, d)?;
                i += 1;
                if i > MAX_VALUES as i32 {
                    bail!("日期区间超过 {MAX_VALUES} 个值。请缩小范围，或用 STEP 7 / STEP MONTH");
                }
            }
        }
    }
    Ok(out)
}

fn check_direction(
    start_le_end: bool,
    step_pos: bool,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<()> {
    if step_pos && !start_le_end {
        bail!("日期区间起始晚于结束：{start} .. {end}。倒序请用负 STEP");
    }
    if !step_pos && start_le_end && start != end {
        bail!("负 STEP 时起始应晚于结束：{start} .. {end}");
    }
    Ok(())
}

fn expand_months(ys: i32, ms: u32, ye: i32, me: u32, step: i64) -> Result<Vec<(i32, u32)>> {
    let start = ym_index(ys, ms)?;
    let end = ym_index(ye, me)?;
    if step > 0 && start > end {
        bail!(
            "月份区间起始晚于结束：{:04}-{:02} .. {:04}-{:02}。倒序请用负 STEP",
            ys,
            ms,
            ye,
            me
        );
    }
    if step < 0 && start < end {
        bail!(
            "负 STEP 时起始应晚于结束：{:04}-{:02} .. {:04}-{:02}",
            ys,
            ms,
            ye,
            me
        );
    }
    let mut out = Vec::new();
    let mut cur = start;
    loop {
        push_cap(&mut out)?;
        out.push(from_ym_index(cur));
        let next = cur
            .checked_add(step)
            .ok_or_else(|| anyhow::anyhow!("月份溢出"))?;
        if step > 0 {
            if next > end {
                break;
            }
        } else if next < end {
            break;
        }
        if next == cur {
            bail!("月份步长没有前进");
        }
        cur = next;
    }
    Ok(out)
}

fn ym_index(y: i32, m: u32) -> Result<i64> {
    if !(1..=12).contains(&m) {
        bail!("无效月份 {m}");
    }
    Ok(y as i64 * 12 + (m as i64 - 1))
}

fn from_ym_index(i: i64) -> (i32, u32) {
    let y = i.div_euclid(12);
    let m = i.rem_euclid(12) + 1;
    (y as i32, m as u32)
}

fn push_date(out: &mut Vec<NaiveDate>, d: NaiveDate) -> Result<()> {
    push_cap(out)?;
    out.push(d);
    Ok(())
}

fn push_cap<T>(out: &Vec<T>) -> Result<()> {
    if out.len() >= MAX_VALUES {
        bail!("日期区间超过 {MAX_VALUES} 个值。请缩小范围，或用 STEP 7 / STEP MONTH");
    }
    Ok(())
}

fn add_months_anchored(start: NaiveDate, n: i32, anchor_day: u32) -> Option<NaiveDate> {
    let mut y = start.year();
    let mut m = start.month() as i32 + n;
    while m > 12 {
        m -= 12;
        y = y.checked_add(1)?;
    }
    while m < 1 {
        m += 12;
        y = y.checked_sub(1)?;
    }
    let last = last_day_of_month(y, m as u32)?;
    NaiveDate::from_ymd_opt(y, m as u32, anchor_day.min(last))
}

fn last_day_of_month(y: i32, m: u32) -> Option<u32> {
    let (ny, nm) = if m == 12 {
        (y.checked_add(1)?, 1)
    } else {
        (y, m + 1)
    };
    let first_next = NaiveDate::from_ymd_opt(ny, nm, 1)?;
    Some(first_next.pred_opt()?.day())
}

fn parse_endpoint(s: &str) -> Result<Endpoint> {
    if let Some(d) = parse_day_strict(s) {
        return Ok(Endpoint::Day(d));
    }
    if let Some((year, month)) = parse_month_strict(s) {
        return Ok(Endpoint::Month { year, month });
    }
    bail!("无法把 `{s}` 解析为日期或月份。支持 2024-01-15、2024/01/15、20240115、2024-01、202401");
}

fn parse_day_strict(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .or_else(|| NaiveDate::parse_from_str(s, "%Y/%m/%d").ok())
        .or_else(|| {
            if s.len() == 8 && s.bytes().all(|c| c.is_ascii_digit()) {
                NaiveDate::parse_from_str(s, "%Y%m%d").ok()
            } else {
                None
            }
        })
}

fn parse_month_strict(s: &str) -> Option<(i32, u32)> {
    if s.len() == 7 && s.as_bytes().get(4) == Some(&b'-') {
        let y: i32 = s[..4].parse().ok()?;
        let m: u32 = s[5..].parse().ok()?;
        if (1..=12).contains(&m) {
            return Some((y, m));
        }
    }
    if s.len() == 7 && s.as_bytes().get(4) == Some(&b'/') {
        let y: i32 = s[..4].parse().ok()?;
        let m: u32 = s[5..].parse().ok()?;
        if (1..=12).contains(&m) {
            return Some((y, m));
        }
    }
    if s.len() == 6 && s.bytes().all(|c| c.is_ascii_digit()) {
        let y: i32 = s[..4].parse().ok()?;
        let m: u32 = s[4..].parse().ok()?;
        if (1..=12).contains(&m) {
            return Some((y, m));
        }
    }
    None
}

fn detect_style(s: &str) -> Option<DateStyle> {
    if parse_day_strict(s).is_some() {
        return Some(if s.contains('-') {
            DateStyle::IsoDay
        } else if s.contains('/') {
            DateStyle::SlashDay
        } else {
            DateStyle::CompactDay
        });
    }
    if parse_month_strict(s).is_some() {
        return Some(if s.contains('-') {
            DateStyle::IsoMonth
        } else if s.contains('/') {
            DateStyle::SlashMonth
        } else {
            DateStyle::CompactMonth
        });
    }
    None
}

fn style_for(ep: &Endpoint) -> DateStyle {
    match ep {
        Endpoint::Day(_) => DateStyle::IsoDay,
        Endpoint::Month { .. } => DateStyle::IsoMonth,
    }
}

fn format_day(d: NaiveDate, style: DateStyle) -> String {
    match style {
        DateStyle::SlashDay => d.format("%Y/%m/%d").to_string(),
        DateStyle::CompactDay => d.format("%Y%m%d").to_string(),
        _ => d.format("%Y-%m-%d").to_string(),
    }
}

fn format_month(y: i32, m: u32, style: DateStyle) -> String {
    match style {
        DateStyle::SlashMonth => format!("{y:04}/{m:02}"),
        DateStyle::CompactMonth => format!("{y:04}{m:02}"),
        _ => format!("{y:04}-{m:02}"),
    }
}

// --- 查询层解析 ---

/// `parse_date`：成功返回 `YYYY-MM-DD`。
pub fn parse_date(input: &str, fmt: Option<&str>) -> std::result::Result<Option<String>, String> {
    Ok(parse_date_naive(input, fmt)?.map(|d| d.format("%Y-%m-%d").to_string()))
}

/// `parse_datetime`：成功返回 `YYYY-MM-DD HH:MM:SS`（UTC 语义，无时区偏移）。
pub fn parse_datetime(
    input: &str,
    fmt: Option<&str>,
) -> std::result::Result<Option<String>, String> {
    Ok(parse_datetime_naive(input, fmt)?.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string()))
}

pub fn from_unix(n: f64) -> Option<String> {
    datetime_from_unix(n).map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
}

pub fn to_unix(input: &str) -> Option<i64> {
    let dt = parse_datetime_naive(input, None).ok().flatten()?;
    Some(dt.signed_duration_since(unix_epoch()).num_seconds())
}

/// Excel 序列（Windows / pandas 常用：以 1899-12-30 为 0）。
pub fn excel_serial_to_iso(serial: f64) -> Option<String> {
    if !serial.is_finite() {
        return None;
    }
    let days = serial.trunc() as i64;
    let frac_secs = (serial.fract().abs() * 86400.0).round() as i64;
    let base = NaiveDate::from_ymd_opt(1899, 12, 30)?;
    let date = base.checked_add_signed(Duration::days(days))?;
    if frac_secs == 0 {
        Some(date.format("%Y-%m-%d").to_string())
    } else {
        let h = frac_secs / 3600;
        let m = (frac_secs % 3600) / 60;
        let s = frac_secs % 60;
        Some(format!(
            "{} {:02}:{:02}:{:02}",
            date.format("%Y-%m-%d"),
            h,
            m,
            s
        ))
    }
}

pub fn excel_serial_display(serial: f64) -> String {
    excel_serial_to_iso(serial).unwrap_or_else(|| serial.to_string())
}

fn parse_date_naive(
    input: &str,
    fmt: Option<&str>,
) -> std::result::Result<Option<NaiveDate>, String> {
    let s = input.trim();
    if s.is_empty() {
        return Ok(None);
    }
    match fmt.map(str::trim).filter(|f| !f.is_empty()) {
        None => Ok(parse_date_auto(s)),
        Some(f) => parse_date_with_fmt(s, f),
    }
}

fn parse_datetime_naive(
    input: &str,
    fmt: Option<&str>,
) -> std::result::Result<Option<NaiveDateTime>, String> {
    let s = input.trim();
    if s.is_empty() {
        return Ok(None);
    }
    match fmt.map(str::trim).filter(|f| !f.is_empty()) {
        None => Ok(parse_datetime_auto(s)),
        Some(f) => parse_datetime_with_fmt(s, f),
    }
}

fn parse_date_auto(s: &str) -> Option<NaiveDate> {
    if let Some(d) = parse_day_strict(s) {
        return Some(d);
    }
    if s.len() > 10 {
        let sep = s.as_bytes().get(10).copied();
        if sep == Some(b'T') || sep == Some(b' ') {
            if let Some(d) = parse_day_strict(&s[..10]) {
                return Some(d);
            }
        }
    }
    parse_numeric_slash_date(s)
}

fn parse_numeric_slash_date(s: &str) -> Option<NaiveDate> {
    let sep = if s.contains('/') {
        '/'
    } else if s.matches('-').count() == 2 {
        '-'
    } else {
        return None;
    };
    let parts: Vec<&str> = s.split(sep).collect();
    if parts.len() != 3 {
        return None;
    }
    let a: u32 = parts[0].parse().ok()?;
    let b: u32 = parts[1].parse().ok()?;
    let mut y: i32 = parts[2].parse().ok()?;
    if parts[0].len() == 4 {
        return NaiveDate::from_ymd_opt(a as i32, b, y as u32);
    }
    if y < 100 {
        y += 2000;
    }
    if a > 12 && b <= 12 {
        NaiveDate::from_ymd_opt(y, b, a)
    } else if b > 12 && a <= 12 {
        NaiveDate::from_ymd_opt(y, a, b)
    } else {
        None
    }
}

fn parse_date_with_fmt(s: &str, fmt: &str) -> std::result::Result<Option<NaiveDate>, String> {
    let patterns = named_date_patterns(fmt)?;
    for p in patterns {
        if let Ok(d) = NaiveDate::parse_from_str(s, p) {
            return Ok(Some(d));
        }
    }
    if fmt.contains('%') {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(Some(dt.date()));
        }
        return Ok(NaiveDate::parse_from_str(s, fmt).ok());
    }
    Ok(None)
}

fn named_date_patterns(fmt: &str) -> std::result::Result<Vec<&'static str>, String> {
    let key = fmt.to_ascii_lowercase();
    Ok(match key.as_str() {
        "iso" => vec!["%Y-%m-%d"],
        "ymd" => vec!["%Y-%m-%d", "%Y/%m/%d", "%Y%m%d"],
        "yyyymmdd" => vec!["%Y%m%d"],
        "dmy" => vec!["%d/%m/%Y", "%d-%m-%Y", "%d.%m.%Y"],
        "mdy" => vec!["%m/%d/%Y", "%m-%d-%Y", "%m.%d.%Y"],
        _ if fmt.contains('%') => Vec::new(),
        _ => {
            return Err(format!(
                "未知日期格式 `{fmt}`。可用 iso / ymd / yyyymmdd / dmy / mdy，或 chrono 模板如 %Y-%m-%d"
            ))
        }
    })
}

fn parse_datetime_auto(s: &str) -> Option<NaiveDateTime> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.naive_utc());
    }
    const PATS: &[&str] = &[
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y/%m/%d %H:%M:%S",
        "%Y%m%d%H%M%S",
        "%Y%m%d %H%M%S",
    ];
    for p in PATS {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, p) {
            return Some(dt);
        }
    }
    if let Some(d) = parse_date_auto(s) {
        return d.and_hms_opt(0, 0, 0);
    }
    if s.bytes().all(|c| c.is_ascii_digit() || c == b'-') {
        if let Ok(n) = s.parse::<f64>() {
            return datetime_from_unix(n);
        }
    }
    None
}

fn parse_datetime_with_fmt(
    s: &str,
    fmt: &str,
) -> std::result::Result<Option<NaiveDateTime>, String> {
    let key = fmt.to_ascii_lowercase();
    if key == "rfc3339" || key == "iso" {
        return Ok(DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.naive_utc())
            .or_else(|| parse_datetime_auto(s)));
    }
    if key == "unix" || key == "epoch" {
        return Ok(s.parse::<f64>().ok().and_then(datetime_from_unix));
    }
    if let Ok(patterns) = named_date_patterns(fmt) {
        for p in patterns {
            if let Ok(d) = NaiveDate::parse_from_str(s, p) {
                return Ok(d.and_hms_opt(0, 0, 0));
            }
        }
    }
    if fmt.contains('%') {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(Some(dt));
        }
        if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
            return Ok(d.and_hms_opt(0, 0, 0));
        }
        return Ok(None);
    }
    if named_date_patterns(fmt).is_err()
        && !matches!(key.as_str(), "rfc3339" | "iso" | "unix" | "epoch")
    {
        return Err(format!(
            "未知时间格式 `{fmt}`。可用 rfc3339 / iso / unix / ymd / dmy / mdy，或 chrono 模板"
        ));
    }
    Ok(None)
}

fn unix_epoch() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(1970, 1, 1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .expect("unix epoch")
}

fn datetime_from_unix(n: f64) -> Option<NaiveDateTime> {
    if !n.is_finite() {
        return None;
    }
    let dur = if n.abs() >= UNIX_MS_THRESHOLD {
        Duration::milliseconds(n as i64)
    } else {
        Duration::seconds(n as i64)
    };
    unix_epoch().checked_add_signed(dur)
}

pub fn register(conn: &Connection) -> Result<()> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    conn.create_scalar_function("parse_date", 1, flags, |ctx| sql_parse_date(&ctx, false))
        .map_err(reg_err)?;
    conn.create_scalar_function("parse_date", 2, flags, |ctx| sql_parse_date(&ctx, true))
        .map_err(reg_err)?;
    conn.create_scalar_function("parse_datetime", 1, flags, |ctx| {
        sql_parse_datetime(&ctx, false)
    })
    .map_err(reg_err)?;
    conn.create_scalar_function("parse_datetime", 2, flags, |ctx| {
        sql_parse_datetime(&ctx, true)
    })
    .map_err(reg_err)?;
    conn.create_scalar_function("from_unix", 1, flags, |ctx| {
        let n = arg_as_f64(&ctx, 0)?;
        Ok(n.and_then(from_unix))
    })
    .map_err(reg_err)?;
    conn.create_scalar_function("to_unix", 1, flags, |ctx| {
        let s = arg_as_string(&ctx, 0)?;
        Ok(s.as_deref().and_then(to_unix))
    })
    .map_err(reg_err)?;
    conn.create_scalar_function("excel_serial", 1, flags, |ctx| {
        let n = arg_as_f64(&ctx, 0)?;
        Ok(n.and_then(excel_serial_to_iso))
    })
    .map_err(reg_err)?;
    Ok(())
}

fn reg_err(e: rusqlite::Error) -> anyhow::Error {
    anyhow::anyhow!("注册日期函数失败: {e}")
}

fn sql_parse_date(ctx: &Context<'_>, with_fmt: bool) -> rusqlite::Result<Option<String>> {
    let s = arg_as_string(ctx, 0)?;
    let fmt = if with_fmt {
        arg_as_string(ctx, 1)?
    } else {
        None
    };
    match s {
        None => Ok(None),
        Some(s) => {
            parse_date(&s, fmt.as_deref()).map_err(|e| rusqlite::Error::UserFunctionError(e.into()))
        }
    }
}

fn sql_parse_datetime(ctx: &Context<'_>, with_fmt: bool) -> rusqlite::Result<Option<String>> {
    let s = arg_as_string(ctx, 0)?;
    let fmt = if with_fmt {
        arg_as_string(ctx, 1)?
    } else {
        None
    };
    match s {
        None => Ok(None),
        Some(s) => parse_datetime(&s, fmt.as_deref())
            .map_err(|e| rusqlite::Error::UserFunctionError(e.into())),
    }
}

fn arg_as_string(ctx: &Context<'_>, i: usize) -> rusqlite::Result<Option<String>> {
    match ctx.get_raw(i) {
        ValueRef::Null => Ok(None),
        ValueRef::Text(t) => {
            let s =
                std::str::from_utf8(t).map_err(|e| rusqlite::Error::UserFunctionError(e.into()))?;
            Ok(Some(s.to_string()))
        }
        ValueRef::Integer(n) => Ok(Some(n.to_string())),
        ValueRef::Real(f) => Ok(Some(if f.fract() == 0.0 {
            (f as i64).to_string()
        } else {
            f.to_string()
        })),
        ValueRef::Blob(_) => Ok(None),
    }
}

fn arg_as_f64(ctx: &Context<'_>, i: usize) -> rusqlite::Result<Option<f64>> {
    match ctx.get_raw(i) {
        ValueRef::Null => Ok(None),
        ValueRef::Integer(n) => Ok(Some(n as f64)),
        ValueRef::Real(f) => Ok(Some(f)),
        ValueRef::Text(t) => {
            let s =
                std::str::from_utf8(t).map_err(|e| rusqlite::Error::UserFunctionError(e.into()))?;
            Ok(s.trim().parse().ok())
        }
        ValueRef::Blob(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_range_iso() {
        let v = expand_range("2024-01-01", "2024-01-03", ForDateStep::Default).unwrap();
        assert_eq!(v, ["2024-01-01", "2024-01-02", "2024-01-03"]);
    }

    #[test]
    fn day_range_compact_keeps_style() {
        let v = expand_range("20240101", "20240103", ForDateStep::Count(1)).unwrap();
        assert_eq!(v, ["20240101", "20240102", "20240103"]);
    }

    #[test]
    fn day_range_step_week() {
        let v = expand_range("2024-01-01", "2024-01-15", ForDateStep::Count(7)).unwrap();
        assert_eq!(v, ["2024-01-01", "2024-01-08", "2024-01-15"]);
    }

    #[test]
    fn month_range() {
        let v = expand_range("2024-01", "2024-03", ForDateStep::Default).unwrap();
        assert_eq!(v, ["2024-01", "2024-02", "2024-03"]);
    }

    #[test]
    fn month_step_on_days_clamps() {
        let v = expand_range("2024-01-31", "2024-03-31", ForDateStep::Months(1)).unwrap();
        assert_eq!(v, ["2024-01-31", "2024-02-29", "2024-03-31"]);
    }

    #[test]
    fn parse_formats() {
        assert_eq!(
            parse_date("20240115", None).unwrap().as_deref(),
            Some("2024-01-15")
        );
        assert_eq!(
            parse_date("15/01/2024", Some("dmy")).unwrap().as_deref(),
            Some("2024-01-15")
        );
        assert_eq!(
            parse_date("01/15/2024", Some("mdy")).unwrap().as_deref(),
            Some("2024-01-15")
        );
        assert_eq!(parse_date("01/02/2024", None).unwrap(), None);
        assert_eq!(
            from_unix(1_700_000_000.0).as_deref(),
            Some("2023-11-14 22:13:20")
        );
        assert_eq!(
            from_unix(1_700_000_000_000.0).as_deref(),
            Some("2023-11-14 22:13:20")
        );
        assert_eq!(excel_serial_to_iso(44927.0).as_deref(), Some("2023-01-01"));
        assert!(to_unix("2023-11-14 22:13:20").unwrap() >= 1_699_999_000);
    }

    #[test]
    fn unknown_fmt_errors() {
        let err = parse_date("2024-01-01", Some("nope")).unwrap_err();
        assert!(err.contains("未知日期格式"), "{err}");
    }
}
