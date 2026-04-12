use crate::engine::Extension;
use crate::functions::read_excel::load_single_excel;
use anyhow::{Context, Result};
use calamine::{open_workbook_auto, Reader};
use clipboard_rs::{Clipboard, ClipboardContext};
use regex::Regex;
use rusqlite::Connection;
use std::path::Path;
use crate::engine::ExtResult;
pub struct ReadClipboardExt;

impl Extension for ReadClipboardExt {
    fn pattern(&self) -> Regex {
        Regex::new(r#"(?i)readclipboard\s*\(\s*(?:['"]([^'"]+)['"])?\s*\)"#).unwrap()
    }

    fn execute(&self, conn: &mut Connection, captures: &regex::Captures, table_name: &str) -> Result<ExtResult> {
        let opt = captures.get(1).map(|m| m.as_str());
        let force_str = opt == Some("str");

        let ctx = ClipboardContext::new().map_err(|e| anyhow::anyhow!("无法初始化剪贴板: {}", e))?;
        let mut text_data = String::new();
        let files_result: Result<Vec<String>, _> = ctx.get_files();
        
        if let Ok(files) = files_result {
            if let Some(file_path) = files.first() {
                let path = Path::new(file_path);
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

                if ext == "xlsx" || ext == "xls" || ext == "xlsm" {
                    println!("📁 剪贴板捕获到 Excel 文件: {}", file_path);
                    let workbook = open_workbook_auto(file_path).with_context(|| format!("无法打开剪贴板中的 Excel: {}", file_path))?;
                    let sheet_names = workbook.sheet_names().to_owned();
                    let first_sheet = sheet_names.first().context("Excel 文件中没有任何表格")?;
                    
                    // 注意：这里改为了执行完毕后返回 Ok(ExtResult::Table)
                    load_single_excel(conn, file_path, Some(first_sheet), 0, table_name, force_str)?;
                    return Ok(ExtResult::Table);
                } else if ext == "csv" || ext == "txt" {
                    println!("📁 剪贴板捕获到文本类文件: {}", file_path);
                    text_data = std::fs::read_to_string(file_path)?;
                }
            }
        }

        if text_data.is_empty() {
            text_data = ctx.get_text().map_err(|e| anyhow::anyhow!("剪贴板中既不是文件，也没有文本内容: {}", e))?;
        }
        if text_data.trim().is_empty() { anyhow::bail!("剪贴板数据为空"); }

        let delimiter = if text_data.contains('\t') { '\t' } else { ',' };
        let mut reader = csv::ReaderBuilder::new().delimiter(delimiter as u8).from_reader(text_data.as_bytes());
        let headers = reader.headers()?.clone();
        
        let mut create_sql = format!("CREATE TABLE {} (", table_name);
        for header in headers.iter() { create_sql.push_str(&format!("\"{}\", ", header.replace("\"", "\"\""))); }
        create_sql.truncate(create_sql.len() - 2); create_sql.push(')');
        conn.execute(&create_sql, [])?;

        let placeholders = vec!["?"; headers.len()].join(", ");
        let mut stmt = conn.prepare(&format!("INSERT INTO {} VALUES ({})", table_name, placeholders))?;

        for result in reader.records() {
            let record = result?;
            let mut params_box: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(headers.len());
            for v in record.iter() { params_box.push(Box::new(v.to_string())); }
            let params: Vec<&dyn rusqlite::ToSql> = params_box.iter().map(|b| b.as_ref()).collect();
            stmt.execute(&*params)?;
        }

        println!("📋 已将剪贴板内容成功加载为临时表。");
        Ok(ExtResult::Table)
    }
}