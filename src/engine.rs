use anyhow::{Context, Result};
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Table};
use rusqlite::{types::ValueRef, Connection};
use rust_xlsxwriter::Workbook;
use std::fs::File;
use std::io::Write; // 新增写入特征
use std::path::PathBuf;

/// 新增：扩展函数的执行结果形态
pub enum ExtResult {
    Table,         // 成功将数据作为表格挂载到了 SQLite 临时表中
    Text(String),  // 直接返回了纯文本内容 (用于宏替换或直接输出)
}

pub trait Extension {
    fn pattern(&self) -> regex::Regex;
    // 修改：返回值从 Result<()> 变为 Result<ExtResult>
    fn execute(&self, conn: &mut Connection, captures: &regex::Captures, table_name: &str) -> Result<ExtResult>;
}

// ==========================================
// 新增：专门用于纯文本结果输出的引擎
// ==========================================
pub fn handle_text_output(text: &str, output_path: Option<&PathBuf>) -> Result<()> {
    if let Some(path) = output_path {
        let mut file = File::create(path)?;
        file.write_all(text.as_bytes())?;
        println!("✅ 纯文本结果已保存至: {:?}", path);
    } else {
        println!("{}", text); // 直接在控制台打印文本
    }
    Ok(())
}

/// 执行 SQL 并路由输出格式
pub fn handle_output(conn: &Connection, sql: &str, output_path: Option<&PathBuf>) -> Result<()> {
    let mut stmt = conn.prepare(sql).with_context(|| format!("SQL 执行失败:\n{}", sql))?;
    let col_names: Vec<String> = stmt.column_names().into_iter().map(|s| s.to_string()).collect();
    let column_count = col_names.len();
    let mut rows = stmt.query([])?;

    if output_path.is_none() {
        let mut table = Table::new();
        table.load_preset(UTF8_FULL).apply_modifier(UTF8_ROUND_CORNERS);
        table.set_header(&col_names);
        let mut row_count = 0;
        while let Some(row) = rows.next()? {
            let mut str_row = Vec::with_capacity(column_count);
            for col_idx in 0..column_count {
                str_row.push(val_to_string(row.get_ref(col_idx)?));
            }
            table.add_row(str_row);
            row_count += 1;
        }
        println!("{}", table);
        println!("📊 共计返回 {} 行数据。", row_count);
        return Ok(());
    }

    let path = output_path.unwrap();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("xlsx").to_lowercase();

    match ext.as_str() {
        "csv" => {
            let mut wtr = csv::Writer::from_path(path)?;
            wtr.write_record(&col_names)?;
            while let Some(row) = rows.next()? {
                let mut str_row = Vec::with_capacity(column_count);
                for col_idx in 0..column_count { str_row.push(val_to_string(row.get_ref(col_idx)?)); }
                wtr.write_record(&str_row)?;
            }
            wtr.flush()?;
        }
        "json" => {
            let file = File::create(path)?;
            let mut json_array = Vec::new();
            while let Some(row) = rows.next()? {
                let mut obj = serde_json::Map::new();
                for col_idx in 0..column_count {
                    let val = match row.get_ref(col_idx)? {
                        ValueRef::Null => serde_json::Value::Null,
                        ValueRef::Integer(i) => serde_json::json!(i),
                        ValueRef::Real(f) => serde_json::json!(f),
                        ValueRef::Text(t) => serde_json::json!(std::str::from_utf8(t).unwrap_or("")),
                        ValueRef::Blob(_) => serde_json::json!("<Binary>"),
                    };
                    obj.insert(col_names[col_idx].clone(), val);
                }
                json_array.push(serde_json::Value::Object(obj));
            }
            serde_json::to_writer_pretty(file, &json_array)?;
        }
        _ => {
            let mut workbook = Workbook::new();
            let worksheet = workbook.add_worksheet();
            for (i, name) in col_names.iter().enumerate() { worksheet.write_string(0, i as u16, name)?; }
            let mut row_idx: u32 = 1;
            while let Some(row) = rows.next()? {
                for col_idx in 0..column_count {
                    let col = col_idx as u16;
                    match row.get_ref(col_idx)? {
                        ValueRef::Null => {}
                        ValueRef::Integer(i) => { worksheet.write_number(row_idx, col, i as f64)?; }
                        ValueRef::Real(f) => { worksheet.write_number(row_idx, col, f)?; }
                        ValueRef::Text(t) => { worksheet.write_string(row_idx, col, std::str::from_utf8(t).unwrap_or(""))?; }
                        ValueRef::Blob(b) => { worksheet.write_string(row_idx, col, &format!("<Blob: {}b>", b.len()))?; }
                    }
                }
                row_idx += 1;
            }
            workbook.save(path).with_context(|| "保存 Excel 失败")?;
        }
    }
    println!("✅ 处理成功! 结果已保存至: {:?}", path);
    Ok(())
}

fn val_to_string(val_ref: ValueRef) -> String {
    match val_ref {
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(t) => std::str::from_utf8(t).unwrap_or("").to_string(),
        ValueRef::Blob(b) => format!("<Binary: {} bytes>", b.len()),
    }
}