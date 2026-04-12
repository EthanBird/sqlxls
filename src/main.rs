mod engine;
mod functions;

use anyhow::{Context, Result};
use clap::Parser;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "sqlxls", version = "3.1", about = "基于 SQL 的 Excel 数据处理框架")]
struct Cli {
    /// 执行的 SQL 语句，或者是一个包含 SQL 代码的文件路径 (如 query.sql)
    input: String,

    #[arg(short, long = "output")]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let mut conn = Connection::open_in_memory()?;
    
    // 🌟 智能判断输入：如果输入的是一个存在的文件，读取它；否则作为纯 SQL 字符串
    let mut final_sql = if Path::new(&args.input).is_file() {
        std::fs::read_to_string(&args.input)
            .with_context(|| format!("无法读取 SQL 文件: {}", args.input))?
    } else {
        args.input.clone()
    };

    let mut table_counter = 0;
    let extensions = functions::register_all();

    for ext in extensions {
        let regex = ext.pattern();
        while let Some(cap) = regex.captures(&final_sql) {
            let full_match = cap.get(0).unwrap().as_str().to_string();
            let table_name = format!("excel_tmp_{}", table_counter);
            table_counter += 1;

            ext.execute(&mut conn, &cap, &table_name)?;
            final_sql = final_sql.replace(&full_match, &table_name);
        }
    }

    engine::handle_output(&conn, &final_sql, args.output.as_ref())?;

    Ok(())
}