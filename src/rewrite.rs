use crate::args::{
    matching_paren, parse_arg_list, parse_call_head, parse_ident, skip_sql_noise, skip_ws,
    sql_quote, ArgExpr, ArgSlot, Args, Value,
};
use crate::classify::{self, SourcePlace};
use crate::functions::{ExecCtx, FuncOutput, Registry};
use crate::syntax::{
    desugar_call, is_scalar_source, resolve_format, validate_closed_options,
    validate_query_source_name, validate_source_args, SourceSpec, SyntaxOpts,
};
use anyhow::{bail, Result};
use rusqlite::Connection;

#[derive(Debug)]
struct TopCall {
    name: String,
    /// 整个 `name(...)` 的字节区间
    start: usize,
    end: usize,
    args_inner: String,
}

/// 扫描 SQL 中顶层已注册表函数（不进入另一个已注册函数的括号内部）。
fn find_top_level_calls(sql: &str, is_func: &dyn Fn(&str) -> bool) -> Result<Vec<TopCall>> {
    let mut out = Vec::new();
    let mut i = 0;
    let bytes = sql.as_bytes();
    while i < sql.len() {
        if let Some(n) = skip_sql_noise(sql, i) {
            i = n;
            continue;
        }
        if is_ident_boundary_before(sql, i) {
            if let Some((name, paren, close)) = parse_call_head(sql, i, is_func) {
                let start = skip_ws(sql, i);
                out.push(TopCall {
                    name,
                    start,
                    end: close + 1,
                    args_inner: sql[paren + 1..close].to_string(),
                });
                i = close + 1;
                continue;
            }
        }
        // 前进一个字节（ASCII 为主；多字节 UTF-8 只要不从 ident 中间切开即可）
        if bytes[i].is_ascii() {
            i += 1;
        } else {
            i += sql[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
    }
    Ok(out)
}

fn is_ident_boundary_before(sql: &str, i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let prev = sql[..i].chars().next_back().unwrap_or(' ');
    !crate::args::is_ident_continue(prev)
}

pub fn eval_arg_slots(
    slots: Vec<ArgSlot>,
    conn: &mut Connection,
    registry: &Registry,
    counter: &mut usize,
    opts: &SyntaxOpts,
) -> Result<Args> {
    let mut args = Args::default();
    for slot in slots {
        match slot {
            ArgSlot::Positional(expr) => {
                args.positional
                    .push(eval_expr(expr, conn, registry, counter, opts)?);
            }
            ArgSlot::Named(name, expr) => {
                args.named
                    .insert(name, eval_expr(expr, conn, registry, counter, opts)?);
            }
        }
    }
    Ok(args)
}

fn eval_expr(
    expr: ArgExpr,
    conn: &mut Connection,
    registry: &Registry,
    counter: &mut usize,
    opts: &SyntaxOpts,
) -> Result<Value> {
    match expr {
        ArgExpr::Literal(v) => Ok(v),
        ArgExpr::Call { name, args } => {
            match eval_call(&name, args, conn, registry, counter, None, opts)? {
                CallResult::Table(t) => Ok(Value::Str(t)),
                CallResult::Scalar(s) => Ok(Value::Str(s)),
            }
        }
    }
}

enum CallResult {
    Table(String),
    Scalar(String),
}

fn eval_call(
    name: &str,
    slots: Vec<ArgSlot>,
    conn: &mut Connection,
    registry: &Registry,
    counter: &mut usize,
    dest: Option<String>,
    opts: &SyntaxOpts,
) -> Result<CallResult> {
    validate_source_args(name, &slots, opts)?;
    if !matches!(name.to_ascii_lowercase().as_str(), "mock_data" | "mockdata") {
        if let Some(fmt) = resolve_format(name, &slots) {
            let keys: Vec<String> = slots
                .iter()
                .filter_map(|s| match s {
                    ArgSlot::Named(k, _) => Some(k.clone()),
                    _ => None,
                })
                .collect();
            validate_closed_options(&fmt, keys)?;
        }
    }
    let (name, slots) = desugar_call(name, slots);
    let args = eval_arg_slots(slots, conn, registry, counter, opts)?;
    let dest = dest.unwrap_or_else(|| {
        let t = format!("excel_tmp_{}", *counter);
        *counter += 1;
        t
    });
    let mut ctx = ExecCtx {
        conn,
        dest_table: dest.clone(),
    };
    match registry.execute(&mut ctx, &name, &args)? {
        FuncOutput::Table => Ok(CallResult::Table(dest)),
        FuncOutput::Scalar(s) => Ok(CallResult::Scalar(s)),
    }
}

/// 求值 LOAD 的数据源，写入 `dest_table`（若给定）。
pub fn eval_source(
    source: &SourceSpec,
    dest_table: Option<String>,
    conn: &mut Connection,
    registry: &Registry,
    counter: &mut usize,
    opts: &SyntaxOpts,
) -> Result<CallOut> {
    match source {
        SourceSpec::Locator { uri, options } => {
            let mut slots = vec![ArgSlot::Positional(ArgExpr::Literal(Value::Str(
                uri.clone(),
            )))];
            slots.extend(options.clone());
            match eval_call("read", slots, conn, registry, counter, dest_table, opts)? {
                CallResult::Table(t) => Ok(CallOut::Table(t)),
                CallResult::Scalar(s) => Ok(CallOut::Scalar(s)),
            }
        }
        SourceSpec::Call { name, args } => {
            match eval_call(
                name,
                args.clone(),
                conn,
                registry,
                counter,
                dest_table,
                opts,
            )? {
                CallResult::Table(t) => Ok(CallOut::Table(t)),
                CallResult::Scalar(s) => Ok(CallOut::Scalar(s)),
            }
        }
    }
}

/// 把 SQL 中的表函数求值并替换成临时表名（或 SQL 字符串字面量）。
pub fn rewrite_sql(
    sql: &str,
    conn: &mut Connection,
    registry: &Registry,
    counter: &mut usize,
    opts: &SyntaxOpts,
) -> Result<String> {
    let is_func = |n: &str| registry.is_func(n);
    let mut calls = find_top_level_calls(sql, &is_func)?;
    if calls.is_empty() {
        return Ok(sql.to_string());
    }
    calls.sort_by_key(|c| c.start);

    let places = classify_calls(sql, &calls, opts)?;

    let mut out = String::with_capacity(sql.len());
    let mut last = 0;
    for (idx, call) in calls.iter().enumerate() {
        out.push_str(&sql[last..call.start]);
        let slots = parse_arg_list(&call.args_inner, &is_func)?;
        let place = places.get(idx).copied().unwrap_or(SourcePlace::Unknown);
        let replacement =
            rewrite_one_call(&call.name, slots, place, conn, registry, counter, opts)?;
        out.push_str(&replacement);
        last = call.end;
    }
    out.push_str(&sql[last..]);
    Ok(out)
}

fn classify_calls(sql: &str, calls: &[TopCall], opts: &SyntaxOpts) -> Result<Vec<SourcePlace>> {
    let mut stubbed = sql.to_string();
    for (i, call) in calls.iter().enumerate().rev() {
        stubbed.replace_range(call.start..call.end, &classify::stub_name(i));
    }
    match classify::classify_source_places(&stubbed, calls.len()) {
        Ok(places) => {
            if opts.effective_strict() {
                classify::require_classified(&places, true)?;
            }
            Ok(places)
        }
        Err(err) => {
            if opts.effective_strict() {
                Err(err)
            } else {
                eprintln!("⚠️  {err}；syntax=1 回退为按出现顺序物化源。");
                Ok(vec![SourcePlace::Table; calls.len()])
            }
        }
    }
}

fn rewrite_one_call(
    name: &str,
    slots: Vec<ArgSlot>,
    place: SourcePlace,
    conn: &mut Connection,
    registry: &Registry,
    counter: &mut usize,
    opts: &SyntaxOpts,
) -> Result<String> {
    let scalar = is_scalar_source(name, &slots);
    match place {
        SourcePlace::Table | SourcePlace::Unknown => {
            if scalar {
                bail!(
                    "标量源 `{}` 不能当作表；只能出现在另一个 read()/LOAD 的参数里",
                    name
                );
            }
            validate_query_source_name(name, opts)?;
            match eval_call(name, slots, conn, registry, counter, None, opts)? {
                CallResult::Table(t) => Ok(t),
                CallResult::Scalar(s) => Ok(sql_quote(&s)),
            }
        }
        SourcePlace::Expr => {
            if !scalar {
                bail!(
                    "表函数 `{}` 只能出现在 FROM/JOIN 或 LOAD 中，不能写在 SELECT/WHERE 表达式里",
                    name
                );
            }
            if !opts.allow_expr_scalars() {
                bail!(
                    "syntax strict：`{}` 是标量源，不能写在查询表达式里；请放进 LOAD / read() 的参数",
                    name
                );
            }
            eprintln!(
                "⚠️  查询表达式里的 `{}` 已废弃，请改为 LOAD 参数或 read(...) 嵌套调用",
                name
            );
            match eval_call(name, slots, conn, registry, counter, None, opts)? {
                CallResult::Scalar(s) => Ok(sql_quote(&s)),
                CallResult::Table(t) => Ok(t),
            }
        }
    }
}

/// 若整段输入就是一个表函数调用（可带末尾分号），返回它。
pub fn parse_standalone_call(sql: &str, registry: &Registry) -> Option<(String, String)> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let is_func = |n: &str| registry.is_func(n);
    let i = skip_ws(trimmed, 0);
    let (name, after) = parse_ident(trimmed, i)?;
    if !is_func(&name) {
        return None;
    }
    let j = skip_ws(trimmed, after);
    if j >= trimmed.len() || trimmed.as_bytes()[j] != b'(' {
        return None;
    }
    let close = matching_paren(trimmed, j).ok()?;
    let rest = skip_ws(trimmed, close + 1);
    if rest != trimmed.len() {
        return None;
    }
    Some((name, trimmed[j + 1..close].to_string()))
}

pub fn eval_standalone(
    name: &str,
    args_inner: &str,
    conn: &mut Connection,
    registry: &Registry,
    counter: &mut usize,
    opts: &SyntaxOpts,
) -> Result<CallOut> {
    let is_func = |n: &str| registry.is_func(n);
    let slots = parse_arg_list(args_inner, &is_func)?;
    match eval_call(name, slots, conn, registry, counter, None, opts)? {
        CallResult::Table(t) => Ok(CallOut::Table(t)),
        CallResult::Scalar(s) => Ok(CallOut::Scalar(s)),
    }
}

pub enum CallOut {
    Table(String),
    Scalar(String),
}

/// 按分号切语句，忽略字符串和注释里的分号。
pub fn split_statements(sql: &str) -> Result<Vec<String>> {
    let mut stmts = Vec::new();
    let mut i = 0;
    let mut start = 0;
    while i < sql.len() {
        if let Some(n) = skip_sql_noise(sql, i) {
            i = n;
            continue;
        }
        if sql.as_bytes()[i] == b';' {
            let piece = sql[start..i].trim();
            if !piece.is_empty() {
                stmts.push(piece.to_string());
            }
            i += 1;
            start = i;
            continue;
        }
        i += 1;
    }
    let piece = sql[start..].trim();
    if !piece.is_empty() {
        stmts.push(piece.to_string());
    }
    if stmts.is_empty() {
        bail!("SQL 为空");
    }
    Ok(stmts)
}

pub fn is_query(sql: &str) -> bool {
    let t = sql.trim_start();
    let lower: String = t.chars().take(12).collect::<String>().to_ascii_lowercase();
    lower.starts_with("select")
        || lower.starts_with("with")
        || lower.starts_with("pragma")
        || lower.starts_with("explain")
        || lower.starts_with("values")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::Registry;
    use rusqlite::Connection;

    fn sess() -> (Connection, Registry, usize) {
        (
            Connection::open_in_memory().unwrap(),
            Registry::builtin(),
            0,
        )
    }

    #[test]
    fn skips_strings_and_comments() {
        let (mut conn, reg, mut c) = sess();
        let sql = "SELECT 'read_csv(\"x\")' AS a -- read_csv('y')\nFROM mock_data(1, 'id:name')";
        let out = rewrite_sql(sql, &mut conn, &reg, &mut c, &SyntaxOpts::default()).unwrap();
        assert!(out.contains("excel_tmp_0"), "{out}");
        assert!(out.contains("read_csv"), "{out}");
    }

    #[test]
    fn nested_scalar_not_injected_raw() {
        // mock 不嵌套文件；用 parse 保证括号匹配
        let sql = "SELECT * FROM mock_data(2, '用户:name', '城:city')";
        let (mut conn, reg, mut c) = sess();
        let out = rewrite_sql(sql, &mut conn, &reg, &mut c, &SyntaxOpts::default()).unwrap();
        assert_eq!(out.trim(), "SELECT * FROM excel_tmp_0");
    }

    #[test]
    fn named_and_json_args_parse() {
        let is_func = |_: &str| false;
        let slots = parse_arg_list(
            "'http://x', 'POST', {\"a\":1}, headers='{\"k\":\"v\"}'",
            &is_func,
        )
        .unwrap();
        assert_eq!(slots.len(), 4);
    }

    #[test]
    fn split_respects_quotes() {
        let s = "SELECT 'a;b' AS x; SELECT 1";
        let v = split_statements(s).unwrap();
        assert_eq!(v.len(), 2);
    }
}
