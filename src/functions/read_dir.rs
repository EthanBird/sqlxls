use crate::engine::Extension;
use super::read_excel::load_single_excel;
use anyhow::{Context, Result};
use glob::glob;
use regex::Regex;
use rusqlite::Connection;
use crate::engine::ExtResult;
pub struct ReadDirExt;

impl Extension for ReadDirExt {
    fn pattern(&self) -> Regex {
        Regex::new(r#"(?i)readdir\s*\(\s*['"]([^'"]+)['"]\s*,\s*['"]([^'"]+)['"](?:,\s*['"]([^'"]+)['"])?\s*\)"#).unwrap()
    }

    fn execute(&self, conn: &mut Connection, captures: &regex::Captures, table_name: &str) -> Result<ExtResult> {
        let pattern = captures.get(1).unwrap().as_str();
        let sheet = captures.get(2).unwrap().as_str();
        let opt = captures.get(3).map(|m| m.as_str());
        let force_str = opt == Some("str");

        let mut file_count = 0;
        for entry in glob(pattern).with_context(|| "通配符路径无效")? {
            if let Ok(path) = entry {
                let file_path = path.to_string_lossy().to_string();
                load_single_excel(conn, &file_path, Some(sheet), 0, table_name, force_str)?;
                file_count += 1;
            }
        }
        println!("🚀 目录加载完成: 共检测并合并了 {} 个 Excel 文件。", file_count);
        Ok(ExtResult::Table)
    }
}