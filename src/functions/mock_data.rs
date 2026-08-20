use crate::args::Args;
use crate::functions::{ExecCtx, FuncOutput, TableFunction};
use crate::ingest::{ingest_rows, Cell, IngestOpts};
use anyhow::{bail, Result};
use fake::{faker, Fake};

pub struct MockDataExt;

impl TableFunction for MockDataExt {
    fn names(&self) -> &'static [&'static str] {
        &["mock_data", "mockdata"]
    }

    fn execute(&self, ctx: &mut ExecCtx, args: &Args) -> Result<FuncOutput> {
        let count = args.get_usize(0, &["n", "count", "rows"], 0);
        if count == 0 {
            bail!("mock_data 需要生成行数，例如 mock_data(10, '用户名:name')");
        }

        let mut col_defs: Vec<(String, String)> = Vec::new();
        for (i, v) in args.positional.iter().enumerate() {
            if i == 0 {
                continue;
            }
            if let Some(s) = v.as_str() {
                push_col_def(&mut col_defs, s);
            }
        }
        for (k, v) in &args.named {
            if matches!(k.as_str(), "n" | "count" | "rows") {
                continue;
            }
            if let Some(s) = v.as_str() {
                col_defs.push((k.clone(), s.to_string()));
            }
        }

        if col_defs.is_empty() {
            bail!("mock_data 需要至少一列，格式: '列名:类型'");
        }

        let headers: Vec<String> = col_defs.iter().map(|(n, _)| n.clone()).collect();
        let mut rows = Vec::with_capacity(count);
        for _ in 0..count {
            let mut row = Vec::with_capacity(col_defs.len());
            for (_, type_str) in &col_defs {
                let val: String = match type_str.to_lowercase().as_str() {
                    "name" => faker::name::en::Name().fake(),
                    "phone" => faker::phone_number::en::PhoneNumber().fake(),
                    "email" => faker::internet::en::SafeEmail().fake(),
                    "company" => faker::company::en::CompanyName().fake(),
                    "city" => faker::address::en::CityName().fake(),
                    _ => "N/A".to_string(),
                };
                row.push(Cell::Text(val));
            }
            rows.push(row);
        }

        ingest_rows(
            ctx.conn,
            &ctx.dest_table,
            &headers,
            rows,
            IngestOpts {
                force_str: true,
                append: false,
            },
        )?;
        println!("🎲 已生成 {} 条模拟数据到表: {}", count, ctx.dest_table);
        Ok(FuncOutput::Table)
    }
}

fn push_col_def(cols: &mut Vec<(String, String)>, spec: &str) {
    if let Some((name, ty)) = spec.split_once(':') {
        let name = name.trim();
        let ty = ty.trim();
        if !name.is_empty() && !ty.is_empty() {
            cols.push((name.to_string(), ty.to_string()));
        }
    }
}
