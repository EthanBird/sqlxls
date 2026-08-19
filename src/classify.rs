use anyhow::{bail, Result};
use sqlparser::ast::{visit_expressions, visit_relations, Expr, ObjectName};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use std::ops::ControlFlow;

pub const STUB_PREFIX: &str = "__sqlxls_src_";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourcePlace {
    Unknown,
    Table,
    Expr,
}

pub fn stub_name(index: usize) -> String {
    format!("{}{}", STUB_PREFIX, index)
}

pub fn stub_index(name: &str) -> Option<usize> {
    name.strip_prefix(STUB_PREFIX)?.parse().ok()
}

/// 把已注册源调用换成占位符后，用 SQL 解析器判断它出现在表位置还是表达式位置。
pub fn classify_source_places(stubbed_sql: &str, n: usize) -> Result<Vec<SourcePlace>> {
    let dialect = GenericDialect {};
    let ast = Parser::parse_sql(&dialect, stubbed_sql).map_err(|e| {
        anyhow::anyhow!("无法把查询解析为标准 SQL（请把数据源写成 LOAD 或 FROM read(...)）: {e}")
    })?;

    let mut places = vec![SourcePlace::Unknown; n];

    for stmt in &ast {
        let _ = visit_relations(stmt, |name: &ObjectName| {
            if let Some(i) = object_stub(name) {
                places[i] = SourcePlace::Table;
            }
            ControlFlow::<()>::Continue(())
        });
        let _ = visit_expressions(stmt, |expr: &Expr| {
            if let Expr::Identifier(ident) = expr {
                if let Some(i) = stub_index(&ident.value) {
                    if places[i] == SourcePlace::Unknown {
                        places[i] = SourcePlace::Expr;
                    }
                }
            }
            ControlFlow::<()>::Continue(())
        });
    }

    Ok(places)
}

fn object_stub(name: &ObjectName) -> Option<usize> {
    let ident = name.0.last()?;
    stub_index(&ident.value)
}

pub fn require_classified(places: &[SourcePlace], opts_strict_parse: bool) -> Result<()> {
    if !opts_strict_parse {
        return Ok(());
    }
    if places.contains(&SourcePlace::Unknown) {
        bail!("syntax strict：无法确认表函数出现在 FROM/JOIN，请改用 LOAD 或检查 SQL 是否可被标准解析器解析");
    }
    Ok(())
}
