use anyhow::{bail, Result};
use std::collections::HashMap;

/// 表函数参数值。嵌套函数求值后都会落成这里的字面量。
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn into_string(self) -> Option<String> {
        match self {
            Value::Str(s) => Some(s),
            Value::Int(i) => Some(i.to_string()),
            Value::Float(f) => Some(f.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            Value::Null => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Int(i) => Some(*i != 0),
            Value::Str(s) => {
                let t = s.trim();
                if t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("str") || t == "1" {
                    Some(true)
                } else if t.eq_ignore_ascii_case("false") || t == "0" || t.is_empty() {
                    Some(false)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn as_usize(&self) -> Option<usize> {
        match self {
            Value::Int(i) if *i >= 0 => Some(*i as usize),
            Value::Float(f) if *f >= 0.0 && f.fract() == 0.0 => Some(*f as usize),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
            || matches!(self, Value::Str(s) if s.eq_ignore_ascii_case("null"))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Args {
    pub positional: Vec<Value>,
    pub named: HashMap<String, Value>,
}

impl Args {
    pub fn get(&self, index: usize, names: &[&str]) -> Option<&Value> {
        for n in names {
            if let Some(v) = self.named.get(*n) {
                return Some(v);
            }
        }
        self.positional.get(index)
    }

    pub fn get_str(&self, index: usize, names: &[&str]) -> Option<String> {
        self.get(index, names).and_then(|v| v.clone().into_string())
    }

    pub fn require_str(&self, index: usize, names: &[&str], hint: &str) -> Result<String> {
        self.get_str(index, names)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("缺少参数: {}", hint))
    }

    pub fn get_bool(&self, index: usize, names: &[&str]) -> bool {
        self.get(index, names)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    pub fn get_usize(&self, index: usize, names: &[&str], default: usize) -> usize {
        self.get(index, names)
            .and_then(|v| v.as_usize())
            .unwrap_or(default)
    }
}

/// 把字符串转成 SQL 字面量（单引号，内部 `'` → `''`）。
pub fn sql_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

#[derive(Debug, Clone)]
pub enum ArgExpr {
    Literal(Value),
    Call { name: String, args: Vec<ArgSlot> },
}

#[derive(Debug, Clone)]
pub enum ArgSlot {
    Positional(ArgExpr),
    Named(String, ArgExpr),
}

pub fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

pub fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

pub fn skip_ws(s: &str, mut i: usize) -> usize {
    let bytes = s.as_bytes();
    while i < bytes.len() {
        let c = bytes[i];
        if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
            i += 1;
        } else {
            break;
        }
    }
    i
}

/// 跳过 SQL 字符串 / 注释。返回新的字节下标；若 `i` 不在这些结构上则原样返回。
pub fn skip_sql_noise(s: &str, i: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if i >= bytes.len() {
        return None;
    }
    match bytes[i] {
        b'\'' => Some(skip_quoted(s, i, '\'')?),
        b'"' => Some(skip_quoted(s, i, '"')?),
        b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
            let rest = &s[i..];
            Some(i + rest.find('\n').map(|n| n + 1).unwrap_or(rest.len()))
        }
        b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
            let rest = &s[i + 2..];
            let end = rest.find("*/")?;
            Some(i + 2 + end + 2)
        }
        _ => None,
    }
}

fn skip_quoted(s: &str, start: usize, quote: char) -> Option<usize> {
    let mut chars = s[start..].char_indices();
    chars.next()?; // opening quote
    while let Some((off, ch)) = chars.next() {
        if ch == quote {
            // SQL 转义：'' 或 ""
            let abs = start + off;
            let next = s[abs + quote.len_utf8()..].chars().next();
            if next == Some(quote) {
                chars.next();
                continue;
            }
            return Some(abs + quote.len_utf8());
        }
    }
    None
}

pub fn matching_paren(s: &str, open: usize) -> Result<usize> {
    if open >= s.len() || s.as_bytes()[open] != b'(' {
        bail!("内部错误: matching_paren 未落在 '(' 上");
    }
    let mut depth = 0;
    let mut i = open;
    while i < s.len() {
        if let Some(n) = skip_sql_noise(s, i) {
            i = n;
            continue;
        }
        let c = s.as_bytes()[i];
        match c {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    bail!("括号未闭合");
}

/// 读取成对的 `{}` 或 `[]`（JSON），字符串内的括号忽略。
pub fn matching_bracket(s: &str, open: usize) -> Result<usize> {
    let bytes = s.as_bytes();
    if open >= bytes.len() {
        bail!("括号未闭合");
    }
    let open_ch = bytes[open];
    let close_ch = match open_ch {
        b'{' => b'}',
        b'[' => b']',
        _ => bail!("内部错误: matching_bracket"),
    };
    let mut depth = 0;
    let mut i = open;
    while i < s.len() {
        let c = bytes[i];
        if c == b'\'' || c == b'"' {
            i = skip_quoted(s, i, c as char).ok_or_else(|| anyhow::anyhow!("JSON 字符串未闭合"))?;
            continue;
        }
        if c == open_ch {
            depth += 1;
        } else if c == close_ch {
            depth -= 1;
            if depth == 0 {
                return Ok(i);
            }
        }
        i += 1;
    }
    bail!("JSON 括号未闭合")
}

pub fn parse_ident(s: &str, i: usize) -> Option<(String, usize)> {
    let rest = &s[i..];
    let mut chars = rest.char_indices();
    let (_, first) = chars.next()?;
    if !is_ident_start(first) {
        return None;
    }
    let mut end = first.len_utf8();
    for (off, ch) in chars {
        if is_ident_continue(ch) {
            end = off + ch.len_utf8();
        } else {
            break;
        }
    }
    Some((rest[..end].to_string(), i + end))
}

pub fn parse_string_literal(s: &str, start: usize) -> Result<(String, usize)> {
    let quote = s[start..].chars().next().unwrap();
    if quote != '\'' && quote != '"' {
        bail!("不是字符串");
    }
    let end = skip_quoted(s, start, quote).ok_or_else(|| anyhow::anyhow!("字符串未闭合"))?;
    let inner = &s[start + quote.len_utf8()..end - quote.len_utf8()];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == quote && chars.peek() == Some(&quote) {
            out.push(quote);
            chars.next();
        } else {
            out.push(ch);
        }
    }
    Ok((out, end))
}

fn parse_number(s: &str, start: usize) -> Option<(Value, usize)> {
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
    let mut is_float = false;
    if i < bytes.len() && bytes[i] == b'.' {
        is_float = true;
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    let token = &rest[..i];
    let value = if is_float {
        Value::Float(token.parse().ok()?)
    } else {
        Value::Int(token.parse().ok()?)
    };
    Some((value, start + i))
}

pub fn parse_arg_expr(
    s: &str,
    start: usize,
    is_func: &dyn Fn(&str) -> bool,
) -> Result<(ArgExpr, usize)> {
    let i = skip_ws(s, start);
    if i >= s.len() {
        bail!("参数列表不完整");
    }
    let bytes = s.as_bytes();

    // 嵌套表函数
    if let Some((name, after_name)) = parse_ident(s, i) {
        let j = skip_ws(s, after_name);
        if j < s.len() && bytes[j] == b'(' && is_func(&name) {
            let close = matching_paren(s, j)?;
            let inner = &s[j + 1..close];
            let args = parse_arg_list(inner, is_func)?;
            return Ok((ArgExpr::Call { name, args }, close + 1));
        }
        let lower = name.to_ascii_lowercase();
        if lower == "null" {
            return Ok((ArgExpr::Literal(Value::Null), after_name));
        }
        if lower == "true" {
            return Ok((ArgExpr::Literal(Value::Bool(true)), after_name));
        }
        if lower == "false" {
            return Ok((ArgExpr::Literal(Value::Bool(false)), after_name));
        }
        bail!("无法解析的标识符 `{}`（字符串请加引号）", name);
    }

    match bytes[i] {
        b'\'' | b'"' => {
            let (sval, end) = parse_string_literal(s, i)?;
            Ok((ArgExpr::Literal(Value::Str(sval)), end))
        }
        b'{' | b'[' => {
            let close = matching_bracket(s, i)?;
            let raw = s[i..=close].to_string();
            Ok((ArgExpr::Literal(Value::Str(raw)), close + 1))
        }
        b'-' | b'+' | b'0'..=b'9' => {
            let (v, end) = parse_number(s, i).ok_or_else(|| anyhow::anyhow!("数字解析失败"))?;
            Ok((ArgExpr::Literal(v), end))
        }
        _ => bail!(
            "无法解析参数，从 `{}` 开始",
            s[i..].chars().take(20).collect::<String>()
        ),
    }
}

pub fn parse_arg_list(s: &str, is_func: &dyn Fn(&str) -> bool) -> Result<Vec<ArgSlot>> {
    let mut i = skip_ws(s, 0);
    let mut out = Vec::new();
    if i >= s.len() {
        return Ok(out);
    }
    loop {
        i = skip_ws(s, i);
        if i >= s.len() {
            break;
        }
        // named: ident =
        let mut named: Option<String> = None;
        if let Some((ident, after)) = parse_ident(s, i) {
            let j = skip_ws(s, after);
            if j < s.len() && s.as_bytes()[j] == b'=' {
                named = Some(ident);
                i = skip_ws(s, j + 1);
            }
        }
        let (expr, end) = parse_arg_expr(s, i, is_func)?;
        if let Some(n) = named {
            out.push(ArgSlot::Named(n.to_ascii_lowercase(), expr));
        } else {
            out.push(ArgSlot::Positional(expr));
        }
        i = skip_ws(s, end);
        if i >= s.len() {
            break;
        }
        if s.as_bytes()[i] == b',' {
            i += 1;
            continue;
        }
        bail!(
            "参数列表在 `{}` 处期望逗号",
            s[i..].chars().take(12).collect::<String>()
        );
    }
    Ok(out)
}

pub fn parse_call_head(
    s: &str,
    start: usize,
    is_func: &dyn Fn(&str) -> bool,
) -> Option<(String, usize, usize)> {
    let i = skip_ws(s, start);
    let (name, after) = parse_ident(s, i)?;
    if !is_func(&name) {
        return None;
    }
    let j = skip_ws(s, after);
    if j >= s.len() || s.as_bytes()[j] != b'(' {
        return None;
    }
    let close = matching_paren(s, j).ok()?;
    Some((name, j, close))
}
