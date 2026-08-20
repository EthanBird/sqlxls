use crate::bind::{eval_load, BindCtx};
use crate::engine::{handle_output, handle_text_output};
use crate::functions::Registry;
use crate::rewrite::{eval_standalone, is_query, parse_standalone_call, rewrite_sql, CallOut};
use crate::syntax::{parse_script, ScriptStmt, SyntaxOpts};
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

pub struct Session {
    conn: Connection,
    registry: Registry,
    table_counter: usize,
    opts: SyntaxOpts,
    bind: BindCtx,
}

impl Session {
    pub fn new() -> Result<Self> {
        Self::with_opts(SyntaxOpts::default())
    }

    pub fn with_opts(opts: SyntaxOpts) -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "synchronous", "OFF")?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        conn.pragma_update(None, "cache_size", "-64000")?;
        conn.pragma_update(None, "journal_mode", "MEMORY")?;
        Ok(Self {
            conn,
            registry: Registry::builtin(),
            table_counter: 0,
            opts,
            bind: BindCtx::default(),
        })
    }

    pub fn set_var(&mut self, name: impl Into<String>, value: crate::args::Value) {
        self.bind.set(name, value);
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn run_sql(&mut self, sql: &str, output: Option<&PathBuf>, explain: bool) -> Result<()> {
        if let Some((name, args_inner)) = parse_standalone_call(sql, &self.registry) {
            return self.emit_call(&name, &args_inner, output, explain);
        }

        let stmts = parse_script(sql)?;
        let last_idx = stmts.len() - 1;
        for (i, stmt) in stmts.into_iter().enumerate() {
            let is_last = i == last_idx;
            match stmt {
                ScriptStmt::Set { name, value } => {
                    let rendered = value.clone().into_string().unwrap_or_default();
                    if explain {
                        eprintln!("-- SET {} = {} --", name, rendered);
                    }
                    self.bind.set(name.clone(), value);
                    if is_last {
                        println!("✅ SET {name} = {rendered}");
                    }
                }
                ScriptStmt::Load(load) => {
                    if explain {
                        eprintln!("-- LOAD {} --", load.name);
                    }
                    match eval_load(
                        &load,
                        &load.name,
                        &mut self.conn,
                        &self.registry,
                        &mut self.table_counter,
                        &self.opts,
                        &self.bind,
                        explain,
                    )? {
                        CallOut::Table(t) => {
                            if is_last {
                                let q = format!("SELECT * FROM {}", t);
                                handle_output(&self.conn, &q, output)?;
                            }
                        }
                        CallOut::Scalar(txt) => {
                            if is_last {
                                handle_text_output(&txt, output)?;
                            } else {
                                anyhow::bail!(
                                    "LOAD {} 得到的是文本而不是表，不能继续后续语句",
                                    load.name
                                );
                            }
                        }
                    }
                }
                ScriptStmt::Query(q) => {
                    let rewritten = rewrite_sql(
                        &q,
                        &mut self.conn,
                        &self.registry,
                        &mut self.table_counter,
                        &self.opts,
                        &self.bind,
                    )?;
                    if explain {
                        eprintln!("-- rewritten SQL --\n{}\n-------------------", rewritten);
                    }
                    if is_last && is_query(&rewritten) {
                        handle_output(&self.conn, &rewritten, output)?;
                    } else {
                        self.conn
                            .execute_batch(&rewritten)
                            .with_context(|| format!("执行语句失败:\n{}", rewritten))?;
                        if is_last {
                            println!("✅ 语句已执行。");
                        }
                    }
                }
            }
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
            &self.opts,
            &self.bind,
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
