use crate::args::Args;
use anyhow::{bail, Result};
use rusqlite::Connection;

pub mod mock_data;
pub mod read_api;
pub mod read_auto;
pub mod read_clipboard;
pub mod read_csv;
pub mod read_dir;
pub mod read_excel;
pub mod read_json;
pub mod read_text;

use mock_data::MockDataExt;
use read_api::ReadApiExt;
use read_auto::ReadAutoExt;
use read_clipboard::ReadClipboardExt;
use read_csv::ReadCsvExt;
use read_dir::ReadDirExt;
use read_excel::ReadExcelExt;
use read_json::ReadJsonExt;
use read_text::ReadTextExt;

pub struct ExecCtx<'a> {
    pub conn: &'a mut Connection,
    pub dest_table: String,
}

pub enum FuncOutput {
    Table,
    Scalar(String),
}

pub trait TableFunction: Send + Sync {
    fn names(&self) -> &'static [&'static str];
    fn execute(&self, ctx: &mut ExecCtx, args: &Args) -> Result<FuncOutput>;
}

pub struct Registry {
    fns: Vec<Box<dyn TableFunction>>,
}

impl Registry {
    pub fn builtin() -> Self {
        Self {
            fns: vec![
                Box::new(ReadTextExt),
                Box::new(ReadExcelExt),
                Box::new(ReadCsvExt),
                Box::new(ReadJsonExt),
                Box::new(ReadApiExt),
                Box::new(ReadDirExt),
                Box::new(ReadClipboardExt),
                Box::new(MockDataExt),
                Box::new(ReadAutoExt),
            ],
        }
    }

    pub fn is_func(&self, name: &str) -> bool {
        let n = name.to_ascii_lowercase();
        self.fns.iter().any(|f| f.names().iter().any(|x| *x == n))
    }

    pub fn execute(&self, ctx: &mut ExecCtx, name: &str, args: &Args) -> Result<FuncOutput> {
        let n = name.to_ascii_lowercase();
        for f in &self.fns {
            if f.names().iter().any(|x| *x == n) {
                return f.execute(ctx, args);
            }
        }
        bail!("未知的表函数: {}", name)
    }
}
