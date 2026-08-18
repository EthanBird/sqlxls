use crate::args::Args;
use crate::functions::read_csv::load_csv_text;
use crate::functions::read_excel::excel_to_frame;
use crate::functions::read_json::json_to_table;
use crate::functions::{ExecCtx, FuncOutput, TableFunction};
use crate::ingest::{ingest_rows, IngestOpts};
use anyhow::{bail, Context, Result};
use std::env;
use std::fs::File;
use std::io::Write;
use std::time::Duration;

pub struct ReadApiExt;

impl TableFunction for ReadApiExt {
    fn names(&self) -> &'static [&'static str] {
        &["read_api", "readapi"]
    }

    fn execute(&self, ctx: &mut ExecCtx, args: &Args) -> Result<FuncOutput> {
        let url = expand_env(&args.require_str(0, &["url"], "URL")?);
        let method_raw = args
            .get_str(1, &["method"])
            .unwrap_or_else(|| "GET".to_string());
        let method_str = if method_raw.is_empty() || method_raw.eq_ignore_ascii_case("null") {
            "GET".to_string()
        } else {
            method_raw.to_uppercase()
        };
        let payload = args
            .get_str(2, &["body", "payload", "data"])
            .unwrap_or_default();
        let headers_str = args.get_str(3, &["headers"]).unwrap_or_default();
        let json_path = args.get_str(4, &["json_path", "path"]).unwrap_or_default();

        println!("🌐 正在请求 API: [{}] {}", method_str, url);

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("sqlxls/0.2")
            .build()?;
        let method = reqwest::Method::from_bytes(method_str.as_bytes())
            .with_context(|| format!("不支持的 HTTP 方法: {}", method_str))?;
        let mut req = client.request(method, &url);

        let mut has_auth = false;
        if !headers_str.is_empty() && !headers_str.eq_ignore_ascii_case("null") {
            let expanded = expand_env(&headers_str);
            match serde_json::from_str::<serde_json::Value>(&expanded) {
                Ok(serde_json::Value::Object(map)) => {
                    for (k, v) in map {
                        if k.eq_ignore_ascii_case("authorization") {
                            has_auth = true;
                        }
                        let v_str = if let Some(s) = v.as_str() {
                            expand_env(s)
                        } else {
                            v.to_string()
                        };
                        req = req.header(k, v_str);
                    }
                }
                _ => println!("⚠️ 警告: Headers 参数不是合法的 JSON，已被忽略。"),
            }
        }
        if !has_auth {
            if let Ok(token) = env::var("SQLXLS_BEARER_TOKEN") {
                if !token.is_empty() {
                    req = req.header("Authorization", format!("Bearer {}", token));
                }
            }
        }

        if !payload.is_empty() && !payload.eq_ignore_ascii_case("null") {
            let path = std::path::Path::new(&payload);
            if path.is_file() {
                println!("📤 正在读取并上传文件: {}", payload);
                let file = std::fs::File::open(path)?;
                req = req.body(file);
            } else {
                let body = expand_env(&payload);
                req = req.body(body.clone());
                if !headers_str.to_lowercase().contains("content-type")
                    && serde_json::from_str::<serde_json::Value>(&body).is_ok()
                {
                    req = req.header("Content-Type", "application/json");
                }
            }
        }

        let response = req
            .send()
            .with_context(|| format!("请求 API 失败: {}", url))?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
            .to_lowercase();
        let bytes = response.bytes()?;

        if !status.is_success() {
            let err_text = String::from_utf8_lossy(&bytes);
            let snippet: String = err_text.chars().take(300).collect();
            bail!("API 请求返回了错误状态码: {}\n{}", status, snippet);
        }

        ingest_http_bytes(
            ctx.conn,
            &ctx.dest_table,
            &bytes,
            &content_type,
            &url,
            &json_path,
        )?;
        println!("🌐 成功加载为临时表: {}", ctx.dest_table);
        Ok(FuncOutput::Table)
    }
}

