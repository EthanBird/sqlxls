use crate::engine::{Extension, ExtResult};
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
        // 强大的正则：忽略括号内被单双引号包裹的右括号，支持抓取 readapi(...) 里的所有参数内容
        Regex::new(r#"(?i)readapi\s*\(((?:'[^']*'|"[^"]*"|[^)])+)\)"#).unwrap()
    }

    fn execute(&self, conn: &mut Connection, captures: &regex::Captures, table_name: &str) -> Result<ExtResult> {
        let args_str = captures.get(1).unwrap().as_str();
        
        // 智能参数分割器：按照逗号分割，但会完美忽略单引号、双引号、以及 JSON {} [] 内部的逗号
        let mut args = Vec::new();
        let mut current = String::new();
        let mut in_single = false;
        let mut in_double = false;
        let mut in_brace = 0;

        for c in args_str.chars() {
            match c {
                '\'' if !in_double => in_single = !in_single,
                '"' if !in_single => in_double = !in_double,
                '{' | '[' if !in_single && !in_double => in_brace += 1,
                '}' | ']' if !in_single && !in_double => in_brace -= 1,
                ',' if !in_single && !in_double && in_brace == 0 => {
                    let mut val = current.trim();
                    if (val.starts_with('\'') && val.ends_with('\'')) || (val.starts_with('"') && val.ends_with('"')) {
                        if val.len() >= 2 { val = &val[1..val.len()-1]; } // 剥离最外层引号
                    }
                    args.push(val.to_string());
                    current.clear();
                    continue;
                }
                _ => {}
            }
            current.push(c);
        }
        // 处理最后一个参数
        let mut val = current.trim();
        if (val.starts_with('\'') && val.ends_with('\'')) || (val.starts_with('"') && val.ends_with('"')) {
            if val.len() >= 2 { val = &val[1..val.len()-1]; }
        }
        args.push(val.to_string());

        // 提取 4 个核心参数 (带默认值降级逻辑)
        let url = args.get(0).context("read_api 至少需要一个 URL 参数")?.clone();
        let method_str = args.get(1).unwrap_or(&"GET".to_string()).to_uppercase();
        let payload_str = args.get(2).unwrap_or(&"".to_string()).clone();
        let headers_str = args.get(3).unwrap_or(&"".to_string()).clone();

        println!("🌐 正在请求 API: [{}] {}", method_str, url);

        let client = reqwest::blocking::Client::new();
        let method = reqwest::Method::from_bytes(method_str.as_bytes()).unwrap_or(reqwest::Method::GET);
        let mut req = client.request(method, &url);

        // 1. 处理 Headers (解析 JSON)
        if !headers_str.is_empty() {
            if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(&headers_str) {
                for (k, v) in map {
                    let v_str = if let Some(s) = v.as_str() { s.to_string() } else { v.to_string() };
                    req = req.header(k, v_str);
                }
            } else {
                println!("⚠️ 警告: Headers 参数不是合法的 JSON，已被忽略。");
            }
        }

        // 2. 处理 Payload (智能文件探测)
        if !payload_str.is_empty() && payload_str.to_lowercase() != "null" {
            let path = std::path::Path::new(&payload_str);
            if path.is_file() {
                // 如果发现这是一个本地文件，直接以二进制 Body 上传它！
                println!("📤 正在读取并上传文件: {}", payload_str);
                let file = std::fs::File::open(path)?;
                req = req.body(file);
            } else {
                // 作为纯文本或 JSON 发送
                req = req.body(payload_str.clone());
                // 如果是标准 JSON 且没有指定 Content-Type，我们智能附加上
                if !headers_str.to_lowercase().contains("content-type") && serde_json::from_str::<serde_json::Value>(&payload_str).is_ok() {
                    req = req.header("Content-Type", "application/json");
                }
            }
        }

        // 3. 发起请求与智能响应解析
        let response = req.send().with_context(|| format!("请求 API 失败: {}", url))?;
            
        let status = response.status();
        if !status.is_success() {
            let err_text = response.text().unwrap_or_default();
            anyhow::bail!("API 请求返回了错误状态码: {}\n{}", status, err_text);
        }

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
            println!("🌐 成功加载为临时表: {}", table_name);
        } 
        // 策略 2: 处理 CSV
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
            println!("🌐 成功加载为临时表: {}", table_name);
        }
        // 策略 3: 处理 Excel
        else {
            let mut temp_path = env::temp_dir();
            temp_path.push(format!("sqlxls_temp_api_{}.xlsx", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis()));
            
            let mut temp_file = File::create(&temp_path)?;
            temp_file.write_all(&bytes)?;
            
            load_single_excel(conn, temp_path.to_str().unwrap(), None, 0, table_name, false)?;
            let _ = std::fs::remove_file(temp_path);
            println!("🌐 成功加载为临时表: {}", table_name);
        }

        Ok(ExtResult::Table)
    }
}