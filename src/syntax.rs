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
    Set { name: String, value: Value },
    Load(LoadStmt),
    Query(String),
}

#[derive(Debug, Clone)]
pub struct LoadStmt {
    pub name: String,
    pub source: SourceSpec,
    pub each: Option<EachSpec>,
    pub fors: Vec<ForClause>,
}

#[derive(Debug, Clone)]
pub enum SourceSpec {
    Locator { uri: String, options: Vec<ArgSlot> },
    Call { name: String, args: Vec<ArgSlot> },
}

#[derive(Debug, Clone)]
pub enum EachSpec {
    List(Vec<String>),
    Glob(String),
}

#[derive(Debug, Clone)]
pub struct ForClause {
    pub vars: Vec<String>,
    pub domain: ForDomain,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ForDateStep {
    /// 日区间按 1 天，月区间按 1 月。
    Default,
    /// `STEP n`：日区间为 n 天，月区间为 n 月。
    Count(i64),
    /// `STEP MONTH` 或 `STEP n MONTH`。
    Months(i64),
}

#[derive(Debug, Clone)]
pub enum ForDomain {
    List(Vec<Vec<Value>>),
    Range {
        start: i64,
        end: i64,
        step: i64,
    },
    Dates {
        start: String,
        end: String,
        step: ForDateStep,
    },
    Glob(String),
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
        if ident.eq_ignore_ascii_case("set") {
            return parse_set(s, after);
        }
    }
    Ok(ScriptStmt::Query(s.to_string()))
}

fn parse_set(s: &str, after_set: usize) -> Result<ScriptStmt> {
    let i = skip_ws(s, after_set);
    let (name, after_name) = parse_ident(s, i).ok_or_else(|| anyhow::anyhow!("SET 缺少变量名"))?;
    validate_bind_name(&name)?;
    let i = skip_ws(s, after_name);
    if i >= s.len() || s.as_bytes()[i] != b'=' {
        bail!("SET 需要 `SET name = value`");
    }
    let (expr, end) = crate::args::parse_arg_expr(s, i + 1, &|_| true)?;
    let rest = skip_ws(s, end);
    if rest != s.len() {
        bail!("SET 语句末尾有多余内容");
    }
    let value = match expr {
        ArgExpr::Literal(v) => v,
        ArgExpr::Call { .. } => {
            bail!("SET 的值必须是字面量（字符串/数字/布尔）。嵌套 read() 请写在 LOAD 里")
        }
    };
    Ok(ScriptStmt::Set { name, value })
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
    let mut each = None;
    let mut source: Option<SourceSpec> = None;
    let mut pos = i;

    if let Some((kw, after_kw)) = parse_ident(s, i) {
        if kw.eq_ignore_ascii_case("each") {
            let (spec, after_each) = parse_each_spec(s, skip_ws(s, after_kw))?;
            each = Some(spec);
            pos = after_each;
            let (options, after_opt) = parse_optional_with(s, pos)?;
            source = Some(SourceSpec::Locator {
                uri: String::new(),
                options,
            });
            pos = after_opt;
        }
    }

    if source.is_none() && (bytes[i] == b'\'' || bytes[i] == b'"') {
        let (uri, end) = parse_string_literal(s, i)?;
        let (options, after_opt) = parse_optional_with(s, skip_ws(s, end))?;
        source = Some(SourceSpec::Locator { uri, options });
        pos = after_opt;
    }

    if source.is_none() {
        if let Some((fname, after_fname)) = parse_ident(s, i) {
            let j = skip_ws(s, after_fname);
            if j < s.len() && bytes[j] == b'(' {
                let close = matching_paren(s, j)?;
                let inner = &s[j + 1..close];
                let args = parse_arg_list(inner, &|_| true)?;
                source = Some(SourceSpec::Call { name: fname, args });
                pos = skip_ws(s, close + 1);
            }
        }
    }

    let Some(source) = source else {
        bail!("LOAD ... FROM 需要路径字符串、EACH (...) 或 read(...) 调用");
    };

    let (fors, rest) = parse_for_clauses(s, pos)?;
    if rest != s.len() {
        bail!(
            "LOAD 语句末尾有多余内容: `{}`",
            s[rest..].chars().take(40).collect::<String>()
        );
    }

    Ok(ScriptStmt::Load(LoadStmt {
        name,
        source,
        each,
        fors,
    }))
}

