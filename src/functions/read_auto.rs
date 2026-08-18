use crate::args::Args;
use crate::functions::read_api::ReadApiExt;
use crate::functions::read_csv::ReadCsvExt;
use crate::functions::read_excel::ReadExcelExt;
use crate::functions::read_json::ReadJsonExt;
use crate::functions::read_text::ReadTextExt;
use crate::functions::{ExecCtx, FuncOutput, TableFunction};
use anyhow::bail;

/// `read()`：按协议 / 扩展名分发到具体连接器。
pub struct ReadAutoExt;

impl TableFunction for ReadAutoExt {
    fn names(&self) -> &'static [&'static str] {
        &["read"]
    }

    fn execute(&self, ctx: &mut ExecCtx, args: &Args) -> Result<FuncOutput, anyhow::Error> {
        let src = args.require_str(0, &["path", "file", "url"], "路径或 URL")?;
        let lower = src.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return ReadApiExt.execute(ctx, args);
        }
        if let Some(ext) = std::path::Path::new(&src)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
        {
            return match ext.as_str() {
                "xlsx" | "xls" | "xlsm" => ReadExcelExt.execute(ctx, args),
                "csv" | "tsv" => ReadCsvExt.execute(ctx, args),
                "json" => ReadJsonExt.execute(ctx, args),
                "txt" => ReadTextExt.execute(ctx, args),
                other => bail!("read() 无法根据扩展名 `.{}` 判断格式，请改用 read_excel / read_csv / read_json", other),
            };
        }
        bail!("read() 需要带扩展名的路径，或 http(s) URL")
    }
}
