use crate::args::Args;
use crate::functions::read_csv::load_csv_path;
use crate::functions::read_excel::excel_to_frame;
use crate::functions::read_json::{default_table_value, extract_json_path, value_to_rows};
use crate::functions::{ExecCtx, FuncOutput, TableFunction};
use crate::ingest::{
    attach_const_column, include_source, ingest_rows, union_from_table, Cell, IngestOpts,
};
use crate::schema::HeaderSpec;
use anyhow::{bail, Context, Result};
use glob::glob;
use std::path::Path;

pub struct ReadDirExt;

impl TableFunction for ReadDirExt {
    fn names(&self) -> &'static [&'static str] {
        &["read_dir", "readdir", "read_glob", "readglob"]
    }

    fn execute(&self, ctx: &mut ExecCtx, args: &Args) -> Result<FuncOutput> {
        let pattern = args.require_str(0, &["path", "glob", "pattern"], "通配符路径")?;
        let mut files: Vec<std::path::PathBuf> = glob(&pattern)
            .with_context(|| "通配符路径无效")?
            .filter_map(|e| e.ok())
            .collect();
        files.sort();
        if files.is_empty() {
            bail!("没有匹配到任何文件: {}", pattern);
        }

        let skip = args.get_usize(2, &["skip", "skiprows"], 0);
        let force_str = args.get_bool(3, &["str", "force_str"])
            || args.positional.get(1).and_then(|v| v.as_bool()) == Some(true)
            || args.positional.get(2).and_then(|v| v.as_bool()) == Some(true);
        let sheet = args
            .named
            .get("sheet")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                args.positional.get(1).and_then(|v| {
                    v.as_str()
                        .filter(|s| !s.is_empty() && v.as_bool() != Some(true))
                        .map(|s| s.to_string())
                })
            });
        let json_path = args
            .get_str(99, &["json_path", "pointer"])
            .unwrap_or_default();
        let encoding = args.get_str(99, &["encoding", "charset"]);
        let add_source = include_source(args);
        let header = HeaderSpec::from_args(args)?;

        let mut first = true;
        for path in &files {
            let file_path = path.to_string_lossy().to_string();
            let extras = if add_source {
                vec![("_source".to_string(), file_path.clone())]
            } else {
                vec![]
            };
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            match ext.as_str() {
                "xlsx" | "xls" | "xlsm" => {
                    let (mut headers, mut rows) =
                        excel_to_frame(&file_path, sheet.as_deref(), skip, force_str, &header)?;
                    for (k, v) in &extras {
                        attach_const_column(&mut headers, &mut rows, k, Cell::Text(v.clone()));
                    }
                    ingest_rows(
                        ctx.conn,
                        &ctx.dest_table,
                        &headers,
                        rows,
                        IngestOpts {
                            force_str,
                            append: !first,
                        },
                    )?;
                }
                "csv" | "tsv" => {
                    let delim = if ext == "tsv" { Some(b'\t') } else { None };
                    if extras.is_empty() {
                        load_csv_path(
                            ctx.conn,
                            &ctx.dest_table,
                            &file_path,
                            delim,
                            skip,
                            force_str,
                            !first,
                            encoding.as_deref(),
                            &header,
                        )?;
                    } else {
                        let tmp = format!("{}__glob", ctx.dest_table);
                        load_csv_path(
                            ctx.conn,
                            &tmp,
                            &file_path,
                            delim,
                            skip,
                            force_str,
                            false,
                            encoding.as_deref(),
                            &header,
                        )?;
                        union_from_table(ctx.conn, &ctx.dest_table, &tmp, &extras, !first)?;
                        ctx.conn
                            .execute(&format!("DROP TABLE IF EXISTS {tmp}"), [])?;
                    }
                }
                "json" => {
                    let bytes = std::fs::read(&file_path)?;
                    let content = crate::encoding::decode_bytes(&bytes, encoding.as_deref())?;
                    let value: serde_json::Value = serde_json::from_str(&content)?;
                    let extracted = extract_json_path(&value, &json_path)?;
                    let table_val = if json_path.trim().is_empty() {
                        default_table_value(extracted)
                    } else {
                        extracted
                    };
                    let (mut headers, mut rows) = value_to_rows(table_val)?;
                    for (k, v) in &extras {
                        attach_const_column(&mut headers, &mut rows, k, Cell::Text(v.clone()));
                    }
                    ingest_rows(
                        ctx.conn,
                        &ctx.dest_table,
                        &headers,
                        rows,
                        IngestOpts {
                            force_str: false,
                            append: !first,
                        },
                    )?;
                }
                _ => {
                    bail!("不支持的文件类型: {}", Path::new(&file_path).display());
                }
            }
            first = false;
        }

        println!("🚀 目录加载完成: 共检测并合并了 {} 个文件。", files.len());
        Ok(FuncOutput::Table)
    }
}
