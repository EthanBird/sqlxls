use crate::args::Args;
use crate::functions::{ExecCtx, FuncOutput, TableFunction};
use crate::ingest::{ingest_rows, Cell, IngestOpts};
use anyhow::{Context, Result};
use std::path::Path;

pub struct ReadCsvExt;

impl TableFunction for ReadCsvExt {
    fn names(&self) -> &'static [&'static str] {
        &["read_csv", "readcsv"]
    }

    fn execute(&self, ctx: &mut ExecCtx, args: &Args) -> Result<FuncOutput> {
        let path = args.require_str(0, &["path", "file"], "CSV 路径")?;
        let skip = args.get_usize(2, &["skip", "skiprows"], 0);
        let force_str = args.get_bool(3, &["str", "force_str"]);
        let delim = args
            .get_str(1, &["delim", "delimiter", "sep"])
            .map(|s| parse_delim(&s));
        load_csv_path(
            ctx.conn,
            &ctx.dest_table,
            &path,
            delim,
            skip,
            force_str,
            false,
        )?;
        Ok(FuncOutput::Table)
    }
}

pub fn parse_delim(s: &str) -> u8 {
    match s {
        "\\t" | "tab" | "\t" => b'\t',
        "pipe" | "|" => b'|',
        "semicolon" | ";" => b';',
        other if other.len() == 1 => other.as_bytes()[0],
        _ => b',',
    }
}

pub fn sniff_delim(sample: &str) -> u8 {
    if sample.contains('\t') {
        b'\t'
    } else if sample.contains(';') && sample.matches(';').count() >= sample.matches(',').count() {
        b';'
    } else {
        b','
    }
}

/// 从完整文本加载 CSV（剪贴板、小文件、HTTP body）。
pub fn load_csv_text(
    conn: &mut rusqlite::Connection,
    table: &str,
    text: &str,
    delim: Option<u8>,
    skip: usize,
    force_str: bool,
    append: bool,
) -> Result<usize> {
    let skipped = skip_lines(text, skip);
    let delim = delim.unwrap_or_else(|| sniff_delim(skipped));
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delim)
        .flexible(true)
        .from_reader(skipped.as_bytes());
    let headers: Vec<String> = rdr
        .headers()
        .with_context(|| "CSV 表头读取失败")?
        .iter()
        .map(|s| s.to_string())
        .collect();
    if headers.is_empty() {
        anyhow::bail!("CSV 没有表头");
    }
    let mut rows = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        let mut row = Vec::with_capacity(headers.len());
        for i in 0..headers.len() {
            let v = rec.get(i).unwrap_or("");
            row.push(csv_cell(v, force_str));
        }
        rows.push(row);
    }
    ingest_rows(
        conn,
        table,
        &headers,
        rows,
        IngestOpts { force_str, append },
    )
}

pub fn load_csv_path(
    conn: &mut rusqlite::Connection,
    table: &str,
    path: &str,
    delim: Option<u8>,
    skip: usize,
    force_str: bool,
    append: bool,
) -> Result<usize> {
    let text = std::fs::read_to_string(path).with_context(|| format!("无法读取 CSV: {}", path))?;
    let delim = delim.or_else(|| {
        Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| e.eq_ignore_ascii_case("tsv"))
            .map(|_| b'\t')
    });
    load_csv_text(conn, table, &text, delim, skip, force_str, append)
}

fn skip_lines(text: &str, skip: usize) -> &str {
    if skip == 0 {
        return text;
    }
    let mut rest = text;
    for _ in 0..skip {
        if let Some(pos) = rest.find('\n') {
            rest = &rest[pos + 1..];
        } else {
            return "";
        }
    }
    rest
}

fn csv_cell(v: &str, force_str: bool) -> Cell {
    if v.is_empty() {
        return Cell::Null;
    }
    if force_str {
        return Cell::Text(v.to_string());
    }
    if let Ok(i) = v.parse::<i64>() {
        return Cell::Int(i);
    }
    if let Ok(f) = v.parse::<f64>() {
        if v.contains('.') || v.contains('e') || v.contains('E') {
            return Cell::Real(f);
        }
    }
    match v.to_ascii_lowercase().as_str() {
        "true" => Cell::Bool(true),
        "false" => Cell::Bool(false),
        _ => Cell::Text(v.to_string()),
    }
}
