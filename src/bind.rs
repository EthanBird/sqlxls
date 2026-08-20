use crate::args::{ArgExpr, ArgSlot, Value};
use crate::functions::Registry;
use crate::ingest::union_from_table;
use crate::rewrite::{eval_source, CallOut};
use crate::syntax::{EachSpec, ForClause, ForDomain, LoadStmt, SourceSpec, SyntaxOpts};
use anyhow::{bail, Result};
use rusqlite::Connection;
use std::collections::HashMap;

/// 脚本绑定：SET / --set / FOR 迭代变量。查找顺序：绑定表 → 环境变量。
#[derive(Clone, Debug, Default)]
pub struct BindCtx {
    vars: HashMap<String, Value>,
}

#[derive(Clone, Copy, Debug)]
pub enum Missing {
    /// 定位符里未定义则报错。
    Error,
    /// 选项字符串里未定义则留空（兼容 headers 的 `${TOKEN}`）。
    Empty,
}

impl BindCtx {
    pub fn set(&mut self, name: impl Into<String>, value: Value) {
        self.vars.insert(name.into(), value);
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.vars.get(name)
    }

    pub fn lookup_str(&self, name: &str) -> Option<String> {
        if let Some(v) = self.vars.get(name) {
            return v.clone().into_string().or_else(|| {
                if v.is_null() {
                    Some(String::new())
                } else {
                    None
                }
            });
        }
        std::env::var(name).ok()
    }

    pub fn interpolate(&self, template: &str, missing: Missing) -> Result<String> {
        interpolate_template(template, missing, |k| self.lookup_str(k))
    }

    pub fn interpolate_slots(&self, slots: &[ArgSlot]) -> Result<Vec<ArgSlot>> {
        interpolate_slot_list(slots, &|k| self.lookup_str(k))
    }
}

fn interpolate_slot_list(
    slots: &[ArgSlot],
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<ArgSlot>> {
    slots
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let missing = match slot {
                ArgSlot::Positional(ArgExpr::Literal(Value::Str(_))) if i == 0 => Missing::Error,
                _ => Missing::Empty,
            };
            match slot {
                ArgSlot::Positional(expr) => Ok(ArgSlot::Positional(interpolate_expr(
                    expr, lookup, missing,
                )?)),
                ArgSlot::Named(k, expr) => Ok(ArgSlot::Named(
                    k.clone(),
                    interpolate_expr(expr, lookup, missing)?,
                )),
            }
        })
        .collect()
}

fn interpolate_expr(
    expr: &ArgExpr,
    lookup: &dyn Fn(&str) -> Option<String>,
    missing: Missing,
) -> Result<ArgExpr> {
    match expr {
        ArgExpr::Literal(Value::Str(s)) => Ok(ArgExpr::Literal(Value::Str(interpolate_template(
            s, missing, lookup,
        )?))),
        ArgExpr::Literal(v) => Ok(ArgExpr::Literal(v.clone())),
        ArgExpr::Call { name, args } => Ok(ArgExpr::Call {
            name: name.clone(),
            args: interpolate_slot_list(args, lookup)?,
        }),
    }
}