fn parse_each_spec(s: &str, i: usize) -> Result<(EachSpec, usize)> {
    if let Some((kw, after)) = parse_ident(s, i) {
        if kw.eq_ignore_ascii_case("glob") {
            let j = skip_ws(s, after);
            if j >= s.len() || (s.as_bytes()[j] != b'\'' && s.as_bytes()[j] != b'"') {
                bail!("EACH GLOB 需要路径字符串");
            }
            let (pat, end) = parse_string_literal(s, j)?;
            return Ok((EachSpec::Glob(pat), end));
        }
    }
    if i < s.len() && s.as_bytes()[i] == b'(' {
        let close = matching_paren(s, i)?;
        let inner = &s[i + 1..close];
        let slots = parse_arg_list(inner, &|_| false)?;
        let mut items = Vec::new();
        for slot in slots {
            match slot {
                ArgSlot::Positional(ArgExpr::Literal(Value::Str(v))) => items.push(v),
                _ => bail!("EACH (...) 只接受字符串定位符列表"),
            }
        }
        if items.is_empty() {
            bail!("EACH (...) 至少需要一个定位符");
        }
        return Ok((EachSpec::List(items), close + 1));
    }
    bail!("EACH 需要括号列表或 GLOB '模式'");
}

fn parse_optional_with(s: &str, i: usize) -> Result<(Vec<ArgSlot>, usize)> {
    let i = skip_ws(s, i);
    if i >= s.len() {
        return Ok((Vec::new(), i));
    }
    if let Some((kw, after_kw)) = parse_ident(s, i) {
        if kw.eq_ignore_ascii_case("with") {
            return parse_with_options(s, skip_ws(s, after_kw));
        }
        if kw.eq_ignore_ascii_case("for") {
            return Ok((Vec::new(), i));
        }
        bail!("FROM 之后只能跟 WITH (...) 或 FOR ...，发现 `{kw}`");
    }
    Ok((Vec::new(), i))
}

fn parse_with_options(s: &str, i: usize) -> Result<(Vec<ArgSlot>, usize)> {
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
    Ok((slots, skip_ws(s, close + 1)))
}

fn parse_for_clauses(s: &str, mut i: usize) -> Result<(Vec<ForClause>, usize)> {
    let mut out = Vec::new();
    loop {
        i = skip_ws(s, i);
        if i >= s.len() {
            break;
        }
        let Some((kw, after)) = parse_ident(s, i) else {
            break;
        };
        if !kw.eq_ignore_ascii_case("for") {
            break;
        }
        let j = skip_ws(s, after);
        let (vars, after_vars) = parse_for_vars(s, j)?;
        if vars.is_empty() {
            bail!("FOR 缺少变量名");
        }
        for v in &vars {
            validate_bind_name(v)?;
        }
        let shown = vars.join(", ");
        let j = skip_ws(s, after_vars);
        let (in_kw, after_in) =
            parse_ident(s, j).ok_or_else(|| anyhow::anyhow!("FOR {shown} 需要 IN"))?;
        if !in_kw.eq_ignore_ascii_case("in") {
            bail!("FOR {shown} 需要 IN，发现 `{in_kw}`");
        }
        let (domain, after_domain) = parse_for_domain(s, skip_ws(s, after_in))?;
        match &domain {
            ForDomain::Range { .. } | ForDomain::Dates { .. } | ForDomain::Glob(_)
                if vars.len() != 1 =>
            {
                bail!("范围和 GLOB 只能绑定一个变量，请写 FOR {} IN ...", vars[0]);
            }
            ForDomain::List(rows) => {
                for (i, row) in rows.iter().enumerate() {
                    if row.len() != vars.len() {
                        bail!(
                            "FOR ({shown}) 第 {} 行有 {} 个值，需要 {}",
                            i + 1,
                            row.len(),
                            vars.len()
                        );
                    }
                }
            }
            _ => {}
        }
        out.push(ForClause { vars, domain });
        i = after_domain;
    }
    Ok((out, skip_ws(s, i)))
}

