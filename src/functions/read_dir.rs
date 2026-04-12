use crate::engine::Extension;
use super::read_excel::load_single_excel;
use anyhow::{Context, Result};
use glob::glob;
use regex::Regex;
use rusqlite::Connection;

pub struct ReadDirExt;

impl Extension for ReadDirExt {
    fn pattern(&self) -> Regex {
        // 匹配 readdir('目录通配符', 'sheet名', 'opt') 
        // 例如: readdir('data/*.xlsx', 'Sheet1', 'str')
        Regex::new(r#"(?i)readdir\s*\(\s*['"]([^'"]+)['"]\s*,\s*['"]([^'"]+)['"](?:,\s*['"]([^'"]+)['"])?\s*\)"#).unwrap()
    }

    fn execute(&self, conn: &mut Connection, captures: &regex::Captures, table_name: &str) -> Result<()> {
        let pattern = captures.get(1).unwrap().as_str();
        let sheet = captures.get(2).unwrap().as_str();
        let opt = captures.get(3).map(|m| m.as_str());
        let force_str = opt == Some("str");

        let mut file_count = 0;
        for entry in glob(pattern).with_context(|| "通配符路径无效")? {
            match entry {
                Ok(path) => {
                    let file_path = path.to_string_lossy().to_string();
                    // 这里利用 load_single_excel 的 `CREATE TABLE IF NOT EXISTS`
                    // 只要结构相同，后续的文件会直接以 INSERT 方式追加到这张表里
                    // 批量目录读取时，暂且默认不跳行，且要求明确给出 sheet 名字 (或者传 None)
                    load_single_excel(conn, &file_path, Some(sheet), 0, table_name, force_str)?;
                    file_count += 1;
                }
                Err(_) => continue,
            }
        }
        
        println!("🚀 目录加载完成: 共检测并合并了 {} 个 Excel 文件。", file_count);
        Ok(())
    }
}