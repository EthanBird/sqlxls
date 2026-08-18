use crate::engine::{handle_output, handle_text_output};
use crate::functions::Registry;
use crate::rewrite::{
    eval_standalone, is_query, parse_standalone_call, rewrite_sql, split_statements, CallOut,
};
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

pub struct Session {
    conn: Connection,
    registry: Registry,
    table_counter: usize,
}

impl Session {
    pub fn new() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "synchronous", "OFF")?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        conn.pragma_update(None, "cache_size", "-64000")?;
        conn.pragma_update(None, "journal_mode", "MEMORY")?;
        Ok(Self {
            conn,
            registry: Registry::builtin(),
            table_counter: 0,
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn run_sql(&mut self, sql: &str, output: Option<&PathBuf>, explain: bool) -> Result<()> {
        if let Some((name, args_inner)) = parse_standalone_call(sql, &self.registry) {
            return self.emit_call(&name, &args_inner, output, explain);
        }

        let rewritten = rewrite_sql(sql, &mut self.conn, &self.registry, &mut self.table_counter)?;
        if explain {
            eprintln!("-- rewritten SQL --\n{}\n-------------------", rewritten);
        }
        let stmts = split_statements(&rewritten)?;
        let (head, last) = stmts.split_at(stmts.len() - 1);
        for s in head {
            self.conn
                .execute_batch(s)
                .with_context(|| format!("执行语句失败:\n{}", s))?;
        }
        let last = last[0].as_str();
        if is_query(last) {
            handle_output(&self.conn, last, output)?;
        } else {
            self.conn
                .execute_batch(last)
                .with_context(|| format!("执行语句失败:\n{}", last))?;
            println!("✅ 语句已执行。");
        }
        Ok(())
    }

    pub fn run_func(&mut self, call: &str, output: Option<&PathBuf>, explain: bool) -> Result<()> {
        if let Some((name, args_inner)) = parse_standalone_call(call, &self.registry) {
            self.emit_call(&name, &args_inner, output, explain)
        } else {
            anyhow::bail!("未找到匹配的内置函数或参数格式错误: {}", call);
        }
    }

    fn emit_call(
        &mut self,
        name: &str,
        args_inner: &str,
        output: Option<&PathBuf>,
        explain: bool,
    ) -> Result<()> {
        if explain {
            eprintln!("-- function {}({}) --", name, args_inner);
        }
        match eval_standalone(
            name,
            args_inner,
            &mut self.conn,
            &self.registry,
            &mut self.table_counter,
        )? {
            CallOut::Table(t) => {
                let sql = format!("SELECT * FROM {}", t);
                handle_output(&self.conn, &sql, output)
            }
            CallOut::Scalar(txt) => handle_text_output(&txt, output),
        }
    }
}

pub fn load_sql_input(input: &str) -> Result<String> {
    if Path::new(input).is_file() {
        std::fs::read_to_string(input).with_context(|| format!("无法读取 SQL 文件: {}", input))
    } else {
        Ok(input.to_string())
    }
}
