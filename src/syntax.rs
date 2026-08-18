use crate::args::{
    matching_paren, parse_arg_list, parse_ident, parse_string_literal, skip_sql_noise, skip_ws,
    ArgSlot,
};
use anyhow::{bail, Result};

/// 语言行为开关。`--syntax=1` 对应默认值。
#[derive(Clone, Debug, Default)]
pub struct SyntaxOpts {
    pub strict: bool,
}

#[derive(Debug, Clone)]
pub enum ScriptStmt {
    Load { name: String, source: SourceSpec },
    Query(String),
}

#[derive(Debug, Clone)]
pub enum SourceSpec {
    Locator { uri: String, options: Vec<ArgSlot> },
    Call { name: String, args: Vec<ArgSlot> },
}

pub fn parse_script(sql: &str) -> Result<Vec<ScriptStmt>> {
    let pieces = crate::rewrite::split_statements(sql)?;
    pieces.into_iter().map(|s| parse_statement(&s)).collect()
}

fn skip_leading(s: &str) -> usize {
    let mut i = 0;
    loop {
        let j = skip_ws(s, i);
        if let Some(n) = skip_sql_noise(s, j) {
            i = n;
            continue;
        }
        return j;
    }
}

fn parse_statement(s: &str) -> Result<ScriptStmt> {
    let i = skip_leading(s);
    if let Some((ident, after)) = parse_ident(s, i) {
        if ident.eq_ignore_ascii_case("load") {
            return parse_load(s, after);
        }
    }
    Ok(ScriptStmt::Query(s.to_string()))
}

fn parse_load(s: &str, after_load: usize) -> Result<ScriptStmt> {
    let i = skip_ws(s, after_load);
    let (first, after_first) = parse_ident(s, i).ok_or_else(|| anyhow::anyhow!("LOAD 缺少表名"))?;
    let (name, after_name) = if first.eq_ignore_ascii_case("table") {
        parse_ident(s, skip_ws(s, after_first))
            .ok_or_else(|| anyhow::anyhow!("LOAD TABLE 缺少表名"))?
    } else {
        (first, after_first)
    };
    validate_table_name(&name)?;

    let i = skip_ws(s, after_name);
    let (from_kw, after_from_kw) =
        parse_ident(s, i).ok_or_else(|| anyhow::anyhow!("LOAD 需要 FROM"))?;
    if !from_kw.eq_ignore_ascii_case("from") {
        bail!("LOAD 需要 FROM，在 `{}` 处", from_kw);
    }
    let i = skip_ws(s, after_from_kw);
    if i >= s.len() {
        bail!("LOAD ... FROM 后面缺少数据源");
    }

    let bytes = s.as_bytes();
    if bytes[i] == b'\'' || bytes[i] == b'"' {
        let (uri, end) = parse_string_literal(s, i)?;
        let j = skip_ws(s, end);
        let options = if j >= s.len() {
            Vec::new()
        } else if let Some((kw, after_kw)) = parse_ident(s, j) {
            if !kw.eq_ignore_ascii_case("with") {
                bail!("FROM 字符串之后只能跟 WITH (...)，发现 `{}`", kw);
            }
            parse_with_options(s, skip_ws(s, after_kw))?
        } else {
            bail!("无法解析 LOAD ... FROM 之后的内容");
        };
        return Ok(ScriptStmt::Load {
            name,
            source: SourceSpec::Locator { uri, options },
        });
    }

    if let Some((fname, after_fname)) = parse_ident(s, i) {
        let j = skip_ws(s, after_fname);
        if j < s.len() && bytes[j] == b'(' {
            let close = matching_paren(s, j)?;
            let inner = &s[j + 1..close];
            // Source 参数里任意 ident( 都当嵌套调用，由注册表在求值时报未知函数。
            let args = parse_arg_list(inner, &|_| true)?;
            let rest = skip_ws(s, close + 1);
            if rest != s.len() {
                bail!("LOAD 语句末尾有多余内容");
            }
            return Ok(ScriptStmt::Load {
                name,
                source: SourceSpec::Call { name: fname, args },
            });
        }
    }

    bail!("LOAD ... FROM 需要路径字符串或 read(...) 调用");
}

fn parse_with_options(s: &str, i: usize) -> Result<Vec<ArgSlot>> {
    if i >= s.len() || s.as_bytes()[i] != b'(' {
        bail!("WITH 需要括号列表，例如 WITH (sheet='Sheet1', skip=2)");
    }
    let close = matching_paren(s, i)?;
    let inner = &s[i + 1..close];
    let slots = parse_arg_list(inner, &|_| true)?;
    for slot in &slots {
        if matches!(slot, ArgSlot::Positional(_)) {
            bail!("WITH (...) 只允许命名参数，例如 sheet='Sheet1'，不要写位置参数");
        }
    }
    let rest = skip_ws(s, close + 1);
    if rest != s.len() {
        bail!("LOAD 语句末尾有多余内容");
    }
    Ok(slots)
}

pub fn validate_table_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        bail!("表名为空");
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        bail!("表名 `{}` 必须以字母或下划线开头", name);
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!("表名 `{}` 只能包含字母、数字、下划线", name);
    }
    match name.to_ascii_lowercase().as_str() {
        "select" | "from" | "where" | "join" | "with" | "load" | "table" | "as" | "group"
        | "order" | "limit" | "insert" | "update" | "delete" => {
            bail!("`{}` 是保留字，不能用作 LOAD 表名", name)
        }
        _ => Ok(()),
    }
}

/// `--strict`：除第一个定位符外必须是命名参数。
pub fn validate_source_args(func: &str, slots: &[ArgSlot], strict: bool) -> Result<()> {
    if !strict {
        return Ok(());
    }
    let lower = func.to_ascii_lowercase();
    if matches!(lower.as_str(), "mock_data" | "mockdata") {
        return Ok(());
    }
    for (i, slot) in slots.iter().enumerate() {
        if i == 0 {
            continue;
        }
        if matches!(slot, ArgSlot::Positional(_)) {
            bail!(
                "syntax strict：`{}` 除第一个定位符外必须使用命名参数，例如 sheet='Sheet1'、skip=2",
                func
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_from_locator_with_named() {
        let stmts =
            parse_script("LOAD users FROM 'a.xlsx' WITH (sheet='S1', skip=2); SELECT * FROM users")
                .unwrap();
        assert_eq!(stmts.len(), 2);
        match &stmts[0] {
            ScriptStmt::Load { name, source } => {
                assert_eq!(name, "users");
                match source {
                    SourceSpec::Locator { uri, options } => {
                        assert_eq!(uri, "a.xlsx");
                        assert_eq!(options.len(), 2);
                    }
                    _ => panic!("expected locator"),
                }
            }
            _ => panic!("expected load"),
        }
        assert!(matches!(stmts[1], ScriptStmt::Query(_)));
    }

    #[test]
    fn load_rejects_positional_with() {
        let err = parse_script("LOAD t FROM 'a.csv' WITH ('x')").unwrap_err();
        assert!(format!("{err}").contains("命名参数"), "{err}");
    }

    #[test]
    fn load_from_call() {
        let stmts = parse_script("LOAD t FROM read_csv('a.csv')").unwrap();
        match &stmts[0] {
            ScriptStmt::Load {
                source: SourceSpec::Call { name, .. },
                ..
            } => assert_eq!(name, "read_csv"),
            _ => panic!("expected call"),
        }
    }
}
