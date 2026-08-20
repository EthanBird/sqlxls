use crate::args::Args;
use crate::functions::{ExecCtx, FuncOutput, TableFunction};
use crate::ingest::{ingest_rows, Cell, IngestOpts};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;

pub struct ReadJsonExt;

impl TableFunction for ReadJsonExt {
    fn names(&self) -> &'static [&'static str] {
        &["read_json", "readjson"]
    }

    fn execute(&self, ctx: &mut ExecCtx, args: &Args) -> Result<FuncOutput> {
        let path = args.require_str(0, &["file"], "JSON 路径")?;
        let json_path = args
            .get_str(1, &["json_path", "pointer"])
            .or_else(|| {
                if !args.positional.is_empty() {
                    args.named.get("path").and_then(|v| v.clone().into_string())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let content =
            fs::read_to_string(&path).with_context(|| format!("无法读取 JSON 文件: {}", path))?;
        let value: Value = serde_json::from_str(&content).context("JSON 格式不合法")?;
        json_to_table(ctx.conn, &ctx.dest_table, &value, &json_path, false)?;
        Ok(FuncOutput::Table)
    }
}

pub fn json_to_table(
    conn: &mut rusqlite::Connection,
    table: &str,
    value: &Value,
    json_path: &str,
    append: bool,
) -> Result<usize> {
    let extracted = extract_json_path(value, json_path)?;
    let table_val = if json_path.trim().is_empty() {
        default_table_value(extracted)
    } else {
        extracted
    };
    let (headers, rows) = value_to_rows(table_val)?;
    ingest_rows(
        conn,
        table,
        &headers,
        rows,
        IngestOpts {
            force_str: false,
            append,
        },
    )
}

pub fn extract_json_path<'a>(value: &'a Value, path: &str) -> Result<&'a Value> {
    let path = path.trim().trim_start_matches('$').trim_start_matches('.');
    if path.is_empty() {
        return Ok(value);
    }
    let mut cur = value;
    for seg in path.split('.') {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        cur = cur
            .get(seg)
            .with_context(|| format!("JSON 路径不存在: {}", seg))?;
    }
    Ok(cur)
}

/// 未指定 path 时：根数组，或常见包装字段，或对象里第一个数组，否则把对象当一行。
pub fn default_table_value(value: &Value) -> &Value {
    match value {
        Value::Array(_) => value,
        Value::Object(obj) => {
            for k in ["data", "items", "results", "records", "rows"] {
                if let Some(v @ Value::Array(_)) = obj.get(k) {
                    return v;
                }
            }
            if let Some(v) = obj.values().find(|v| v.is_array()) {
                return v;
            }
            value
        }
        _ => value,
    }
}

pub fn value_to_rows(value: &Value) -> Result<(Vec<String>, Vec<Vec<Cell>>)> {
    match value {
        Value::Array(arr) => array_to_rows(arr),
        Value::Object(obj) => {
            let headers: Vec<String> = obj.keys().cloned().collect();
            if headers.is_empty() {
                bail!("JSON 对象没有任何字段");
            }
            let row = headers
                .iter()
                .map(|h| json_atom(obj.get(h).unwrap_or(&Value::Null)))
                .collect();
            Ok((headers, vec![row]))
        }
        other => Ok((vec!["value".into()], vec![vec![json_atom(other)]])),
    }
}

fn array_to_rows(arr: &[Value]) -> Result<(Vec<String>, Vec<Vec<Cell>>)> {
    if arr.is_empty() {
        return Ok((vec!["value".into()], vec![]));
    }
    let has_object = arr.iter().any(|v| v.is_object());
    if has_object {
        let headers = union_object_keys(arr);
        if headers.is_empty() {
            bail!("JSON 数组里的对象没有任何字段");
        }
        let mut rows = Vec::with_capacity(arr.len());
        for item in arr {
            if let Some(o) = item.as_object() {
                rows.push(
                    headers
                        .iter()
                        .map(|h| json_atom(o.get(h).unwrap_or(&Value::Null)))
                        .collect(),
                );
            } else {
                rows.push(vec![Cell::Null; headers.len()]);
            }
        }
        Ok((headers, rows))
    } else {
        let rows = arr.iter().map(|v| vec![json_atom(v)]).collect();
        Ok((vec!["value".into()], rows))
    }
}

fn union_object_keys(arr: &[Value]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut keys = Vec::new();
    for item in arr.iter().take(200) {
        if let Some(obj) = item.as_object() {
            for k in obj.keys() {
                if seen.insert(k.clone()) {
                    keys.push(k.clone());
                }
            }
        }
    }
    if arr.len() > 200 {
        for item in &arr[200..] {
            if let Some(obj) = item.as_object() {
                for k in obj.keys() {
                    if seen.insert(k.clone()) {
                        keys.push(k.clone());
                    }
                }
            }
        }
    }
    keys
}

fn json_atom(v: &Value) -> Cell {
    match v {
        Value::Null => Cell::Null,
        Value::Bool(b) => Cell::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Cell::Int(i)
            } else if let Some(f) = n.as_f64() {
                Cell::Real(f)
            } else {
                Cell::Text(n.to_string())
            }
        }
        Value::String(s) => Cell::Text(s.clone()),
        Value::Array(_) | Value::Object(_) => Cell::Text(v.to_string()),
    }
}
