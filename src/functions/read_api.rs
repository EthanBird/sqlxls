use crate::args::Args;
use crate::functions::read_csv::load_csv_text;
use crate::functions::read_excel::excel_to_frame;
use crate::functions::read_json::{default_table_value, extract_json_path, value_to_rows};
use crate::functions::{ExecCtx, FuncOutput, TableFunction};
use crate::ingest::{attach_const_column, ingest_rows, Cell, IngestOpts};
use crate::schema::HeaderSpec;
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
        let page_param = args.get_str(99, &["page_param"]).filter(|s| !s.is_empty());
        let offset_param = args
            .get_str(99, &["offset_param"])
            .filter(|s| !s.is_empty());
        let add_meta = crate::ingest::include_source(args);
        let format = args.get_str(99, &["format", "fmt"]);
        let encoding = args.get_str(99, &["encoding", "charset"]);
        let sheet = args.get_str(99, &["sheet"]);
        let skip = args.get_usize(99, &["skip", "skiprows", "skip_rows"], 0);
        let delim = args
            .get_str(99, &["delim", "delimiter", "sep"])
            .map(|s| crate::functions::read_csv::parse_delim(&s));
        let force_str = args.get_bool(99, &["str", "force_str"]);
        let header = HeaderSpec::from_args(args)?;
        let body = HttpBodyOpts {
            format: format.as_deref(),
            encoding: encoding.as_deref(),
            json_path: json_path.as_str(),
            sheet: sheet.as_deref(),
            skip,
            delim,
            force_str,
            header,
        };

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("sqlxls/0.2")
            .build()?;

        let fetch = |url: &str| -> Result<(Vec<u8>, String)> {
            fetch_http(&client, url, &method_str, &payload, &headers_str)
        };

        if page_param.is_none() && offset_param.is_none() {
            println!("🌐 正在请求 API: [{}] {}", method_str, url);
            let (bytes, content_type) = fetch(&url)?;
            ingest_http_bytes(
                ctx.conn,
                &ctx.dest_table,
                &bytes,
                &content_type,
                &url,
                &body,
                false,
                &[],
            )?;
            println!("🌐 成功加载为临时表: {}", ctx.dest_table);
            return Ok(FuncOutput::Table);
        }

        let page_from = args.get_usize(99, &["page_from"], 1);
        let page_to = args.get_usize(99, &["page_to"], 100).max(page_from);
        let page_size = args.get_usize(99, &["page_size"], 0);
        let page_size_param = args
            .get_str(99, &["page_size_param"])
            .filter(|s| !s.is_empty());
        let offset_step = args.get_usize(
            99,
            &["offset_step"],
            if page_size > 0 { page_size } else { 1 },
        );
        let stop = args
            .get_str(99, &["stop"])
            .unwrap_or_else(|| "empty".into());
        let stop_empty = !stop.eq_ignore_ascii_case("never");

        let mut first = true;
        let mut pages = 0usize;
        if let Some(pp) = page_param {
            for page in page_from..=page_to {
                let mut page_url = with_query_param(&url, &pp, &page.to_string());
                if let Some(ref psp) = page_size_param {
                    if page_size > 0 {
                        page_url = with_query_param(&page_url, psp, &page_size.to_string());
                    }
                }
                println!("🌐 正在请求 API: [{}] {}", method_str, page_url);
                let (bytes, content_type) = fetch(&page_url)?;
                let extras = if add_meta {
                    vec![("_page".to_string(), page.to_string())]
                } else {
                    vec![]
                };
                let n = ingest_http_bytes(
                    ctx.conn,
                    &ctx.dest_table,
                    &bytes,
                    &content_type,
                    &page_url,
                    &body,
                    !first,
                    &extras,
                )?;
                first = false;
                pages += 1;
                if stop_empty && n == 0 {
                    break;
                }
            }
        } else if let Some(op) = offset_param {
            let mut offset = 0i64;
            for _ in 0..=(page_to - page_from) {
                let mut page_url = with_query_param(&url, &op, &offset.to_string());
                if let Some(ref psp) = page_size_param {
                    if page_size > 0 {
                        page_url = with_query_param(&page_url, psp, &page_size.to_string());
                    }
                }
                println!("🌐 正在请求 API: [{}] {}", method_str, page_url);
                let (bytes, content_type) = fetch(&page_url)?;
                let extras = if add_meta {
                    vec![("_offset".to_string(), offset.to_string())]
                } else {
                    vec![]
                };
                let n = ingest_http_bytes(
                    ctx.conn,
                    &ctx.dest_table,
                    &bytes,
                    &content_type,
                    &page_url,
                    &body,
                    !first,
                    &extras,
                )?;
                first = false;
                pages += 1;
                if stop_empty && n == 0 {
                    break;
                }
                offset += offset_step as i64;
            }
        }

        println!("🌐 分页加载完成: {} 次请求 → 表 {}", pages, ctx.dest_table);
        Ok(FuncOutput::Table)
    }
}