pub fn interpolate_template(
    template: &str,
    missing: Missing,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(i) = rest.find("${") {
        out.push_str(&rest[..i]);
        rest = &rest[i + 2..];
        let Some(end) = rest.find('}') else {
            out.push_str("${");
            break;
        };
        let key = &rest[..end];
        rest = &rest[end + 1..];
        if key.is_empty() {
            bail!("空的占位符 ${{}}");
        }
        match lookup(key) {
            Some(v) => out.push_str(&v),
            None => match missing {
                Missing::Empty => {}
                Missing::Error => {
                    bail!("未定义的变量 `${{{key}}}`。请 SET {key}=...、--set {key}=...，或设置同名环境变量")
                }
            },
        }
    }
    out.push_str(rest);
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct ExpandPlan {
    pub source: SourceSpec,
    pub extras: Vec<(String, String)>,
    pub bind: BindCtx,
}

/// 把 LOAD 展开成一组物化计划。无 EACH/FOR 时仍返回单计划（无来源列）。
pub fn expand_load(load: &LoadStmt, bind: &BindCtx) -> Result<Vec<ExpandPlan>> {
    let bindings = expand_fors(&load.fors, bind)?;
    let mut plans = Vec::new();
    for row in bindings {
        let mut local = bind.clone();
        for (k, v) in &row {
            local.set(k.clone(), v.clone());
        }
        if let Some(each) = &load.each {
            let items = resolve_each(each, &local)?;
            if items.is_empty() {
                bail!("EACH 没有匹配到任何数据源");
            }
            for item in items {
                let locator = local.interpolate(&item, Missing::Error)?;
                let mut extras = extras_from_bindings(&row);
                push_extra(&mut extras, "_source", &locator);
                plans.push(ExpandPlan {
                    source: replace_locator(&load.source, locator),
                    extras,
                    bind: local.clone(),
                });
            }
        } else {
            let mut extras = extras_from_bindings(&row);
            let mut source = load.source.clone();
            if !load.fors.is_empty() {
                if let Some(uri) = source_locator(&source) {
                    let uri = local.interpolate(&uri, Missing::Error)?;
                    push_extra(&mut extras, "_source", &uri);
                    source = replace_locator(&source, uri);
                }
            }
            plans.push(ExpandPlan {
                source,
                extras,
                bind: local,
            });
        }
    }
    Ok(plans)
}

pub fn eval_load(
    load: &LoadStmt,
    dest: &str,
    conn: &mut Connection,
    registry: &Registry,
    counter: &mut usize,
    opts: &SyntaxOpts,
    bind: &BindCtx,
    explain: bool,
) -> Result<CallOut> {
    let expanding = load.each.is_some() || !load.fors.is_empty();
    if !expanding {
        return eval_source(
            &load.source,
            Some(dest.to_string()),
            conn,
            registry,
            counter,
            opts,
            bind,
        );
    }

    let plans = expand_load(load, bind)?;
    if plans.is_empty() {
        bail!("展开后没有任何数据源");
    }
    if explain {
        eprintln!("-- LOAD {} 展开为 {} 个源 --", dest, plans.len());
        for (i, p) in plans.iter().enumerate() {
            eprintln!(
                "   [{}] {} {:?}",
                i + 1,
                source_locator(&p.source).unwrap_or_default(),
                p.extras
            );
        }
    }

    let mut first = true;
    for plan in plans {
        let tmp = format!("excel_tmp_{}", *counter);
        *counter += 1;
        match eval_source(
            &plan.source,
            Some(tmp.clone()),
            conn,
            registry,
            counter,
            opts,
            &plan.bind,
        )? {
            CallOut::Table(src) => {
                union_from_table(conn, dest, &src, &plan.extras, !first)?;
                if src != dest {
                    conn.execute(&format!("DROP TABLE IF EXISTS {src}"), [])?;
                }
                first = false;
            }
            CallOut::Scalar(_) => {
                bail!("FOR/EACH 展开的源必须是表，不能是 read_text 这类标量");
            }
        }
    }
    Ok(CallOut::Table(dest.to_string()))
}

fn extras_from_bindings(row: &[(String, Value)]) -> Vec<(String, String)> {
    row.iter()
        .map(|(k, v)| {
            let col = if k.starts_with('_') {
                k.clone()
            } else {
                format!("_{k}")
            };
            let val = v.clone().into_string().unwrap_or_default();
            (col, val)
        })
        .collect()
}

fn push_extra(extras: &mut Vec<(String, String)>, key: &str, value: &str) {
    if extras.iter().any(|(k, _)| k == key) {
        return;
    }
    extras.push((key.to_string(), value.to_string()));
}

fn source_locator(source: &SourceSpec) -> Option<String> {
    match source {
        SourceSpec::Locator { uri, .. } => Some(uri.clone()),
        SourceSpec::Call { args, .. } => crate::syntax::locator_literal(args),
    }
}

fn replace_locator(source: &SourceSpec, uri: String) -> SourceSpec {
    match source {
        SourceSpec::Locator { options, .. } => SourceSpec::Locator {
            uri,
            options: options.clone(),
        },
        SourceSpec::Call { name, args } => {
            let mut args = args.clone();
            replace_call_locator(&mut args, uri);
            SourceSpec::Call {
                name: name.clone(),
                args,
            }
        }
    }
}

fn replace_call_locator(args: &mut [ArgSlot], uri: String) {
    for slot in args.iter_mut() {
        match slot {
            ArgSlot::Positional(ArgExpr::Literal(Value::Str(s))) => {
                *s = uri;
                return;
            }
            ArgSlot::Named(k, ArgExpr::Literal(Value::Str(s)))
                if matches!(k.as_str(), "path" | "file" | "url" | "locator") =>
            {
                *s = uri;
                return;
            }
            _ => {}
        }
    }
}

fn expand_fors(fors: &[ForClause], bind: &BindCtx) -> Result<Vec<Vec<(String, Value)>>> {
    fn rec(
        rest: &[ForClause],
        bind: &BindCtx,
        prefix: Vec<(String, Value)>,
    ) -> Result<Vec<Vec<(String, Value)>>> {
        if rest.is_empty() {
            return Ok(vec![prefix]);
        }
        let clause = &rest[0];
        let mut local = bind.clone();
        for (k, v) in &prefix {
            local.set(k.clone(), v.clone());
        }
        let values = eval_domain(&clause.domain, &local)?;
        if values.is_empty() {
            bail!("FOR {} 的取值集合为空", clause.vars.join(", "));
        }
        let mut out = Vec::new();
        for row_vals in values {
            if row_vals.len() != clause.vars.len() {
                bail!(
                    "FOR ({}) 需要 {} 个值，实际 {}",
                    clause.vars.join(", "),
                    clause.vars.len(),
                    row_vals.len()
                );
            }
            let mut row = prefix.clone();
            for (var, v) in clause.vars.iter().zip(row_vals) {
                row.push((var.clone(), v));
            }
            out.extend(rec(&rest[1..], bind, row)?);
        }
        Ok(out)
    }
    rec(fors, bind, Vec::new())
}

fn eval_domain(domain: &ForDomain, bind: &BindCtx) -> Result<Vec<Vec<Value>>> {
    match domain {
        ForDomain::List(rows) => {
            let mut out = Vec::new();
            for row in rows {
                let mut interpolated = Vec::with_capacity(row.len());
                for item in row {
                    if let Value::Str(s) = item {
                        interpolated.push(Value::Str(bind.interpolate(s, Missing::Error)?));
                    } else {
                        interpolated.push(item.clone());
                    }
                }
                out.push(interpolated);
            }
            Ok(out)
        }
        ForDomain::Range { start, end, step } => {
            if *step == 0 {
                bail!("FOR 范围的 STEP 不能为 0");
            }
            let mut n = *start;
            let mut out = Vec::new();
            if *step > 0 {
                while n <= *end {
                    out.push(vec![Value::Int(n)]);
                    n += *step;
                }
            } else {
                while n >= *end {
                    out.push(vec![Value::Int(n)]);
                    n += *step;
                }
            }
            Ok(out)
        }
        ForDomain::Dates { start, end, step } => {
            let start = bind.interpolate(start, Missing::Error)?;
            let end = bind.interpolate(end, Missing::Error)?;
            Ok(crate::dates::expand_range(&start, &end, *step)?
                .into_iter()
                .map(|s| vec![Value::Str(s)])
                .collect())
        }
        ForDomain::Glob(pat) => {
            let pat = bind.interpolate(pat, Missing::Error)?;
            Ok(glob_values(&pat)?.into_iter().map(|v| vec![v]).collect())
        }
    }
}

fn resolve_each(each: &EachSpec, bind: &BindCtx) -> Result<Vec<String>> {
    match each {
        EachSpec::List(items) => {
            let mut out = Vec::new();
            for item in items {
                out.push(bind.interpolate(item, Missing::Error)?);
            }
            Ok(out)
        }
        EachSpec::Glob(pat) => {
            let pat = bind.interpolate(pat, Missing::Error)?;
            glob_values(&pat)?
                .into_iter()
                .map(|v| {
                    v.into_string()
                        .ok_or_else(|| anyhow::anyhow!("GLOB 结果不是路径"))
                })
                .collect()
        }
    }
}

fn glob_values(pat: &str) -> Result<Vec<Value>> {
    let mut files: Vec<String> = glob::glob(pat)
        .map_err(|e| anyhow::anyhow!("通配符无效: {e}"))?
        .filter_map(|e| e.ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    files.sort();
    Ok(files.into_iter().map(Value::Str).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolate_bind_then_env() {
        let mut b = BindCtx::default();
        b.set("region", Value::Str("east".into()));
        assert_eq!(
            b.interpolate("https://x/${region}/o", Missing::Error)
                .unwrap(),
            "https://x/east/o"
        );
    }

    #[test]
    fn interpolate_missing_errors() {
        let b = BindCtx::default();
        let err = b.interpolate("${nope}", Missing::Error).unwrap_err();
        assert!(format!("{err}").contains("nope"), "{err}");
    }

    #[test]
    fn range_inclusive() {
        let d = ForDomain::Range {
            start: 1,
            end: 3,
            step: 1,
        };
        let v = eval_domain(&d, &BindCtx::default()).unwrap();
        assert_eq!(
            v,
            vec![
                vec![Value::Int(1)],
                vec![Value::Int(2)],
                vec![Value::Int(3)]
            ]
        );
    }

    #[test]
    fn date_range_expands_iso_strings() {
        let d = ForDomain::Dates {
            start: "2024-01-01".into(),
            end: "2024-01-03".into(),
            step: crate::syntax::ForDateStep::Default,
        };
        let v = eval_domain(&d, &BindCtx::default()).unwrap();
        assert_eq!(
            v,
            vec![
                vec![Value::Str("2024-01-01".into())],
                vec![Value::Str("2024-01-02".into())],
                vec![Value::Str("2024-01-03".into())],
            ]
        );
    }

    #[test]
    fn date_range_interpolates_bounds() {
        let mut b = BindCtx::default();
        b.set("start", Value::Str("2024-01".into()));
        b.set("end", Value::Str("2024-02".into()));
        let d = ForDomain::Dates {
            start: "${start}".into(),
            end: "${end}".into(),
            step: crate::syntax::ForDateStep::Default,
        };
        let v = eval_domain(&d, &b).unwrap();
        assert_eq!(
            v,
            vec![
                vec![Value::Str("2024-01".into())],
                vec![Value::Str("2024-02".into())],
            ]
        );
    }
}
