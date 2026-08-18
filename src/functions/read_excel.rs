use crate::args::Args;
use crate::functions::{ExecCtx, FuncOutput, TableFunction};
use crate::ingest::{ingest_rows, Cell, IngestOpts};
use crate::schema::unique_column_names;
use anyhow::{Context, Result};
use calamine::{open_workbook_auto, Data, Reader};
use chrono::{Duration, NaiveDate};

pub struct ReadExcelExt;

impl TableFunction for ReadExcelExt {
    fn names(&self) -> &'static [&'static str] {
        &["read_excel", "readexcel"]
    }

    fn execute(&self, ctx: &mut ExecCtx, args: &Args) -> Result<FuncOutput> {
        let spec = parse_excel_args(args)?;
        let (headers, rows) =
            excel_to_frame(&spec.path, spec.sheet.as_deref(), spec.skip, spec.force_str)?;
        ingest_rows(
            ctx.conn,
            &ctx.dest_table,
            &headers,
            rows,
            IngestOpts {
                force_str: spec.force_str,
                append: false,
            },
        )?;
        Ok(FuncOutput::Table)
    }
}

pub struct ExcelSpec {
    pub path: String,
    pub sheet: Option<String>,
    pub skip: usize,
    pub force_str: bool,
}

fn is_str_flag(args: &Args, index: usize) -> bool {
    args.positional.get(index).and_then(|v| v.as_bool()) == Some(true)
}

pub fn parse_excel_args(args: &Args) -> Result<ExcelSpec> {
    let path = args.require_str(0, &["path", "file"], "Excel 路径")?;
    let mut force_str = args.get_bool(99, &["str", "force_str"]) || is_str_flag(args, 3);
    let mut skip = args.get_usize(2, &["skip", "skiprows", "skip_rows"], 0);
    let mut sheet = args
        .get_str(1, &["sheet"])
        .filter(|s| !s.is_empty() && s.as_str() != "null");

    // README 兼容：第三参数可以是 'str' 而不是 skip
    if is_str_flag(args, 1) {
        sheet = None;
        force_str = true;
    }
    if is_str_flag(args, 2) {
        skip = args.get_usize(99, &["skip", "skiprows", "skip_rows"], 0);
        force_str = true;
    }
    if sheet.as_deref() == Some("str") && !args.named.contains_key("sheet") {
        sheet = None;
        force_str = true;
    }

    Ok(ExcelSpec {
        path,
        sheet,
        skip,
        force_str,
    })
}

pub fn excel_to_frame(
    path: &str,
    sheet_opt: Option<&str>,
    skip_rows: usize,
    force_str: bool,
) -> Result<(Vec<String>, Vec<Vec<Cell>>)> {
    let mut workbook =
        open_workbook_auto(path).with_context(|| format!("无法打开 Excel: {}", path))?;
    let sheet_name = match sheet_opt {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => workbook
            .sheet_names()
            .first()
            .cloned()
            .context("Excel 文件中没有任何表格")?,
    };
    let range = workbook
        .worksheet_range(&sheet_name)
        .with_context(|| format!("未找到 Sheet: {}", sheet_name))?;

    let mut iter = range.rows().skip(skip_rows);
    let header_row = iter.next().context("跳过指定行后没有任何数据（无表头）")?;
    let raw_headers: Vec<String> = header_row
        .iter()
        .enumerate()
        .map(|(i, cell)| match cell {
            Data::String(s) if !s.trim().is_empty() => s.trim().to_string(),
            Data::Int(n) => n.to_string(),
            Data::Float(f) => f.to_string(),
            _ => format!("col_{}", i),
        })
        .collect();
    let headers = unique_column_names(raw_headers);
    let width = headers.len();

    let mut rows = Vec::new();
    for row in iter {
        let mut cells = Vec::with_capacity(width);
        for i in 0..width {
            let cell = row.get(i).unwrap_or(&Data::Empty);
            cells.push(data_to_cell(cell, force_str));
        }
        rows.push(cells);
    }
    Ok((headers, rows))
}

fn data_to_cell(cell: &Data, force_str: bool) -> Cell {
    if force_str {
        return match cell {
            Data::Empty | Data::Error(_) => Cell::Null,
            Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::Text(s.clone()),
            Data::Int(i) => Cell::Text(i.to_string()),
            Data::Float(f) => Cell::Text(f.to_string()),
            Data::Bool(b) => Cell::Text(b.to_string()),
            Data::DateTime(d) => Cell::Text(datetime_cell(d)),
        };
    }
    match cell {
        Data::Empty | Data::Error(_) => Cell::Null,
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::Text(s.clone()),
        Data::Int(i) => Cell::Int(*i),
        Data::Float(f) => Cell::Real(*f),
        Data::Bool(b) => Cell::Bool(*b),
        Data::DateTime(d) => {
            if d.is_duration() {
                Cell::Real(d.as_f64())
            } else {
                Cell::Text(datetime_cell(d))
            }
        }
    }
}

fn datetime_cell(d: &calamine::ExcelDateTime) -> String {
    if let Some(dt) = d.as_datetime() {
        let s = dt.format("%Y-%m-%d %H:%M:%S").to_string();
        if s.ends_with(" 00:00:00") {
            s[..10].to_string()
        } else {
            s
        }
    } else {
        excel_serial_to_iso(d.as_f64())
    }
}

fn excel_serial_to_iso(serial: f64) -> String {
    if !serial.is_finite() {
        return serial.to_string();
    }
    let days = serial.trunc() as i64;
    let frac_secs = (serial.fract().abs() * 86400.0).round() as i64;
    let Some(base) = NaiveDate::from_ymd_opt(1899, 12, 30) else {
        return serial.to_string();
    };
    let Some(date) = base.checked_add_signed(Duration::days(days)) else {
        return serial.to_string();
    };
    if frac_secs == 0 {
        date.format("%Y-%m-%d").to_string()
    } else {
        let h = frac_secs / 3600;
        let m = (frac_secs % 3600) / 60;
        let s = frac_secs % 60;
        format!("{} {:02}:{:02}:{:02}", date.format("%Y-%m-%d"), h, m, s)
    }
}