fn fetch_http(
    client: &reqwest::blocking::Client,
    url: &str,
    method_str: &str,
    payload: &str,
    headers_str: &str,
) -> Result<(Vec<u8>, String)> {
    let method = reqwest::Method::from_bytes(method_str.as_bytes())
        .with_context(|| format!("不支持的 HTTP 方法: {}", method_str))?;
    let mut req = client.request(method, url);

    let mut has_auth = false;
    if !headers_str.is_empty() && !headers_str.eq_ignore_ascii_case("null") {
        let expanded = expand_env(headers_str);
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
        let path = std::path::Path::new(payload);
        if path.is_file() {
            println!("📤 正在读取并上传文件: {}", payload);
            let file = std::fs::File::open(path)?;
            req = req.body(file);
        } else {
            let body = expand_env(payload);
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
    Ok((bytes.to_vec(), content_type))
}

pub fn with_query_param(url: &str, key: &str, value: &str) -> String {
    let hash = url.find('#');
    let (base_url, frag) = match hash {
        Some(i) => (&url[..i], Some(&url[i..])),
        None => (url, None),
    };
    let updated = if let Some(q) = base_url.find('?') {
        let (base, query) = base_url.split_at(q);
        let query = &query[1..];
        let mut parts: Vec<String> = Vec::new();
        let mut found = false;
        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let name = pair.split('=').next().unwrap_or("");
            if name.eq_ignore_ascii_case(key) {
                parts.push(format!("{key}={value}"));
                found = true;
            } else {
                parts.push(pair.to_string());
            }
        }
        if !found {
            parts.push(format!("{key}={value}"));
        }
        format!("{base}?{}", parts.join("&"))
    } else {
        format!("{base_url}?{key}={value}")
    };
    match frag {
        Some(f) => format!("{updated}{f}"),
        None => updated,
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

#[derive(Clone, Debug)]
pub struct HttpBodyOpts<'a> {
    pub format: Option<&'a str>,
    pub encoding: Option<&'a str>,
    pub json_path: &'a str,
    pub sheet: Option<&'a str>,
    pub skip: usize,
    pub delim: Option<u8>,
    pub force_str: bool,
    pub header: HeaderSpec,
}

impl<'a> HttpBodyOpts<'a> {
    pub fn new() -> Self {
        Self {
            format: None,
            encoding: None,
            json_path: "",
            sheet: None,
            skip: 0,
            delim: None,
            force_str: false,
            header: HeaderSpec::default(),
        }
    }
}

pub fn ingest_http_bytes(
    conn: &mut rusqlite::Connection,
    table: &str,
    bytes: &[u8],
    content_type: &str,
    url: &str,
    opts: &HttpBodyOpts<'_>,
    append: bool,
    extras: &[(String, String)],
) -> Result<usize> {
    let url_l = url.to_ascii_lowercase();
    let trimmed = crate::encoding::trim_utf8_bom(bytes);
    let forced = opts
        .format
        .map(|s| s.to_ascii_lowercase())
        .filter(|s| !matches!(s.as_str(), "http" | "https" | "api" | ""));

    if looks_like_html(content_type, trimmed) {
        let snippet: String = String::from_utf8_lossy(trimmed).chars().take(200).collect();
        bail!(
            "API 返回了 HTML，而不是表格数据。请检查 URL 或鉴权。\n{}",
            snippet
        );
    }

    let excel_hint = is_xlsx_magic(trimmed)
        || is_xls_magic(trimmed)
        || content_type.contains("spreadsheet")
        || content_type.contains("excel")
        || url_l.ends_with(".xlsx")
        || url_l.ends_with(".xls")
        || url_l.ends_with(".xlsm");
    let as_excel = forced.as_deref() == Some("excel")
        || forced.as_deref() == Some("xlsx")
        || forced.as_deref() == Some("xls")
        || (forced.is_none() && excel_hint);

    if as_excel {
        return ingest_excel_bytes(conn, table, bytes, trimmed, opts, append, extras);
    }

    let as_json = forced.as_deref() == Some("json")
        || (forced.is_none()
            && (content_type.contains("json")
                || url_l.ends_with(".json")
                || looks_like_json(trimmed)));
    let as_csv = forced.as_deref() == Some("csv")
        || forced.as_deref() == Some("tsv")
        || (forced.is_none()
            && (content_type.contains("csv")
                || url_l.ends_with(".csv")
                || url_l.ends_with(".tsv")));

    let text = crate::encoding::decode_http(trimmed, opts.encoding, content_type)?;

    if as_json || (forced.is_none() && looks_like_json(trimmed)) {
        return ingest_json_text(conn, table, &text, opts.json_path, append, extras);
    }
    if as_csv || forced.is_none() {
        let delim = opts.delim.or_else(|| {
            if url_l.ends_with(".tsv") || forced.as_deref() == Some("tsv") {
                Some(b'\t')
            } else {
                None
            }
        });
        return ingest_csv_with_extras(
            conn,
            table,
            &text,
            delim,
            opts.skip,
            opts.force_str,
            append,
            extras,
            &opts.header,
        )
        .with_context(|| {
            format!(
                "无法把 API 响应识别为 JSON / CSV / Excel（Content-Type: {}）",
                content_type
            )
        });
    }

    bail!(
        "无法识别的 API 响应类型（Content-Type: {}，{} 字节）。可显式写 format='json'|'csv'|'excel'，文本编码用 encoding='gbk'",
        content_type,
        bytes.len()
    );
}

fn ingest_json_text(
    conn: &mut rusqlite::Connection,
    table: &str,
    text: &str,
    json_path: &str,
    append: bool,
    extras: &[(String, String)],
) -> Result<usize> {
    let json_val: serde_json::Value =
        serde_json::from_str(text).context("API 返回的数据不是合法的 JSON")?;
    let extracted = extract_json_path(&json_val, json_path)?;
    let table_val = if json_path.trim().is_empty() {
        default_table_value(extracted)
    } else {
        extracted
    };
    let (mut headers, mut rows) = value_to_rows(table_val)?;
    let n = rows.len();
    for (k, v) in extras {
        attach_const_column(&mut headers, &mut rows, k, Cell::Text(v.clone()));
    }
    ingest_rows(
        conn,
        table,
        &headers,
        rows,
        IngestOpts {
            force_str: false,
            append,
        },
    )?;
    Ok(n)
}

fn ingest_excel_bytes(
    conn: &mut rusqlite::Connection,
    table: &str,
    bytes: &[u8],
    trimmed: &[u8],
    opts: &HttpBodyOpts<'_>,
    append: bool,
    extras: &[(String, String)],
) -> Result<usize> {
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
    let frame = excel_to_frame(
        temp_path.to_str().unwrap(),
        opts.sheet,
        opts.skip,
        opts.force_str,
        &opts.header,
    );
    let _ = std::fs::remove_file(&temp_path);
    let (mut headers, mut rows) = frame?;
    let n = rows.len();
    for (k, v) in extras {
        attach_const_column(&mut headers, &mut rows, k, Cell::Text(v.clone()));
    }
    ingest_rows(
        conn,
        table,
        &headers,
        rows,
        IngestOpts {
            force_str: opts.force_str,
            append,
        },
    )?;
    Ok(n)
}

fn ingest_csv_with_extras(
    conn: &mut rusqlite::Connection,
    table: &str,
    text: &str,
    delim: Option<u8>,
    skip: usize,
    force_str: bool,
    append: bool,
    extras: &[(String, String)],
    header: &HeaderSpec,
) -> Result<usize> {
    if extras.is_empty() {
        return load_csv_text(conn, table, text, delim, skip, force_str, append, header);
    }
    let tmp = format!("{table}__page");
    let n = load_csv_text(conn, &tmp, text, delim, skip, force_str, false, header)?;
    crate::ingest::union_from_table(conn, table, &tmp, extras, append)?;
    conn.execute(&format!("DROP TABLE IF EXISTS {tmp}"), [])?;
    Ok(n)
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
