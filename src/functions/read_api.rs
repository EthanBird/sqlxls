use crate::engine::Extension;
use crate::functions::read_excel::load_single_excel;
use crate::functions::read_json::json_to_sqlite;
use anyhow::{Context, Result};
use regex::Regex;
use rusqlite::Connection;
use std::env;
use std::fs::File;
use std::io::Write;

pub struct ReadApiExt;

impl Extension for ReadApiExt {
    fn pattern(&self) -> Regex {
        // 匹配 readapi('url') 
        Regex::new(r#"(?i)readapi\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap()
    }

    fn execute(&self, conn: &mut Connection, captures: &regex::Captures, table_name: &str) -> Result<()> {
        let url = captures.get(1).unwrap().as_str();
        println!("🌐 正在请求 API: {} ...", url);

        // 发起同步 HTTP 请求
        let response = reqwest::blocking::get(url)
            .with_context(|| format!("请求 API 失败: {}", url))?;
            
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("API 请求返回了错误状态码: {}", status);
        }

        // 提取 Content-Type，并转换为小写
        let content_type = response.headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        let bytes = response.bytes()?;

        // 策略 1: 处理 JSON
        if content_type.contains("application/json") || url.ends_with(".json") {
            let json_val: serde_json::Value = serde_json::from_slice(&bytes)
                .context("API 返回的数据不是合法的 JSON")?;
            json_to_sqlite(conn, &json_val, table_name)?;
            println!("🌐 智能识别为 JSON 格式并加载至表: {}", table_name);
        } 
        // 策略 2: 处理 CSV (包含文本类型且以逗号分隔)
        else if content_type.contains("text/csv") || url.ends_with(".csv") {
            let mut reader = csv::ReaderBuilder::new().from_reader(bytes.as_ref());
            let headers = reader.headers()?.clone();
            
            let mut create_sql = format!("CREATE TABLE {} (", table_name);
            for h in headers.iter() { create_sql.push_str(&format!("\"{}\", ", h.replace("\"", "\"\""))); }
            create_sql.truncate(create_sql.len() - 2);
            create_sql.push(')');
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
            println!("🌐 智能识别为 CSV 格式并加载至表: {}", table_name);
        }
        // 策略 3: 处理 Excel 二进制流 (xlsx, xls 等)
        else {
            // 如果既不是 JSON 也不是明确的 CSV，我们将其存入临时文件并当做 Excel 处理
            // 这种设计完美复用了 calamine 的解析能力
            let mut temp_path = env::temp_dir();
            temp_path.push(format!("sqlxls_temp_api_{}.xlsx", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis()));
            
            let mut temp_file = File::create(&temp_path)?;
            temp_file.write_all(&bytes)?;
            
            // 复用 read_excel 的逻辑，默认不跳行、读取第一个 Sheet
            load_single_excel(conn, temp_path.to_str().unwrap(), None, 0, table_name, false)?;
            
            // 清理临时文件
            let _ = std::fs::remove_file(temp_path);
            println!("🌐 智能识别为 Excel 格式并加载至表: {}", table_name);
        }

        Ok(())
    }
}