use crate::engine::Extension;
use anyhow::{Context, Result};
use calamine::{open_workbook_auto, Data, Reader};
use regex::Regex;
use rusqlite::{Connection, ToSql};
use std::collections::HashSet;

pub struct ReadExcelExt;

impl Extension for ReadExcelExt {
    fn pattern(&self) -> Regex {
        // 匹配 readexcel('path', 'sheet', 'opt')
        Regex::new(r#"(?i)readexcel\s*\(\s*['"]([^'"]+)['"]\s*,\s*['"]([^'"]+)['"](?:,\s*['"]([^'"]+)['"])?\s*\)"#).unwrap()
    }

    fn execute(&self, conn: &mut Connection, captures: &regex::Captures, table_name: &str) -> Result<()> {
        let path = captures.get(1).unwrap().as_str();
        let sheet = captures.get(2).unwrap().as_str();
        let opt = captures.get(3).map(|m| m.as_str());
        let force_str = opt == Some("str");

        load_single_excel(conn, path, sheet, table_name, force_str)
    }
}

// 抽取出来的核心加载方法，readdir 里也能复用
pub fn load_single_excel(conn: &mut Connection, path: &str, sheet: &str, table_name: &str, force_str: bool) -> Result<()> {
    let mut workbook = open_workbook_auto(path).with_context(|| format!("无法打开 Excel: {}", path))?;
    let range = workbook.worksheet_range(sheet).with_context(|| format!("未找到 Sheet: {}", sheet))?;
    let mut rows = range.rows();
    
    let headers = match rows.next() {
        Some(row) => row,
        None => return Ok(()),
    };

    let mut create_table_sql = format!("CREATE TABLE IF NOT EXISTS {} (", table_name);
    let mut seen_cols = HashSet::new();

    for (i, header) in headers.iter().enumerate() {
        let col_name = match header {
            Data::String(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => format!("col_{}", i),
        };
        let mut safe_name = col_name.clone();
        let mut count = 1;
        while seen_cols.contains(&safe_name) {
            safe_name = format!("{}_{}", col_name, count);
            count += 1;
        }
        seen_cols.insert(safe_name.clone());
        create_table_sql.push_str(&format!("\"{}\", ", safe_name.replace("\"", "\"\"")));
    }
    
    create_table_sql.truncate(create_table_sql.len() - 2); 
    create_table_sql.push(')');
    conn.execute(&create_table_sql, [])?;

    let placeholders = vec!["?"; headers.len()].join(", ");
    let insert_sql = format!("INSERT INTO {} VALUES ({})", table_name, placeholders);
    let mut stmt = conn.prepare(&insert_sql)?;

    for row in rows {
        let mut params_vec: Vec<Box<dyn ToSql>> = Vec::with_capacity(headers.len());
        for cell in row.iter() {
            if force_str {
                let s = match cell {
                    Data::Empty | Data::Error(_) => None,
                    Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => Some(s.clone()),
                    Data::Int(i) => Some(i.to_string()),
                    Data::Float(f) => Some(f.to_string()),
                    Data::Bool(b) => Some(b.to_string()),
                    Data::DateTime(d) => Some(d.as_f64().to_string()),
                };
                if let Some(val) = s { params_vec.push(Box::new(val)); } else { params_vec.push(Box::new(rusqlite::types::Null)); }
            } else {
                match cell {
                    Data::Empty | Data::Error(_) => params_vec.push(Box::new(rusqlite::types::Null)),
                    Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => params_vec.push(Box::new(s.clone())),
                    Data::Int(i) => params_vec.push(Box::new(*i)),
                    Data::Float(f) => params_vec.push(Box::new(*f)),
                    Data::Bool(b) => params_vec.push(Box::new(*b)),
                    Data::DateTime(d) => params_vec.push(Box::new(d.as_f64())),
                }
            }
        }
        let params: Vec<&dyn ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
        stmt.execute(&*params)?;
    }
    Ok(())
}