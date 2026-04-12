use crate::engine::Extension;
use anyhow::{Context, Result};
use regex::Regex;
use rusqlite::Connection;
use serde_json::Value;
use std::fs;

pub struct ReadJsonExt;

impl Extension for ReadJsonExt {
    fn pattern(&self) -> Regex {
        // 匹配 readjson('path')
        Regex::new(r#"(?i)readjson\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap()
    }

    fn execute(&self, conn: &mut Connection, captures: &regex::Captures, table_name: &str) -> Result<()> {
        let path = captures.get(1).unwrap().as_str();
        let content = fs::read_to_string(path).with_context(|| format!("无法读取 JSON 文件: {}", path))?;
        let value: Value = serde_json::from_str(&content).context("JSON 格式不合法")?;
        
        json_to_sqlite(conn, &value, table_name)?;
        println!("📄 已从 JSON 文件加载数据到表: {}", table_name);
        Ok(())
    }
}

// 抽取为公共函数，以便 API 模块也能复用这段 JSON 转换逻辑
pub fn json_to_sqlite(conn: &mut Connection, value: &Value, table_name: &str) -> Result<()> {
    // 智能寻址：如果根节点是对象，尝试找里面的第一个数组（很多 API 喜欢包一层 {"data": [...]}）
    let array = match value {
        Value::Array(arr) => arr,
        Value::Object(obj) => {
            obj.values().find_map(|v| v.as_array())
               .context("JSON 根节点必须是数组，或包含数组属性的对象")?
        }
        _ => anyhow::bail!("无法识别的 JSON 结构"),
    };

    let first_item = array.first().context("JSON 数组为空")?;
    let obj = first_item.as_object().context("JSON 数组元素必须是 Object")?;
    let headers: Vec<String> = obj.keys().cloned().collect();

    // 动态建表
    let mut create_sql = format!("CREATE TABLE {} (", table_name);
    for h in &headers {
        create_sql.push_str(&format!("\"{}\", ", h.replace("\"", "\"\"")));
    }
    create_sql.truncate(create_sql.len() - 2);
    create_sql.push(')');
    conn.execute(&create_sql, [])?;

    // 插入数据
    let placeholders = vec!["?"; headers.len()].join(", ");
    let mut stmt = conn.prepare(&format!("INSERT INTO {} VALUES ({})", table_name, placeholders))?;

    for item in array {
        if let Some(o) = item.as_object() {
            let mut params_box: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(headers.len());
            for h in &headers {
                match o.get(h).unwrap_or(&Value::Null) {
                    Value::Null => params_box.push(Box::new(rusqlite::types::Null)),
                    Value::Bool(b) => params_box.push(Box::new(*b)),
                    Value::Number(n) => {
                        if let Some(i) = n.as_i64() { params_box.push(Box::new(i)); }
                        else if let Some(f) = n.as_f64() { params_box.push(Box::new(f)); }
                        else { params_box.push(Box::new(n.to_string())); }
                    }
                    Value::String(s) => params_box.push(Box::new(s.clone())),
                    // 对于嵌套的数组或对象，直接转成 JSON 字符串存入
                    Value::Array(a) => params_box.push(Box::new(serde_json::to_string(a)?)),
                    Value::Object(nested) => params_box.push(Box::new(serde_json::to_string(nested)?)),
                }
            }
            let params: Vec<&dyn rusqlite::ToSql> = params_box.iter().map(|b| b.as_ref()).collect();
            stmt.execute(&*params)?;
        }
    }
    Ok(())
}