fn parse_for_vars(s: &str, i: usize) -> Result<(Vec<String>, usize)> {
    if i < s.len() && s.as_bytes()[i] == b'(' {
        let close = matching_paren(s, i)?;
        let inner = &s[i + 1..close];
        let vars = parse_ident_csv(inner)?;
        if vars.is_empty() {
            bail!("FOR (...) 至少需要一个变量名");
        }
        return Ok((vars, close + 1));
    }
    let mut vars = Vec::new();
    let mut p = i;
    loop {
        let (name, after) = parse_ident(s, p).ok_or_else(|| anyhow::anyhow!("FOR 缺少变量名"))?;
        vars.push(name);
        p = skip_ws(s, after);
        if p < s.len() && s.as_bytes()[p] == b',' {
            p = skip_ws(s, p + 1);
            continue;
        }
        return Ok((vars, after));
    }
}

fn parse_ident_csv(s: &str) -> Result<Vec<String>> {
    let mut i = skip_ws(s, 0);
    let mut vars = Vec::new();
    if i >= s.len() {
        return Ok(vars);
    }
    loop {
        let (name, after) =
            parse_ident(s, i).ok_or_else(|| anyhow::anyhow!("FOR (...) 需要变量名列表"))?;
        vars.push(name);
        i = skip_ws(s, after);
        if i >= s.len() {
            break;
        }
        if s.as_bytes()[i] != b',' {
            bail!("FOR (...) 变量名之间用逗号分隔");
        }
        i = skip_ws(s, i + 1);
    }
    Ok(vars)
}

fn parse_for_list_rows(inner: &str) -> Result<Vec<Vec<Value>>> {
    let i = skip_ws(inner, 0);
    if i < inner.len() && inner.as_bytes()[i] == b'(' {
        let mut rows = Vec::new();
        let mut p = i;
        loop {
            p = skip_ws(inner, p);
            if p >= inner.len() {
                break;
            }
            if inner.as_bytes()[p] != b'(' {
                bail!("多变量 FOR 的每一行必须是元组，例如 ('east', 'prod')");
            }
            let close = matching_paren(inner, p)?;
            rows.push(parse_scalar_row(&inner[p + 1..close])?);
            p = skip_ws(inner, close + 1);
            if p >= inner.len() {
                break;
            }
            if inner.as_bytes()[p] != b',' {
                bail!("元组之间用逗号分隔");
            }
            p += 1;
        }
        return Ok(rows);
    }
    Ok(parse_scalar_row(inner)?
        .into_iter()
        .map(|v| vec![v])
        .collect())
}

fn parse_scalar_row(s: &str) -> Result<Vec<Value>> {
    let slots = parse_arg_list(s, &|_| false)?;
    let mut vals = Vec::new();
    for slot in slots {
        match slot {
            ArgSlot::Positional(ArgExpr::Literal(v)) => vals.push(v),
            _ => bail!("FOR ... IN 只接受字面量"),
        }
    }
    Ok(vals)
}

