use crate::args::Args;
use crate::functions::read_api::ReadApiExt;
use crate::functions::read_clipboard::ReadClipboardExt;
use crate::functions::read_csv::ReadCsvExt;
use crate::functions::read_dir::ReadDirExt;
use crate::functions::read_excel::ReadExcelExt;
use crate::functions::read_json::ReadJsonExt;
use crate::functions::read_text::ReadTextExt;
use crate::functions::{ExecCtx, FuncOutput, TableFunction};
use crate::syntax::infer_format;
use anyhow::bail;

/// `read()`：规范数据源构造器。按 `format=` 或 locator 分发。
pub struct ReadAutoExt;

impl TableFunction for ReadAutoExt {
    fn names(&self) -> &'static [&'static str] {
        &["read"]
    }

    fn execute(&self, ctx: &mut ExecCtx, args: &Args) -> Result<FuncOutput, anyhow::Error> {
        let src = args.require_str(0, &["path", "file", "url", "locator"], "路径或 URL")?;
        let fmt = args
            .get_str(99, &["format", "fmt"])
            .map(|s| s.to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .or_else(|| infer_format(&src))
            .unwrap_or_default();
        if fmt.is_empty() {
            anyhow::bail!(
                "无法从 `{}` 推断 format，请显式写 format='excel'|'csv'|'json'|'http'",
                src
            );
        }

        let mut dispatched = args.clone();
        rewrite_locator(&mut dispatched, &src, &fmt);

        match fmt.as_str() {
            "excel" | "xlsx" | "xls" | "xlsm" => ReadExcelExt.execute(ctx, &dispatched),
            "csv" | "tsv" => ReadCsvExt.execute(ctx, &dispatched),
            "json" => ReadJsonExt.execute(ctx, &dispatched),
            "http" | "https" | "api" => ReadApiExt.execute(ctx, &dispatched),
            "text" | "txt" => ReadTextExt.execute(ctx, &dispatched),
            "clipboard" | "clip" => ReadClipboardExt.execute(ctx, &dispatched),
            "glob" | "dir" => ReadDirExt.execute(ctx, &dispatched),
            other => bail!(
                "未知 format='{}'。syntax=1 支持: excel, csv, json, http, glob, clipboard, text",
                other
            ),
        }
    }
}

fn rewrite_locator(args: &mut Args, src: &str, fmt: &str) {
    let stripped = src
        .strip_prefix("glob:")
        .or_else(|| src.strip_prefix("clip:"))
        .or_else(|| src.strip_prefix("clipboard:"))
        .unwrap_or(src);
    if let Some(v) = args.positional.get_mut(0) {
        *v = crate::args::Value::Str(stripped.to_string());
    }
    if matches!(fmt, "clipboard" | "clip") && stripped.is_empty() {
        args.positional.clear();
        args.named.remove("path");
        args.named.remove("file");
        args.named.remove("url");
        args.named.remove("locator");
    }
}
