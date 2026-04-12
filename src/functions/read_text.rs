use crate::engine::{Extension, ExtResult};
use anyhow::{Context, Result};
use regex::Regex;
use rusqlite::Connection;
use std::fs;

pub struct ReadTextExt;

impl Extension for ReadTextExt {
    fn pattern(&self) -> Regex {
        // 匹配 readtext('文件路径')
        Regex::new(r#"(?i)readtext\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap()
    }

    fn execute(&self, _conn: &mut Connection, captures: &regex::Captures, _table_name: &str) -> Result<ExtResult> {
        let path = captures.get(1).unwrap().as_str();
        
        // 我们甚至可以加入简单的网络识别
        let content = if path.starts_with("http") {
            reqwest::blocking::get(path)
                .with_context(|| format!("请求文本 API 失败: {}", path))?
                .text()?
        } else {
            fs::read_to_string(path)
                .with_context(|| format!("无法读取本地文本文件: {}", path))?
        };

        // 🌟 重点：返回纯文本特征
        Ok(ExtResult::Text(content))
    }
}