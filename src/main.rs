mod engine;
mod functions;

use anyhow::{Context, Result};
use clap::Parser;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "sqlxls", version = "4.0", about = "基于 SQL 的多维数据处理与编排框架")]
#[command(group(clap::ArgGroup::new("run_mode").required(true).args(["input", "func"])))]
struct Cli {
    /// 执行的 SQL 语句或 SQL 文件
    input: Option<String>,

    /// 直接执行指定的内置函数 (例如: -f "read_api('http://xx')")
    #[arg(short, long = "func")]
    func: Option<String>,

    /// 输出文件路径 (表格支持 xlsx/csv/json，文本支持任意扩展名)
    #[arg(short, long = "output")]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let mut conn = Connection::open_in_memory()?;
    let extensions = functions::register_all();

    // ==========================================
    // 模式 A：直接执行单个函数 (-f 模式)
    // ==========================================
    if let Some(func_call) = args.func {
        let mut matched = false;
        for ext in extensions {
            let regex = ext.pattern();
            if let Some(cap) = regex.captures(&func_call) {
                matched = true;
                let table_name = "func_direct_output";
                
                // 执行函数并根据返回类型决定如何渲染
                match ext.execute(&mut conn, &cap, table_name)? {
                    engine::ExtResult::Table => {
                        let sql = format!("SELECT * FROM {}", table_name);
                        engine::handle_output(&conn, &sql, args.output.as_ref())?;
                    }
                    engine::ExtResult::Text(txt) => {
                        engine::handle_text_output(&txt, args.output.as_ref())?;
                    }
                }
                break;
            }
        }
        if !matched { anyhow::bail!("未找到匹配的内置函数或参数格式错误: {}", func_call); }
        return Ok(());
    }

    // ==========================================
    // 模式 B：标准 SQL 执行与宏解析模式
    // ==========================================
    if let Some(input) = args.input {
        let mut final_sql = if Path::new(&input).is_file() {
            std::fs::read_to_string(&input).with_context(|| format!("无法读取 SQL 文件: {}", input))?
        } else {
            input
        };

        let mut table_counter = 0;
        for ext in extensions {
            let regex = ext.pattern();
            while let Some(cap) = regex.captures(&final_sql) {
                let full_match = cap.get(0).unwrap().as_str().to_string();
                let table_name = format!("excel_tmp_{}", table_counter);
                table_counter += 1;

                match ext.execute(&mut conn, &cap, &table_name)? {
                    engine::ExtResult::Table => {
                        // 如果是表格，注册到 SQLite 后，用表名替换原函数
                        final_sql = final_sql.replace(&full_match, &table_name);
                    }
                    engine::ExtResult::Text(txt) => {
                        // 🌟 魔法：如果是文本结果，直接替换进 SQL 语句中充当宏参数！
                        final_sql = final_sql.replace(&full_match, &txt);
                    }
                }
            }
        }
        engine::handle_output(&conn, &final_sql, args.output.as_ref())?;
    }

    Ok(())
}