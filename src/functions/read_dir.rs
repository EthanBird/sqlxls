use crate::args::Args;
use crate::functions::read_csv::load_csv_path;
use crate::functions::read_excel::excel_to_frame;
use crate::functions::read_json::json_to_table;
use crate::functions::{ExecCtx, FuncOutput, TableFunction};
use crate::ingest::{ingest_rows, IngestOpts};
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
        let files: Vec<std::path::PathBuf> = glob(&pattern)
            .with_context(|| "通配符路径无效")?
            .filter_map(|e| e.ok())
            .collect();
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

        let mut first = true;
        for path in &files {
            let file_path = path.to_string_lossy().to_string();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            match ext.as_str() {
                "xlsx" | "xls" | "xlsm" => {
                    let (headers, rows) =
                        excel_to_frame(&file_path, sheet.as_deref(), skip, force_str)?;
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
                    load_csv_path(
                        ctx.conn,
                        &ctx.dest_table,
                        &file_path,
                        delim,
                        skip,
                        force_str,
                        !first,
                    )?;
                }
                "json" => {
                    if !first {
                        bail!("read_dir 合并 JSON 时请使用结构相同的对象数组；当前实现仅支持单文件 JSON 或改用 UNION。文件: {}", file_path);
                    }
                    let content = std::fs::read_to_string(&file_path)?;
                    let value: serde_json::Value = serde_json::from_str(&content)?;
                    json_to_table(ctx.conn, &ctx.dest_table, &value, "")?;
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
