use crate::engine::Extension;
use anyhow::Result;
use fake::{Fake, faker};
use regex::Regex;
use rusqlite::Connection;

pub struct MockDataExt;

impl Extension for MockDataExt {
    fn pattern(&self) -> Regex {
        // 匹配 mock_data(数量, '列1:类型', '列2:类型'...)
        Regex::new(r#"(?i)mock_data\s*\(\s*(\d+)\s*(?P<cols>.*)\)"#).unwrap()
    }

    fn execute(&self, conn: &mut Connection, captures: &regex::Captures, table_name: &str) -> Result<()> {
        let count: usize = captures[1].parse()?;
        let cols_raw = &captures["cols"];
        
        // 提取所有 'name:type' 格式的参数
        let re_col = Regex::new(r#"['"]([^'"]+):([^'"]+)['"]"#).unwrap();
        let mut col_defs = Vec::new();
        for cap in re_col.captures_iter(cols_raw) {
            col_defs.push((cap[1].to_string(), cap[2].to_string()));
        }

        // 创建表
        let mut create_sql = format!("CREATE TABLE {} (", table_name);
        for (name, _) in &col_defs {
            create_sql.push_str(&format!("\"{}\", ", name));
        }
        create_sql.truncate(create_sql.len() - 2);
        create_sql.push(')');
        conn.execute(&create_sql, [])?;

        // 生成并插入数据
        let placeholders = vec!["?"; col_defs.len()].join(", ");
        let mut stmt = conn.prepare(&format!("INSERT INTO {} VALUES ({})", table_name, placeholders))?;

        for _ in 0..count {
            let mut row_data = Vec::new();
            for (_, type_str) in &col_defs {
                let val: String = match type_str.to_lowercase().as_str() {
                    "name" => faker::name::en::Name().fake(),
                    "phone" => faker::phone_number::en::PhoneNumber().fake(),
                    "email" => faker::internet::en::SafeEmail().fake(),
                    "company" => faker::company::en::CompanyName().fake(),
                    "city" => faker::address::en::CityName().fake(),
                    _ => "N/A".to_string(),
                };
                row_data.push(val);
            }
            let params: Vec<&dyn rusqlite::ToSql> = row_data.iter()
                .map(|v| v as &dyn rusqlite::ToSql)
                .collect();
            stmt.execute(&*params)?;
        }

        println!("🎲 已生成 {} 条模拟数据到表: {}", count, table_name);
        Ok(())
    }
}