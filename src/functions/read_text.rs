use crate::args::Args;
use crate::functions::{ExecCtx, FuncOutput, TableFunction};
use anyhow::{Context, Result};
use std::fs;
use std::time::Duration;

pub struct ReadTextExt;

impl TableFunction for ReadTextExt {
    fn names(&self) -> &'static [&'static str] {
        &["read_text", "readtext"]
    }

    fn execute(&self, _ctx: &mut ExecCtx, args: &Args) -> Result<FuncOutput> {
        let path = args.require_str(0, &["path", "file", "url"], "文本路径或 URL")?;
        let content = if path.starts_with("http://") || path.starts_with("https://") {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent("sqlxls/0.2")
                .build()?;
            client
                .get(&path)
                .send()
                .with_context(|| format!("请求文本失败: {}", path))?
                .error_for_status()
                .with_context(|| format!("请求文本失败: {}", path))?
                .text()?
        } else {
            fs::read_to_string(&path).with_context(|| format!("无法读取本地文本文件: {}", path))?
        };
        Ok(FuncOutput::Scalar(content))
    }
}
