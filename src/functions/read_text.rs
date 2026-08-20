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
        let encoding = args.get_str(99, &["encoding", "charset"]);
        let content = if path.starts_with("http://") || path.starts_with("https://") {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent("sqlxls/0.2")
                .build()?;
            let resp = client
                .get(&path)
                .send()
                .with_context(|| format!("请求文本失败: {}", path))?
                .error_for_status()
                .with_context(|| format!("请求文本失败: {}", path))?;
            let ct = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let bytes = resp.bytes()?;
            crate::encoding::decode_http(&bytes, encoding.as_deref(), &ct)?
        } else {
            let bytes =
                fs::read(&path).with_context(|| format!("无法读取本地文本文件: {}", path))?;
            crate::encoding::decode_bytes(&bytes, encoding.as_deref())?
        };
        Ok(FuncOutput::Scalar(content))
    }
}
