use anyhow::Result;
use clap::Parser;
use sqlxls::session::{load_sql_input, Session};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "sqlxls",
    version = "0.2.0",
    about = "用 SQL 清理和分析 Excel / CSV / JSON / API 等表格数据"
)]
#[command(group(clap::ArgGroup::new("run_mode").required(true).args(["input", "func"])))]
struct Cli {
    /// 执行的 SQL 语句或 SQL 文件
    input: Option<String>,

    /// 直接执行指定的内置函数 (例如: -f "read_api('http://xx')")
    #[arg(short, long = "func")]
    func: Option<String>,

    /// 输出文件路径 (xlsx/csv/json/ndjson；文本函数可写任意扩展名)
    #[arg(short, long = "output")]
    output: Option<PathBuf>,

    /// 打印改写后的 SQL / 函数调用后再执行
    #[arg(long = "explain")]
    explain: bool,
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let mut session = Session::new()?;

    if let Some(func_call) = args.func {
        session.run_func(&func_call, args.output.as_ref(), args.explain)?;
        return Ok(());
    }

    if let Some(input) = args.input {
        let sql = load_sql_input(&input)?;
        session.run_sql(&sql, args.output.as_ref(), args.explain)?;
    }

    Ok(())
}
