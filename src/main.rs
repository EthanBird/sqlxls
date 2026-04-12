mod engine;
mod functions;

use anyhow::Result;
use clap::Parser;
use rusqlite::Connection;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "sqlxls", version = "3.0", about = "基于 SQL 的 Excel 数据处理框架")]
struct Cli {
    sql: String,
    #[arg(short, long = "output")]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let mut conn = Connection::open_in_memory()?;
    
    let mut final_sql = args.sql.clone();
    let mut table_counter = 0;

    let extensions = functions::register_all();

    // 修复 E0599 & E0282: 不再将 Captures 存入 Vec，而是使用 while 循环每次重新匹配
    // 这样既避开了生命周期借用冲突，又完美支持多次调用同一个内置函数！
    for ext in extensions {
        let regex = ext.pattern();
        while let Some(cap) = regex.captures(&final_sql) {
            let full_match = cap.get(0).unwrap().as_str().to_string();
            let table_name = format!("excel_tmp_{}", table_counter);
            table_counter += 1;

            // 调用插件，执行具体的加载逻辑
            ext.execute(&mut conn, &cap, &table_name)?;

            // 将 SQL 中的函数调用替换为内存表名 (替换后循环会匹配下一个)
            final_sql = final_sql.replace(&full_match, &table_name);
        }
    }

    // 统一交由引擎处理执行与输出
    engine::handle_output(&conn, &final_sql, args.output.as_ref())?;

    Ok(())
}