pub fn expand_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("${") {
        out.push_str(&rest[..i]);
        rest = &rest[i + 2..];
        if let Some(end) = rest.find('}') {
            let key = &rest[..end];
            out.push_str(&env::var(key).unwrap_or_default());
            rest = &rest[end + 1..];
        } else {
            out.push_str("${");
            break;
        }
    }
    out.push_str(rest);
    out
}

pub fn ingest_http_bytes(
    conn: &mut rusqlite::Connection,
    table: &str,
    bytes: &[u8],
    content_type: &str,
    url: &str,
    json_path: &str,
) -> Result<()> {
    let url_l = url.to_ascii_lowercase();
    let trimmed = trim_utf8_bom(bytes);

    if looks_like_html(content_type, trimmed) {
        let snippet: String = String::from_utf8_lossy(trimmed).chars().take(200).collect();
        bail!(
            "API 返回了 HTML，而不是表格数据。请检查 URL 或鉴权。\n{}",
            snippet
        );
    }

    if content_type.contains("json") || url_l.ends_with(".json") || looks_like_json(trimmed) {
        let json_val: serde_json::Value =
            serde_json::from_slice(trimmed).context("API 返回的数据不是合法的 JSON")?;
        json_to_table(conn, table, &json_val, json_path)?;
        return Ok(());
    }

    if content_type.contains("csv") || url_l.ends_with(".csv") || url_l.ends_with(".tsv") {
        let text = std::str::from_utf8(trimmed).context("CSV 不是合法 UTF-8")?;
        let delim = if url_l.ends_with(".tsv") {
            Some(b'\t')
        } else {
            None
        };
        load_csv_text(conn, table, text, delim, 0, false, false)?;
        return Ok(());
    }

    if is_xlsx_magic(trimmed)
        || is_xls_magic(trimmed)
        || content_type.contains("spreadsheet")
        || content_type.contains("excel")
        || url_l.ends_with(".xlsx")
        || url_l.ends_with(".xls")
        || url_l.ends_with(".xlsm")
    {
        let ext = if is_xls_magic(trimmed) { "xls" } else { "xlsx" };
        let mut temp_path = env::temp_dir();
        temp_path.push(format!(
            "sqlxls_temp_api_{}.{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis(),
            ext
        ));
        let mut temp_file = File::create(&temp_path)?;
        temp_file.write_all(bytes)?;
        temp_file.flush()?;
        let (headers, rows) = excel_to_frame(temp_path.to_str().unwrap(), None, 0, false)?;
        let _ = std::fs::remove_file(&temp_path);
        ingest_rows(conn, table, &headers, rows, IngestOpts::default())?;
        return Ok(());
    }

    if let Ok(text) = std::str::from_utf8(trimmed) {
        if looks_like_json(trimmed) {
            let json_val: serde_json::Value = serde_json::from_str(text)?;
            json_to_table(conn, table, &json_val, json_path)?;
            return Ok(());
        }
        load_csv_text(conn, table, text, None, 0, false, false).with_context(|| {
            format!(
                "无法把 API 响应识别为 JSON / CSV / Excel（Content-Type: {}）",
                content_type
            )
        })?;
        return Ok(());
    }

    bail!(
        "无法识别的 API 响应类型（Content-Type: {}，{} 字节）",
        content_type,
        bytes.len()
    );
}

fn trim_utf8_bom(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    }
}

fn looks_like_json(bytes: &[u8]) -> bool {
    let s = std::str::from_utf8(bytes)
        .map(|t| t.trim_start())
        .unwrap_or("");
    s.starts_with('{') || s.starts_with('[')
}

fn looks_like_html(content_type: &str, bytes: &[u8]) -> bool {
    if content_type.contains("text/html") {
        return true;
    }
    let s = std::str::from_utf8(bytes)
        .map(|t| t.trim_start().to_ascii_lowercase())
        .unwrap_or_default();
    s.starts_with("<!doctype html") || s.starts_with("<html")
}

fn is_xlsx_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04]) // ZIP / xlsx
}

fn is_xls_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0])
}
