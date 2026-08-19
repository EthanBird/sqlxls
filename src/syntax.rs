use crate::args::{
    matching_paren, parse_arg_list, parse_ident, parse_string_literal, skip_sql_noise, skip_ws,
    ArgExpr, ArgSlot, Value,
};
use anyhow::{bail, Result};

/// 语言行为开关。`--syntax=1` 对应默认值。
#[derive(Clone, Debug)]
pub struct SyntaxOpts {
    pub version: u8,
    pub strict: bool,
}

impl Default for SyntaxOpts {
    fn default() -> Self {
        Self {
            version: 1,
            strict: false,
        }
    }
}

impl SyntaxOpts {
    pub fn effective_strict(&self) -> bool {
        self.strict || self.version >= 2
    }

    /// syntax=2：FROM 只接受表名或规范 `read()` / `mock_data`。
    pub fn allow_sugar_names(&self) -> bool {
        self.version < 2
    }

    pub fn allow_expr_scalars(&self) -> bool {
        self.version < 2 && !self.strict
    }

    pub fn warn_aliases(&self) -> bool {
        self.effective_strict()
    }
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

const HISTORICAL_ALIASES: &[&str] = &[
    "readexcel",
    "readcsv",
    "readjson",
    "readapi",
    "readdir",
    "readglob",
    "readclipboard",
    "readtext",
    "mockdata",
];

const SUGAR_NAMES: &[&str] = &[
    "read_excel",
    "readexcel",
    "read_csv",
    "readcsv",
    "read_json",
    "readjson",
    "read_api",
    "readapi",
    "read_dir",
    "readdir",
    "read_glob",
    "readglob",
    "read_clipboard",
    "readclipboard",
    "read_text",
    "readtext",
];

fn sugar_format(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "read_excel" | "readexcel" => Some("excel"),
        "read_csv" | "readcsv" => Some("csv"),
        "read_json" | "readjson" => Some("json"),
        "read_api" | "readapi" => Some("http"),
        "read_dir" | "readdir" | "read_glob" | "readglob" => Some("glob"),
        "read_clipboard" | "readclipboard" => Some("clipboard"),
        "read_text" | "readtext" => Some("text"),
        _ => None,
    }
}

pub fn is_historical_alias(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    HISTORICAL_ALIASES.contains(&n.as_str())
}

pub fn is_sugar_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    SUGAR_NAMES.contains(&n.as_str())
}

/// 把糖函数收成规范 `read(..., format=...)`。`mock_data` 保持原样。
pub fn desugar_call(name: &str, mut slots: Vec<ArgSlot>) -> (String, Vec<ArgSlot>) {
    let Some(fmt) = sugar_format(name) else {
        return (name.to_ascii_lowercase(), slots);
    };
    let has_format = slots.iter().any(|s| match s {
        ArgSlot::Named(k, _) => k == "format" || k == "fmt",
        _ => false,
    });
    if !has_format {
        slots.push(ArgSlot::Named(
            "format".into(),
            ArgExpr::Literal(Value::Str(fmt.into())),
        ));
    }
    ("read".into(), slots)
}

pub fn named_format(slots: &[ArgSlot]) -> Option<String> {
    for slot in slots {
        if let ArgSlot::Named(k, ArgExpr::Literal(Value::Str(s))) = slot {
            if k == "format" || k == "fmt" {
                return Some(s.to_ascii_lowercase());
            }
        }
    }
    None
}

pub fn locator_literal(slots: &[ArgSlot]) -> Option<String> {
    match slots.first() {
        Some(ArgSlot::Positional(ArgExpr::Literal(Value::Str(s)))) => Some(s.clone()),
        Some(ArgSlot::Named(k, ArgExpr::Literal(Value::Str(s))))
            if matches!(k.as_str(), "path" | "file" | "url" | "locator") =>
        {
            Some(s.clone())
        }
        _ => None,
    }
}

pub fn infer_format(src: &str) -> Option<String> {
    let lower = src.to_ascii_lowercase();
    if lower == "clip:" || lower == "clipboard:" || lower.starts_with("clip:") {
        return Some("clipboard".into());
    }
    if lower.starts_with("glob:") {
        return Some("glob".into());
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some("http".into());
    }
    if lower.contains('*') || lower.contains('?') {
        return Some("glob".into());
    }
    let ext = std::path::Path::new(src)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())?;
    Some(match ext.as_str() {
        "xlsx" | "xls" | "xlsm" => "excel".into(),
        "csv" | "tsv" => "csv".into(),
        "json" => "json".into(),
        "txt" => "text".into(),
        _ => return None,
    })
}

