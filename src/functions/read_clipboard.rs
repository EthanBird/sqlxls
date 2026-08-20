use crate::args::Args;
use crate::functions::read_csv::{load_csv_text, parse_delim, sniff_delim};
use crate::functions::read_excel::excel_to_frame;
use crate::functions::{ExecCtx, FuncOutput, TableFunction};
use crate::ingest::{ingest_rows, IngestOpts};
use crate::schema::HeaderSpec;
use anyhow::{bail, Result};
use clipboard_rs::{Clipboard, ClipboardContext};
use std::path::Path;

pub struct ReadClipboardExt;

impl TableFunction for ReadClipboardExt {
    fn names(&self) -> &'static [&'static str] {
        &["read_clipboard", "readclipboard"]
    }

    fn execute(&self, ctx: &mut ExecCtx, args: &Args) -> Result<FuncOutput> {
        let force_str = args.get_bool(0, &["str", "force_str"]);
        let encoding = args.get_str(99, &["encoding", "charset"]);
        let delim = args
            .get_str(1, &["delim", "delimiter", "sep"])
            .map(|s| parse_delim(&s));
        let header = HeaderSpec::from_args(args)?;

        let ctx_clip =
            ClipboardContext::new().map_err(|e| anyhow::anyhow!("无法初始化剪贴板: {}", e))?;
        let mut text_data = String::new();

        if let Ok(files) = ctx_clip.get_files() {
            if let Some(file_path) = files.first() {
                let path = Path::new(file_path);
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if ext == "xlsx" || ext == "xls" || ext == "xlsm" {
                    println!("📁 剪贴板捕获到 Excel 文件: {}", file_path);
                    let (headers, rows) = excel_to_frame(file_path, None, 0, force_str, &header)?;
                    ingest_rows(
                        ctx.conn,
                        &ctx.dest_table,
                        &headers,
                        rows,
                        IngestOpts {
                            force_str,
                            append: false,
                        },
                    )?;
                    return Ok(FuncOutput::Table);
                } else if ext == "csv" || ext == "txt" || ext == "tsv" {
                    println!("📁 剪贴板捕获到文本类文件: {}", file_path);
                    let bytes = std::fs::read(file_path)?;
                    text_data = crate::encoding::decode_bytes(&bytes, encoding.as_deref())?;
                }
            }
        }

        if text_data.is_empty() {
            text_data = ctx_clip
                .get_text()
                .map_err(|e| anyhow::anyhow!("剪贴板中既不是文件，也没有文本内容: {}", e))?;
        }
        if text_data.trim().is_empty() {
            bail!("剪贴板数据为空");
        }

        let delim = delim.or_else(|| Some(sniff_delim(&text_data)));
        load_csv_text(
            ctx.conn,
            &ctx.dest_table,
            &text_data,
            delim,
            0,
            force_str,
            false,
            &header,
        )?;
        println!("📋 已将剪贴板内容成功加载为临时表。");
        Ok(FuncOutput::Table)
    }
}
