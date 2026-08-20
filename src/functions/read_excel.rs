use crate::args::Args;
use crate::functions::{ExecCtx, FuncOutput, TableFunction};
use crate::ingest::{ingest_rows, Cell, IngestOpts};
use crate::schema::{table_width, HeaderSpec};
use anyhow::{Context, Result};
use calamine::{open_workbook_auto, Data, Reader};

pub struct ReadExcelExt;

impl TableFunction for ReadExcelExt {
    fn names(&self) -> &'static [&'static str] {
        &["read_excel", "readexcel"]
    }

    fn execute(&self, ctx: &mut ExecCtx, args: &Args) -> Result<FuncOutput> {
        let spec = parse_excel_args(args)?;
        let add_source = crate::ingest::include_source(args);
        let all_sheets = spec
            .sheet
            .as_deref()
            .map(|s| s == "*" || s.eq_ignore_ascii_case("all"))
            .unwrap_or(false);

        if all_sheets {
            let names = excel_sheet_names(&spec.path)?;
            if names.is_empty() {
                anyhow::bail!("Excel 文件中没有任何表格");
            }
            let mut first = true;
            for sheet in &names {
                let (mut headers, mut rows) = excel_to_frame(
                    &spec.path,
                    Some(sheet),
                    spec.skip,
                    spec.force_str,
                    &spec.header,
                )?;
                if add_source {
                    crate::ingest::attach_const_column(
                        &mut headers,
                        &mut rows,
                        "_sheet",
                        Cell::Text(sheet.clone()),
                    );
                }
                ingest_rows(
                    ctx.conn,
                    &ctx.dest_table,
                    &headers,
                    rows,
                    IngestOpts {
                        force_str: spec.force_str,
                        append: !first,
                    },
                )?;
                first = false;
            }
            return Ok(FuncOutput::Table);
        }

        let (headers, rows) = excel_to_frame(
            &spec.path,
            spec.sheet.as_deref(),
            spec.skip,
            spec.force_str,
            &spec.header,
        )?;
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
    pub header: HeaderSpec,
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
        header: HeaderSpec::from_args(args)?,
    })
}

pub fn excel_to_frame(
    path: &str,
    sheet_opt: Option<&str>,
    skip_rows: usize,
    force_str: bool,
    header: &HeaderSpec,
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
    let file_headers = if header.has_header {
        let header_row = iter
            .next()
            .context("跳过指定行后没有任何数据（无表头）。若第一行就是数据，请写 header=false")?;
        Some(header_row.iter().map(header_cell_text).collect::<Vec<_>>())
    } else {
        None
    };

    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut data_width = 0usize;
    for row in iter {
        data_width = data_width.max(row.len());
        rows.push(row.iter().map(|c| data_to_cell(c, force_str)).collect());
    }

    let width = table_width(header, file_headers.as_ref().map(|h| h.len()), data_width);
    if width == 0 {
        anyhow::bail!(
            "Excel 没有数据。无表头文件请写 header=false，自定义列名用 columns='id,name'"
        );
    }
    let headers = header.resolve(width, file_headers.as_deref());
    for row in &mut rows {
        if row.len() > width {
            row.truncate(width);
        } else {
            row.resize(width, Cell::Null);
        }
    }
    Ok((headers, rows))
}

fn header_cell_text(cell: &Data) -> String {
    match cell {
        Data::String(s) if !s.trim().is_empty() => s.trim().to_string(),
        Data::Int(n) => n.to_string(),
        Data::Float(f) => f.to_string(),
        _ => String::new(),
    }
}

pub fn excel_sheet_names(path: &str) -> Result<Vec<String>> {
    let workbook = open_workbook_auto(path).with_context(|| format!("无法打开 Excel: {}", path))?;
    Ok(workbook.sheet_names().to_vec())
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
        crate::dates::excel_serial_display(d.as_f64())
    }
}
