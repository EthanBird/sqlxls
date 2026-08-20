use anyhow::Result;
use clap::Parser;
use sqlxls::session::{load_sql_input, Session};
use sqlxls::syntax::SyntaxOpts;
use std::path::PathBuf;

fn parse_set_kv(s: &str) -> Result<(String, String), String> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| "期望 name=value，例如 --set region=east".to_string())?;
    let k = k.trim();
    if k.is_empty() {
        return Err("变量名不能为空".into());
    }
    Ok((k.to_string(), v.to_string()))
}

#[derive(Parser, Debug)]
#[command(
    name = "sqlxls",
    version = env!("CARGO_PKG_VERSION"),
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

    /// 除定位符外必须使用命名参数（syntax=1 的严格模式）
    #[arg(long = "strict")]
    strict: bool,

    /// 语言版本：1=兼容糖函数；2=查询层只允许 read()/LOAD
    #[arg(long = "syntax", default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=2))]
    syntax: u8,

    /// 绑定变量，可重复：--set region=east --set year=2024
    #[arg(long = "set", value_parser = parse_set_kv, action = clap::ArgAction::Append)]
    set: Vec<(String, String)>,
}

fn parse_set_value(raw: &str) -> sqlxls::args::Value {
    let t = raw.trim();
    if t.eq_ignore_ascii_case("true") {
        return sqlxls::args::Value::Bool(true);
    }
    if t.eq_ignore_ascii_case("false") {
        return sqlxls::args::Value::Bool(false);
    }
    if t.eq_ignore_ascii_case("null") {
        return sqlxls::args::Value::Null;
    }
    if let Ok(i) = t.parse::<i64>() {
        return sqlxls::args::Value::Int(i);
    }
    if let Ok(f) = t.parse::<f64>() {
        if t.contains('.') || t.contains('e') || t.contains('E') {
            return sqlxls::args::Value::Float(f);
        }
    }
    sqlxls::args::Value::Str(raw.to_string())
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let mut session = Session::with_opts(SyntaxOpts {
        version: args.syntax,
        strict: args.strict,
    })?;
    for (k, v) in args.set {
        session.set_var(k, parse_set_value(&v));
    }

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