pub fn resolve_format(name: &str, slots: &[ArgSlot]) -> Option<String> {
    if let Some(fmt) = named_format(slots) {
        return Some(fmt);
    }
    if let Some(fmt) = sugar_format(name) {
        return Some(fmt.into());
    }
    locator_literal(slots).and_then(|u| infer_format(&u))
}

const COMMON_OPTS: &[&str] = &["format", "fmt", "path", "file", "url", "locator"];

fn format_options(format: &str) -> &'static [&'static str] {
    match format {
        "excel" | "xlsx" | "xls" | "xlsm" => {
            &["sheet", "skip", "skiprows", "skip_rows", "str", "force_str"]
        }
        "csv" | "tsv" => &[
            "delim",
            "delimiter",
            "sep",
            "skip",
            "skiprows",
            "str",
            "force_str",
        ],
        "json" => &["json_path", "path", "pointer"],
        "http" | "https" | "api" => &[
            "method",
            "body",
            "payload",
            "data",
            "headers",
            "json_path",
            "path",
        ],
        "clipboard" | "clip" => &["delim", "delimiter", "sep", "str", "force_str"],
        "text" | "txt" => &[],
        "glob" | "dir" => &[
            "sheet",
            "skip",
            "skiprows",
            "skip_rows",
            "str",
            "force_str",
            "delim",
            "delimiter",
            "sep",
            "json_path",
            "path",
            "pointer",
        ],
        _ => &[],
    }
}

pub fn validate_closed_options(
    format: &str,
    named_keys: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<()> {
    let extra = format_options(format);
    for key in named_keys {
        let k = key.as_ref();
        let ok = COMMON_OPTS.iter().any(|a| a.eq_ignore_ascii_case(k))
            || extra.iter().any(|a| a.eq_ignore_ascii_case(k));
        if !ok {
            let mut allowed: Vec<&str> = COMMON_OPTS
                .iter()
                .copied()
                .chain(extra.iter().copied())
                .collect();
            allowed.sort();
            allowed.dedup();
            bail!(
                "format='{}' 不支持选项 `{}`。syntax=1 允许: {}",
                format,
                k,
                allowed.join(", ")
            );
        }
    }
    Ok(())
}

pub fn is_scalar_source(name: &str, slots: &[ArgSlot]) -> bool {
    match resolve_format(name, slots).unwrap_or_default().as_str() {
        "text" | "txt" => true,
        _ => matches!(name.to_ascii_lowercase().as_str(), "read_text" | "readtext"),
    }
}

pub fn validate_query_source_name(name: &str, opts: &SyntaxOpts) -> Result<()> {
    let lower = name.to_ascii_lowercase();
    if lower == "read" || matches!(lower.as_str(), "mock_data" | "mockdata") {
        return Ok(());
    }
    if !opts.allow_sugar_names() {
        bail!(
            "syntax={}: 查询里的表位置只允许 read() 或 mock_data()，请写 read(..., format='...') 或改用 LOAD",
            opts.version
        );
    }
    if is_historical_alias(&lower) && opts.warn_aliases() {
        eprintln!(
            "⚠️  `{}` 是历史别名，请改用 read_{} 或规范 read(..., format=...)",
            name,
            lower.trim_start_matches("read")
        );
    }
    Ok(())
}

/// `--strict` / syntax=2：除第一个定位符外必须是命名参数。
pub fn validate_source_args(func: &str, slots: &[ArgSlot], opts: &SyntaxOpts) -> Result<()> {
    if !opts.effective_strict() {
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

    #[test]
    fn closed_options_reject_unknown() {
        let err = validate_closed_options("csv", ["foo"]).unwrap_err();
        assert!(format!("{err}").contains("foo"), "{err}");
    }

    #[test]
    fn desugar_excel_adds_format() {
        let slots = vec![ArgSlot::Positional(ArgExpr::Literal(Value::Str(
            "a.xlsx".into(),
        )))];
        let (name, slots) = desugar_call("read_excel", slots);
        assert_eq!(name, "read");
        assert!(named_format(&slots).as_deref() == Some("excel"));
    }
}
