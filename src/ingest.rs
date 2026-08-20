use crate::args::sql_quote;
use crate::schema::{quote_ident, unique_column_names};
use anyhow::{bail, Context, Result};
use rusqlite::{params_from_iter, types::Value as SqlValue, Connection};

/// 连接器交给 Ingest 的单元格。
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
}

impl From<Cell> for SqlValue {
    fn from(c: Cell) -> Self {
        match c {
            Cell::Null => SqlValue::Null,
            Cell::Int(i) => SqlValue::Integer(i),
            Cell::Real(f) => SqlValue::Real(f),
            Cell::Text(s) => SqlValue::Text(s),
            Cell::Bool(b) => SqlValue::Integer(if b { 1 } else { 0 }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColType {
    Int,
    Real,
    Text,
}

#[derive(Clone, Debug, Default)]
pub struct IngestOpts {
    pub force_str: bool,
    /// 表已存在时按列名追加（新列 ALTER，缺列填 NULL）。
    pub append: bool,
}

pub fn ingest_rows(
    conn: &mut Connection,
    table_name: &str,
    headers: &[String],
    rows: impl IntoIterator<Item = Vec<Cell>>,
    opts: IngestOpts,
) -> Result<usize> {
    let rows: Vec<Vec<Cell>> = rows.into_iter().collect();
    ingest_collected(conn, table_name, headers, rows, opts)
}

fn ingest_collected(
    conn: &mut Connection,
    table_name: &str,
    headers: &[String],
    mut rows: Vec<Vec<Cell>>,
    opts: IngestOpts,
) -> Result<usize> {
    if headers.is_empty() {
        bail!("没有可用的列名，无法建表");
    }
    let headers = unique_column_names(headers.iter().map(|s| s.as_str()));

    if opts.force_str {
        for row in &mut rows {
            for cell in row.iter_mut() {
                *cell = match std::mem::replace(cell, Cell::Null) {
                    Cell::Null => Cell::Null,
                    Cell::Int(i) => Cell::Text(i.to_string()),
                    Cell::Real(f) => Cell::Text(f.to_string()),
                    Cell::Text(s) => Cell::Text(s),
                    Cell::Bool(b) => Cell::Text(b.to_string()),
                };
            }
        }
    }

    let exists = table_exists(conn, table_name)?;
    if exists && !opts.append {
        conn.execute(&format!("DROP TABLE {}", table_name), [])?;
    }

    let exists = table_exists(conn, table_name)?;
    if !exists {
        let types = infer_types(&headers, &rows, opts.force_str);
        let mut ddl = format!("CREATE TABLE {} (", table_name);
        for (i, (name, ty)) in headers.iter().zip(types.iter()).enumerate() {
            if i > 0 {
                ddl.push_str(", ");
            }
            ddl.push_str(&format!("{} {}", quote_ident(name), col_sql_type(*ty)));
        }
        ddl.push(')');
        conn.execute(&ddl, [])
            .with_context(|| format!("建表失败: {}", ddl))?;
    } else {
        let existing = table_columns(conn, table_name)?;
        for name in &headers {
            if !existing.iter().any(|e| e == name) {
                conn.execute(
                    &format!(
                        "ALTER TABLE {} ADD COLUMN {} TEXT",
                        table_name,
                        quote_ident(name)
                    ),
                    [],
                )?;
            }
        }
    }

    let dest_cols = table_columns(conn, table_name)?;
    let index_map: Vec<Option<usize>> = dest_cols
        .iter()
        .map(|d| headers.iter().position(|h| h == d))
        .collect();

    let placeholders = vec!["?"; dest_cols.len()].join(", ");
    let insert_sql = format!("INSERT INTO {} VALUES ({})", table_name, placeholders);

    let tx = conn.transaction()?;
    let count;
    {
        let mut stmt = tx.prepare(&insert_sql)?;
        let mut params: Vec<SqlValue> = Vec::with_capacity(dest_cols.len());
        count = rows.len();
        for row in rows {
            params.clear();
            for src_idx in &index_map {
                let cell = match src_idx {
                    Some(i) => row.get(*i).cloned().unwrap_or(Cell::Null),
                    None => Cell::Null,
                };
                params.push(cell.into());
            }
            stmt.execute(params_from_iter(params.iter()))
                .with_context(|| "插入行失败")?;
        }
    }
    tx.commit()?;
    Ok(count)
}

fn infer_types(headers: &[String], rows: &[Vec<Cell>], force_str: bool) -> Vec<ColType> {
    if force_str {
        return vec![ColType::Text; headers.len()];
    }
    let mut types = vec![None; headers.len()];
    for row in rows.iter().take(200) {
        for (i, cell) in row.iter().enumerate().take(headers.len()) {
            types[i] = Some(widen(types[i], cell));
        }
    }
    types
        .into_iter()
        .map(|t| t.unwrap_or(ColType::Text))
        .collect()
}

fn widen(cur: Option<ColType>, cell: &Cell) -> ColType {
    let seen = match cell {
        Cell::Null => return cur.unwrap_or(ColType::Int),
        Cell::Int(_) | Cell::Bool(_) => ColType::Int,
        Cell::Real(_) => ColType::Real,
        Cell::Text(_) => ColType::Text,
    };
    match (cur, seen) {
        (None, s) => s,
        (Some(ColType::Text), _) | (_, ColType::Text) => ColType::Text,
        (Some(ColType::Real), _) | (_, ColType::Real) => ColType::Real,
        (Some(ColType::Int), ColType::Int) => ColType::Int,
    }
}

fn col_sql_type(ty: ColType) -> &'static str {
    match ty {
        ColType::Int => "INTEGER",
        ColType::Real => "REAL",
        ColType::Text => "TEXT",
    }
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

fn table_columns(conn: &Connection, name: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", name))?;
    let cols = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(cols)
}

/// 在每一行末尾追加常量列（来源路径、sheet、页码等）。
pub fn attach_const_column(
    headers: &mut Vec<String>,
    rows: &mut [Vec<Cell>],
    name: &str,
    value: Cell,
) {
    headers.push(name.to_string());
    for row in rows {
        row.push(value.clone());
    }
}

/// 把 `src` 表按列名合并进 `dest`，并附上来源维度列。
pub fn union_from_table(
    conn: &mut Connection,
    dest: &str,
    src: &str,
    extras: &[(String, String)],
    append: bool,
) -> Result<()> {
    let src_cols = table_columns(conn, src)?;
    if src_cols.is_empty() && extras.is_empty() {
        bail!("无法合并空表 `{src}`");
    }

    if !append || !table_exists(conn, dest)? {
        if table_exists(conn, dest)? {
            conn.execute(&format!("DROP TABLE {}", quote_ident(dest)), [])?;
        }
        let mut select = Vec::new();
        for c in &src_cols {
            select.push(format!("s.{}", quote_ident(c)));
        }
        for (n, v) in extras {
            select.push(format!("{} AS {}", sql_quote(v), quote_ident(n)));
        }
        let sql = format!(
            "CREATE TABLE {} AS SELECT {} FROM {} AS s",
            quote_ident(dest),
            select.join(", "),
            quote_ident(src)
        );
        conn.execute(&sql, [])
            .with_context(|| format!("合并建表失败: {sql}"))?;
        return Ok(());
    }

    let mut dest_cols = table_columns(conn, dest)?;
    for name in src_cols.iter().chain(extras.iter().map(|(n, _)| n)) {
        if !dest_cols.iter().any(|d| d == name) {
            conn.execute(
                &format!(
                    "ALTER TABLE {} ADD COLUMN {} TEXT",
                    quote_ident(dest),
                    quote_ident(name)
                ),
                [],
            )?;
            dest_cols.push(name.clone());
        }
    }

    let mut select_exprs = Vec::new();
    for d in &dest_cols {
        if let Some((_, v)) = extras.iter().find(|(n, _)| n == d) {
            select_exprs.push(sql_quote(v));
        } else if src_cols.iter().any(|s| s == d) {
            select_exprs.push(format!("s.{}", quote_ident(d)));
        } else {
            select_exprs.push("NULL".into());
        }
    }
    let sql = format!(
        "INSERT INTO {} SELECT {} FROM {} AS s",
        quote_ident(dest),
        select_exprs.join(", "),
        quote_ident(src)
    );
    conn.execute(&sql, [])
        .with_context(|| format!("合并插入失败: {sql}"))?;
    Ok(())
}

pub fn include_source(args: &crate::args::Args) -> bool {
    if args.named.contains_key("include_source") {
        args.get_bool(99, &["include_source"])
    } else {
        true
    }
}