fn parse_for_domain(s: &str, i: usize) -> Result<(ForDomain, usize)> {
    let mut i = skip_ws(s, i);
    let mut force_date = false;
    if let Some((kw, after)) = parse_ident(s, i) {
        if kw.eq_ignore_ascii_case("glob") {
            let j = skip_ws(s, after);
            if j >= s.len() || (s.as_bytes()[j] != b'\'' && s.as_bytes()[j] != b'"') {
                bail!("FOR ... IN GLOB 需要路径字符串");
            }
            let (pat, end) = parse_string_literal(s, j)?;
            return Ok((ForDomain::Glob(pat), end));
        }
        if kw.eq_ignore_ascii_case("date") {
            force_date = true;
            i = skip_ws(s, after);
        } else {
            bail!("FOR ... IN 后面不能是标识符 `{kw}`，请用列表、范围或 GLOB");
        }
    }
    if !force_date && i < s.len() && s.as_bytes()[i] == b'(' {
        let close = matching_paren(s, i)?;
        let inner = &s[i + 1..close];
        let rows = parse_for_list_rows(inner)?;
        if rows.is_empty() {
            bail!("FOR ... IN (...) 至少需要一个值");
        }
        return Ok((ForDomain::List(rows), close + 1));
    }
    if let Some((start, after_start, start_is_str)) = parse_range_bound(s, i) {
        let j = skip_ws(s, after_start);
        if s[j..].starts_with("..") {
            let (end, after_end, end_is_str) =
                parse_range_bound(s, skip_ws(s, j + 2)).ok_or_else(|| {
                    anyhow::anyhow!("范围缺少结束值，例如 1..12 或 DATE '2024-01-01'..'2024-01-31'")
                })?;
            let (step, after_step) = parse_optional_step(s, after_end)?;
            let start_int = if start_is_str {
                None
            } else {
                start.parse::<i64>().ok()
            };
            let end_int = if end_is_str {
                None
            } else {
                end.parse::<i64>().ok()
            };
            let as_date = force_date;
            if (start_is_str || end_is_str) && !force_date {
                bail!(
                    "字符串范围语义不明。日期必须写成 DATE '2024-01-01'..'2024-01-31'，月份写成 DATE '2024-01'..'2024-12'"
                );
            }
            if as_date {
                let step = match step {
                    None => ForDateStep::Default,
                    Some(StepTok::Count(n)) => ForDateStep::Count(n),
                    Some(StepTok::Months(n)) => ForDateStep::Months(n),
                };
                return Ok((ForDomain::Dates { start, end, step }, after_step));
            }
            let (Some(a), Some(b)) = (start_int, end_int) else {
                bail!("整数范围的两端都必须是整数");
            };
            let step = match step {
                None => 1,
                Some(StepTok::Count(n)) => n,
                Some(StepTok::Months(_)) => {
                    bail!(
                        "整数范围不能 STEP MONTH，请写 DATE '2024-01-01'..'2024-12-31' STEP MONTH"
                    )
                }
            };
            return Ok((
                ForDomain::Range {
                    start: a,
                    end: b,
                    step,
                },
                after_step,
            ));
        }
        if force_date {
            bail!("DATE 范围需要 '..'，例如 DATE '2024-01-01'..'2024-01-31'");
        }
    }
    bail!("FOR ... IN 需要 ('a','b')、1..12、DATE '2024-01-01'..'2024-01-31' 或 GLOB 'pat'");
}

fn parse_range_bound(s: &str, i: usize) -> Option<(String, usize, bool)> {
    let i = skip_ws(s, i);
    if i < s.len() {
        let b = s.as_bytes()[i];
        if b == b'\'' || b == b'"' {
            let (val, end) = parse_string_literal(s, i).ok()?;
            return Some((val, end, true));
        }
    }
    if let Some((n, end)) = parse_int_at(s, i) {
        return Some((n.to_string(), end, false));
    }
    None
}

enum StepTok {
    Count(i64),
    Months(i64),
}

fn parse_optional_step(s: &str, i: usize) -> Result<(Option<StepTok>, usize)> {
    let i = skip_ws(s, i);
    let Some((kw, after)) = parse_ident(s, i) else {
        return Ok((None, i));
    };
    if !kw.eq_ignore_ascii_case("step") {
        return Ok((None, i));
    }
    let j = skip_ws(s, after);
    if let Some((n, after_n)) = parse_int_at(s, j) {
        let k = skip_ws(s, after_n);
        if let Some((unit, after_unit)) = parse_ident(s, k) {
            if unit.eq_ignore_ascii_case("month") || unit.eq_ignore_ascii_case("months") {
                return Ok((Some(StepTok::Months(n)), after_unit));
            }
            if unit.eq_ignore_ascii_case("day") || unit.eq_ignore_ascii_case("days") {
                return Ok((Some(StepTok::Count(n)), after_unit));
            }
        }
        return Ok((Some(StepTok::Count(n)), after_n));
    }
    if let Some((unit, after_unit)) = parse_ident(s, j) {
        if unit.eq_ignore_ascii_case("month") || unit.eq_ignore_ascii_case("months") {
            return Ok((Some(StepTok::Months(1)), after_unit));
        }
        if unit.eq_ignore_ascii_case("day") || unit.eq_ignore_ascii_case("days") {
            return Ok((Some(StepTok::Count(1)), after_unit));
        }
        bail!("STEP 需要整数、MONTH 或 n MONTH");
    }
    bail!("STEP 需要整数或 MONTH");
}

