use crate::args::{Args, Value};
use anyhow::{bail, Context, Result};
use std::collections::HashSet;

/// 清洗列名：空列补 `col_N`，重复列加后缀，去掉首尾空白。
pub fn unique_column_names<I, S>(raw: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (i, name) in raw.into_iter().enumerate() {
        let trimmed = name.as_ref().trim();
        let base = if trimmed.is_empty() {
            format!("col_{}", i)
        } else {
            trimmed.to_string()
        };
        let mut safe = base.clone();
        let mut n = 1;
        while seen.contains(&safe) {
            safe = format!("{}_{}", base, n);
            n += 1;
        }
        seen.insert(safe.clone());
        out.push(safe);
    }
    out
}

/// Excel / CSV 表头行为。默认第一行是列名；`header=false` 时第一行当数据。
///
/// `columns` 只改名字，**不会**自动变成无表头。无表头文件要自定义列名时必须同时写
/// `header=false, columns='id,name'`。
#[derive(Debug, Clone)]
pub struct HeaderSpec {
    pub has_header: bool,
    pub columns: Option<Vec<String>>,
}

impl Default for HeaderSpec {
    fn default() -> Self {
        Self {
            has_header: true,
            columns: None,
        }
    }
}

impl HeaderSpec {
    pub fn from_args(args: &Args) -> Result<Self> {
        Ok(Self {
            has_header: parse_has_header(args)?,
            columns: parse_columns(args)?,
        })
    }

    /// 按目标宽度生成列名：自定义 `columns` 优先，否则用文件表头，再否则 `col_N`。
    pub fn resolve(&self, width: usize, file_headers: Option<&[String]>) -> Vec<String> {
        if width == 0 {
            return Vec::new();
        }
        if let Some(cols) = &self.columns {
            pad_column_names(cols, width)
        } else if let Some(h) = file_headers {
            pad_column_names(h, width)
        } else {
            unique_column_names((0..width).map(|i| format!("col_{i}")))
        }
    }
}

fn parse_has_header(args: &Args) -> Result<bool> {
    match args.get(99, &["header", "has_header"]) {
        None => Ok(true),
        Some(v) if v.is_null() => Ok(true),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| anyhow::anyhow!("header 必须是 true/false/1/0，例如 header=false")),
    }
}

fn parse_columns(args: &Args) -> Result<Option<Vec<String>>> {
    match args.get(99, &["columns", "names", "colnames"]) {
        None => Ok(None),
        Some(v) if v.is_null() => Ok(None),
        Some(Value::Str(s)) => Ok(Some(parse_column_list(s)?)),
        Some(other) => {
            if let Some(s) = other.clone().into_string() {
                Ok(Some(parse_column_list(&s)?))
            } else {
                bail!("columns 必须是逗号分隔字符串或 JSON 数组，例如 columns='id,name'");
            }
        }
    }
}

fn parse_column_list(raw: &str) -> Result<Vec<String>> {
    let s = raw.trim();
    if s.is_empty() {
        bail!("columns 不能为空");
    }
    let names = if s.starts_with('[') {
        let val: serde_json::Value =
            serde_json::from_str(s).with_context(|| "columns JSON 数组解析失败")?;
        let arr = val
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("columns JSON 必须是数组，例如 [\"id\",\"name\"]"))?;
        arr.iter()
            .map(|v| match v {
                serde_json::Value::String(t) => t.trim().to_string(),
                serde_json::Value::Null => String::new(),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
    } else {
        s.split(',').map(|p| p.trim().to_string()).collect()
    };
    if names.iter().all(|n| n.is_empty()) {
        bail!("columns 不能为空");
    }
    Ok(names)
}

fn pad_column_names(names: &[String], width: usize) -> Vec<String> {
    let mut out: Vec<String> = names.iter().cloned().take(width).collect();
    while out.len() < width {
        out.push(String::new());
    }
    unique_column_names(out)
}

/// 有表头时保持「宽度 = 表头列数」（多出来的单元格丢掉，兼容旧行为）。
/// 无表头或提供了 `columns` 时，按数据最宽列 / 自定义列名取较大值。
pub fn table_width(
    header: &HeaderSpec,
    file_header_len: Option<usize>,
    data_width: usize,
) -> usize {
    let mut width = data_width;
    if let Some(n) = file_header_len {
        if header.has_header && header.columns.is_none() {
            return n;
        }
        width = width.max(n);
    }
    if let Some(cols) = &header.columns {
        width = width.max(cols.len());
    }
    width
}

pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::Args;
    use std::collections::HashMap;

    fn named(pairs: &[(&str, Value)]) -> Args {
        Args {
            positional: vec![],
            named: pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect::<HashMap<_, _>>(),
        }
    }

    #[test]
    fn header_defaults_true() {
        let spec = HeaderSpec::from_args(&named(&[])).unwrap();
        assert!(spec.has_header);
        assert!(spec.columns.is_none());
    }

    #[test]
    fn header_false_and_columns() {
        let spec = HeaderSpec::from_args(&named(&[
            ("header", Value::Bool(false)),
            ("columns", Value::Str("id, name, 金额".into())),
        ]))
        .unwrap();
        assert!(!spec.has_header);
        assert_eq!(
            spec.columns,
            Some(vec!["id".into(), "name".into(), "金额".into()])
        );
        let names = spec.resolve(3, None);
        assert_eq!(names, vec!["id", "name", "金额"]);
    }

    #[test]
    fn columns_json_array_and_padding() {
        let spec = HeaderSpec::from_args(&named(&[("columns", Value::Str(r#"["a","b"]"#.into()))]))
            .unwrap();
        let names = spec.resolve(4, Some(&["h0".into(), "h1".into(), "h2".into()]));
        assert_eq!(names, vec!["a", "b", "col_2", "col_3"]);
    }

    #[test]
    fn table_width_keeps_header_len_by_default() {
        let spec = HeaderSpec::default();
        assert_eq!(table_width(&spec, Some(2), 9), 2);
        let mut no_header = HeaderSpec::default();
        no_header.has_header = false;
        assert_eq!(table_width(&no_header, None, 3), 3);
    }
}