fn parse_int_at(s: &str, start: usize) -> Option<(i64, usize)> {
    let rest = &s[start..];
    let bytes = rest.as_bytes();
    let mut i = 0;
    if bytes.first() == Some(&b'-') || bytes.first() == Some(&b'+') {
        i = 1;
    }
    if i >= bytes.len() || !bytes[i].is_ascii_digit() {
        return None;
    }
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let token = &rest[..i];
    let n: i64 = token.parse().ok()?;
    Some((n, start + i))
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
        | "order" | "limit" | "insert" | "update" | "delete" | "set" | "each" | "for" | "in"
        | "glob" | "step" | "union" => {
            bail!("`{}` 是保留字，不能用作 LOAD 表名", name)
        }
        _ => Ok(()),
    }
}

pub fn validate_bind_name(name: &str) -> Result<()> {
    validate_table_name(name).map_err(|e| anyhow::anyhow!("变量名不合法: {e}"))
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

const COMMON_OPTS: &[&str] = &[
    "format",
    "fmt",
    "path",
    "file",
    "url",
    "locator",
    // 传输/展开选项：与 payload format 正交
    "page_param",
    "page_from",
    "page_to",
    "page_size",
    "page_size_param",
    "offset_param",
    "offset_step",
    "stop",
    "include_source",
];

fn format_options(format: &str) -> &'static [&'static str] {
    match format {
        "excel" | "xlsx" | "xls" | "xlsm" => &[
            "sheet",
            "skip",
            "skiprows",
            "skip_rows",
            "str",
            "force_str",
            "include_source",
        ],
        "csv" | "tsv" => &[
            "delim",
            "delimiter",
            "sep",
            "skip",
            "skiprows",
            "str",
            "force_str",
            "encoding",
            "charset",
        ],
        "json" => &["json_path", "path", "pointer", "encoding", "charset"],
        "http" | "https" | "api" => &[
            "method",
            "body",
            "payload",
            "data",
            "headers",
            "json_path",
            "path",
            "page_param",
            "page_from",
            "page_to",
            "page_size",
            "page_size_param",
            "offset_param",
            "offset_step",
            "stop",
            "include_source",
            "encoding",
            "charset",
            "sheet",
            "skip",
            "skiprows",
            "delim",
            "delimiter",
            "sep",
            "str",
            "force_str",
        ],
        "clipboard" | "clip" => &[
            "delim",
            "delimiter",
            "sep",
            "str",
            "force_str",
            "encoding",
            "charset",
        ],
        "text" | "txt" => &["encoding", "charset"],
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
            "encoding",
            "charset",
            "path",
            "pointer",
            "include_source",
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
            ScriptStmt::Load(load) => {
                assert_eq!(load.name, "users");
                match &load.source {
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
            ScriptStmt::Load(LoadStmt {
                source: SourceSpec::Call { name, .. },
                ..
            }) => assert_eq!(name, "read_csv"),
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

    #[test]
    fn parse_set_and_for_each() {
        let stmts = parse_script(
            "SET region = 'east'; LOAD t FROM '${base}/${region}.csv' FOR region IN ('east', 'west'); LOAD u FROM EACH ('a.csv', 'b.csv'); LOAD p FROM 'x' FOR n IN 1..3 STEP 1",
        )
        .unwrap();
        assert!(matches!(stmts[0], ScriptStmt::Set { .. }));
        match &stmts[1] {
            ScriptStmt::Load(l) => {
                assert_eq!(l.fors.len(), 1);
                assert_eq!(l.fors[0].vars, vec!["region".to_string()]);
            }
            _ => panic!("load"),
        }
        match &stmts[2] {
            ScriptStmt::Load(l) => {
                assert!(matches!(l.each, Some(EachSpec::List(ref v)) if v.len() == 2));
            }
            _ => panic!("each"),
        }
        match &stmts[3] {
            ScriptStmt::Load(l) => match &l.fors[0].domain {
                ForDomain::Range { start, end, step } => {
                    assert_eq!((*start, *end, *step), (1, 3, 1));
                }
                _ => panic!("range"),
            },
            _ => panic!("load"),
        }
    }

    #[test]
    fn parse_for_date_range() {
        let stmts = parse_script(
            "LOAD t FROM 'https://x/${d}' FOR d IN DATE '2024-01-01'..'2024-01-03'; \
             LOAD u FROM 'https://x/${m}' FOR m IN DATE '2024-01'..'2024-12' STEP MONTH; \
             LOAD v FROM 'https://x/${d}' FOR d IN DATE 20240101..20240103 STEP 1",
        )
        .unwrap();
        match &stmts[0] {
            ScriptStmt::Load(l) => match &l.fors[0].domain {
                ForDomain::Dates { start, end, step } => {
                    assert_eq!(start, "2024-01-01");
                    assert_eq!(end, "2024-01-03");
                    assert_eq!(*step, ForDateStep::Default);
                }
                other => panic!("expected dates, got {other:?}"),
            },
            _ => panic!("load"),
        }
        match &stmts[1] {
            ScriptStmt::Load(l) => match &l.fors[0].domain {
                ForDomain::Dates { step, .. } => {
                    assert_eq!(*step, ForDateStep::Months(1));
                }
                other => panic!("expected dates, got {other:?}"),
            },
            _ => panic!("load"),
        }
        match &stmts[2] {
            ScriptStmt::Load(l) => match &l.fors[0].domain {
                ForDomain::Dates { start, end, step } => {
                    assert_eq!(start, "20240101");
                    assert_eq!(end, "20240103");
                    assert_eq!(*step, ForDateStep::Count(1));
                }
                other => panic!("expected compact dates, got {other:?}"),
            },
            _ => panic!("load"),
        }
    }

    #[test]
    fn date_range_requires_date_keyword() {
        let err =
            parse_script("LOAD t FROM 'x/${d}' FOR d IN '2024-01-01'..'2024-01-03'").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("DATE"), "{msg}");
        let stmts = parse_script("LOAD t FROM 'x/${n}' FOR n IN 20240101..20240103").unwrap();
        match &stmts[0] {
            ScriptStmt::Load(l) => match &l.fors[0].domain {
                ForDomain::Range { start, end, step } => {
                    assert_eq!((*start, *end, *step), (20240101, 20240103, 1));
                }
                other => panic!("compact without DATE must stay integer, got {other:?}"),
            },
            _ => panic!("load"),
        }
    }

    #[test]
    fn parse_for_tuple_and_multi() {
        let stmts = parse_script(
            "LOAD t FROM '${region}/${env}' FOR (region, env) IN (('east', 'prod'), ('west', 'stg')); \
             LOAD u FROM '${a}/${b}' FOR a IN ('x') FOR b IN ('y', 'z')",
        )
        .unwrap();
        match &stmts[0] {
            ScriptStmt::Load(l) => {
                assert_eq!(l.fors[0].vars, vec!["region", "env"]);
                match &l.fors[0].domain {
                    ForDomain::List(rows) => assert_eq!(rows.len(), 2),
                    _ => panic!("list"),
                }
            }
            _ => panic!("load"),
        }
        match &stmts[1] {
            ScriptStmt::Load(l) => assert_eq!(l.fors.len(), 2),
            _ => panic!("load"),
        }
    }
}
