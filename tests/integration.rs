use rusqlite::Connection;
use rust_xlsxwriter::Workbook;
use sqlxls::functions::Registry;
use sqlxls::rewrite::rewrite_sql;
use sqlxls::session::Session;
use sqlxls::syntax::SyntaxOpts;
use std::fs;
use std::path::PathBuf;

fn temp_dir() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "sqlxls-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn write_xlsx(path: &std::path::Path, headers: &[&str], rows: &[Vec<&str>]) {
    let mut wb = Workbook::new();
    let sheet = wb.add_worksheet();
    for (i, h) in headers.iter().enumerate() {
        sheet.write_string(0, i as u16, *h).unwrap();
    }
    for (r, row) in rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            if v.contains('.') {
                if let Ok(n) = v.parse::<f64>() {
                    sheet.write_number((r as u32) + 1, c as u16, n).unwrap();
                    continue;
                }
            }
            sheet.write_string((r as u32) + 1, c as u16, *v).unwrap();
        }
    }
    wb.save(path).unwrap();
}

fn write_xlsx_data_only(path: &std::path::Path, rows: &[Vec<&str>]) {
    let mut wb = Workbook::new();
    let sheet = wb.add_worksheet();
    for (r, row) in rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            if v.contains('.') {
                if let Ok(n) = v.parse::<f64>() {
                    sheet.write_number(r as u32, c as u16, n).unwrap();
                    continue;
                }
            }
            sheet.write_string(r as u32, c as u16, *v).unwrap();
        }
    }
    wb.save(path).unwrap();
}

fn table_column_names(s: &Session, table: &str) -> Vec<String> {
    let mut stmt = s
        .connection()
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    let mut rows = stmt.query([]).unwrap();
    let mut names = Vec::new();
    while let Some(row) = rows.next().unwrap() {
        names.push(row.get::<_, String>(1).unwrap());
    }
    names
}

#[test]
fn csv_filter_and_types() {
    let dir = temp_dir();
    let csv = dir.join("a.csv");
    fs::write(&csv, "id,name,amount\n1,alice,10.5\n2,bob,20\n").unwrap();
    let mut s = Session::new().unwrap();
    let sql = format!(
        "SELECT name, amount FROM read_csv('{}') WHERE id = 2",
        csv.display()
    );
    s.run_sql(&sql, None, false).unwrap();
    let n: i64 = s
        .connection()
        .query_row("SELECT COUNT(*) FROM excel_tmp_0", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
}

#[test]
fn json_union_keys_and_path() {
    let dir = temp_dir();
    let json = dir.join("a.json");
    fs::write(
        &json,
        r#"{"code":0,"data":[{"id":1,"name":"a"},{"id":2,"extra":true}]}"#,
    )
    .unwrap();
    let mut conn = Connection::open_in_memory().unwrap();
    let reg = Registry::builtin();
    let mut c = 0;
    let sql = format!(
        "SELECT * FROM read_json('{}', json_path='data')",
        json.display()
    );
    let rewritten = rewrite_sql(
        &sql,
        &mut conn,
        &reg,
        &mut c,
        &SyntaxOpts::default(),
        &sqlxls::bind::BindCtx::default(),
    )
    .unwrap();
    let stmt = conn.prepare(&rewritten).unwrap();
    let names = stmt.column_names();
    assert!(names.contains(&"id"), "{names:?}");
    assert!(names.contains(&"name"), "{names:?}");
    assert!(names.contains(&"extra"), "{names:?}");
}

#[test]
fn excel_skip_and_join_csv() {
    let dir = temp_dir();
    let xlsx = dir.join("u.xlsx");
    write_xlsx(
        &xlsx,
        &["note", "id", "name"],
        &[
            vec!["junk", "junk", "junk"],
            vec!["id", "name", "dept"],
            vec!["1", "Ada", "eng"],
            vec!["2", "Bob", "ops"],
        ],
    );
    // 上面把第一行当表头写进去了；再写一个干净文件
    let xlsx = dir.join("users.xlsx");
    write_xlsx(
        &xlsx,
        &["id", "name"],
        &[vec!["1", "Ada"], vec!["2", "Bob"]],
    );
    let csv = dir.join("dept.csv");
    fs::write(&csv, "user_id,department\n1,eng\n2,ops\n").unwrap();

    let mut s = Session::new().unwrap();
    let sql = format!(
        "SELECT a.name, b.department FROM read_excel('{x}') AS a \
         JOIN read_csv('{c}') AS b ON a.id = b.user_id ORDER BY a.name",
        x = xlsx.display(),
        c = csv.display()
    );
    s.run_sql(&sql, None, false).unwrap();
}

#[test]
fn nested_read_text_into_csv() {
    let dir = temp_dir();
    let csv = dir.join("t.csv");
    fs::write(&csv, "x,y\n1,2\n3,4\n").unwrap();
    let pointer = dir.join("path.txt");
    fs::write(&pointer, csv.to_str().unwrap()).unwrap();

    let mut conn = Connection::open_in_memory().unwrap();
    let reg = Registry::builtin();
    let mut c = 0;
    let sql = format!(
        "SELECT * FROM read_csv(read_text('{}')) WHERE x = 3",
        pointer.display()
    );
    let rewritten = rewrite_sql(
        &sql,
        &mut conn,
        &reg,
        &mut c,
        &SyntaxOpts::default(),
        &sqlxls::bind::BindCtx::default(),
    )
    .unwrap();
    assert!(rewritten.contains("excel_tmp_"), "{}", rewritten);
    let n: i64 = conn
        .query_row(&rewritten, [], |r| r.get(0))
        .expect(&rewritten);
    assert_eq!(n, 3);
}

#[test]
fn glob_merges_by_column_name() {
    let dir = temp_dir();
    fs::write(dir.join("a.csv"), "id,name\n1,a\n").unwrap();
    fs::write(dir.join("b.csv"), "id,age\n2,9\n").unwrap();
    let pattern = dir.join("*.csv");
    let mut s = Session::new().unwrap();
    let sql = format!("SELECT * FROM read_dir('{}')", pattern.display());
    s.run_sql(&sql, None, false).unwrap();
    let n: i64 = s
        .connection()
        .query_row("SELECT COUNT(*) FROM excel_tmp_0", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
    let cols: Vec<String> = {
        let mut stmt = s
            .connection()
            .prepare("PRAGMA table_info(excel_tmp_0)")
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|x| x.unwrap())
            .collect()
    };
    assert!(cols.contains(&"id".to_string()));
    assert!(cols.contains(&"name".to_string()));
    assert!(cols.contains(&"age".to_string()));
}

#[test]
fn multi_statement_session() {
    let dir = temp_dir();
    let csv = dir.join("t.csv");
    fs::write(&csv, "id,n\n1,10\n2,20\n").unwrap();
    let mut s = Session::new().unwrap();
    let sql = format!(
        "CREATE TABLE t AS SELECT * FROM read_csv('{}');\nSELECT SUM(n) FROM t;",
        csv.display()
    );
    s.run_sql(&sql, None, false).unwrap();
}

#[test]
fn html_bytes_are_not_excel() {
    let mut conn = Connection::open_in_memory().unwrap();
    let err = sqlxls::functions::read_api::ingest_http_bytes(
        &mut conn,
        "t",
        b"<!DOCTYPE html><html>nope</html>",
        "text/html",
        "https://example.com/data",
        &sqlxls::functions::read_api::HttpBodyOpts::new(),
        false,
        &[],
    )
    .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("HTML"), "{msg}");
}

#[test]
fn json_output_streaming_file() {
    let dir = temp_dir();
    let csv = dir.join("t.csv");
    fs::write(&csv, "id,name\n1,a\n").unwrap();
    let out = dir.join("out.json");
    let mut s = Session::new().unwrap();
    let sql = format!("SELECT * FROM read_csv('{}')", csv.display());
    s.run_sql(&sql, Some(&out), false).unwrap();
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(v[0]["name"], "a");
}

#[test]
fn load_then_standard_sql() {
    let dir = temp_dir();
    let csv = dir.join("t.csv");
    fs::write(&csv, "id,n\n1,10\n2,20\n").unwrap();
    let mut s = Session::new().unwrap();
    let sql = format!(
        "LOAD nums FROM '{p}'; SELECT SUM(n) AS s FROM nums;",
        p = csv.display()
    );
    s.run_sql(&sql, None, false).unwrap();
    let sum: i64 = s
        .connection()
        .query_row("SELECT SUM(n) FROM nums", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sum, 30);
}

#[test]
fn load_with_named_options() {
    let dir = temp_dir();
    let csv = dir.join("t.csv");
    fs::write(&csv, "skipme\nid,name\n1,a\n").unwrap();
    let mut s = Session::new().unwrap();
    let sql = format!(
        "LOAD t FROM '{}' WITH (format='csv', skip=1); SELECT name FROM t",
        csv.display()
    );
    s.run_sql(&sql, None, false).unwrap();
    let name: String = s
        .connection()
        .query_row("SELECT name FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "a");
}

#[test]
fn strict_rejects_positional_options() {
    let dir = temp_dir();
    let csv = dir.join("t.csv");
    fs::write(&csv, "id,name\n1,a\n").unwrap();
    let mut s = Session::with_opts(SyntaxOpts {
        strict: true,
        ..Default::default()
    })
    .unwrap();
    let sql = format!("SELECT * FROM read_csv('{}', ',')", csv.display());
    let err = s.run_sql(&sql, None, false).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("strict") || msg.contains("命名参数"), "{msg}");
}

#[test]
fn strict_allows_named_options() {
    let dir = temp_dir();
    let csv = dir.join("t.csv");
    fs::write(&csv, "id,name\n1,a\n").unwrap();
    let mut s = Session::with_opts(SyntaxOpts {
        strict: true,
        ..Default::default()
    })
    .unwrap();
    let sql = format!("SELECT * FROM read('{}', format='csv')", csv.display());
    s.run_sql(&sql, None, false).unwrap();
}

#[test]
fn table_fn_in_select_is_rejected() {
    let dir = temp_dir();
    let csv = dir.join("t.csv");
    fs::write(&csv, "id,name\n1,a\n").unwrap();
    let mut s = Session::new().unwrap();
    let sql = format!("SELECT read_csv('{}')", csv.display());
    let err = s.run_sql(&sql, None, false).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("FROM") || msg.contains("LOAD"), "{msg}");
}

#[test]
fn read_text_cannot_be_a_table() {
    let dir = temp_dir();
    let txt = dir.join("p.txt");
    fs::write(&txt, "hello").unwrap();
    let mut s = Session::new().unwrap();
    let sql = format!("SELECT * FROM read_text('{}')", txt.display());
    let err = s.run_sql(&sql, None, false).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("标量") || msg.contains("不能当作表"), "{msg}");
}

#[test]
fn unknown_option_is_error() {
    let dir = temp_dir();
    let csv = dir.join("t.csv");
    fs::write(&csv, "id,name\n1,a\n").unwrap();
    let mut s = Session::new().unwrap();
    let sql = format!(
        "SELECT * FROM read('{}', format='csv', foo=1)",
        csv.display()
    );
    let err = s.run_sql(&sql, None, false).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("foo"), "{msg}");
}

#[test]
fn syntax2_rejects_sugar_in_query() {
    let dir = temp_dir();
    let csv = dir.join("t.csv");
    fs::write(&csv, "id,name\n1,a\n").unwrap();
    let mut s = Session::with_opts(SyntaxOpts {
        version: 2,
        strict: false,
    })
    .unwrap();
    let sql = format!("SELECT * FROM read_csv('{}')", csv.display());
    let err = s.run_sql(&sql, None, false).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("syntax=2") || msg.contains("read()"), "{msg}");
}

#[test]
fn syntax2_allows_canonical_read() {
    let dir = temp_dir();
    let csv = dir.join("t.csv");
    fs::write(&csv, "id,name\n1,a\n").unwrap();
    let mut s = Session::with_opts(SyntaxOpts {
        version: 2,
        strict: true,
    })
    .unwrap();
    let sql = format!("SELECT * FROM read('{}', format='csv')", csv.display());
    s.run_sql(&sql, None, false).unwrap();
}

#[test]
fn set_interpolates_locator() {
    let dir = temp_dir();
    let csv = dir.join("east.csv");
    fs::write(&csv, "id,n\n1,10\n").unwrap();
    let mut s = Session::new().unwrap();
    let sql = format!(
        "SET file = '{p}'; LOAD t FROM '${{file}}'; SELECT n FROM t",
        p = csv.display()
    );
    s.run_sql(&sql, None, false).unwrap();
    let n: i64 = s
        .connection()
        .query_row("SELECT n FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 10);
}

#[test]
fn each_unions_with_source_column() {
    let dir = temp_dir();
    fs::write(dir.join("a.csv"), "id,n\n1,10\n").unwrap();
    fs::write(dir.join("b.csv"), "id,n\n2,20\n").unwrap();
    let mut s = Session::new().unwrap();
    let sql = format!(
        "LOAD t FROM EACH ('{a}', '{b}'); SELECT SUM(n) AS s FROM t",
        a = dir.join("a.csv").display(),
        b = dir.join("b.csv").display()
    );
    s.run_sql(&sql, None, false).unwrap();
    let (sum, files): (i64, i64) = s
        .connection()
        .query_row("SELECT SUM(n), COUNT(DISTINCT _source) FROM t", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(sum, 30);
    assert_eq!(files, 2);
}

#[test]
fn for_list_adds_region_column() {
    let dir = temp_dir();
    fs::write(dir.join("east.csv"), "id,n\n1,1\n").unwrap();
    fs::write(dir.join("west.csv"), "id,n\n2,2\n").unwrap();
    let mut s = Session::new().unwrap();
    let sql = format!(
        "LOAD t FROM '{dir}/${{region}}.csv' FOR region IN ('east', 'west');\n\
         SELECT _region, SUM(n) AS s FROM t GROUP BY _region ORDER BY _region",
        dir = dir.display()
    );
    s.run_sql(&sql, None, false).unwrap();
    let n: i64 = s
        .connection()
        .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
    let east: i64 = s
        .connection()
        .query_row("SELECT SUM(n) FROM t WHERE _region = 'east'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(east, 1);
}

#[test]
fn for_cartesian_two_axes() {
    let dir = temp_dir();
    fs::write(dir.join("east-2024.csv"), "id,n\n1,1\n").unwrap();
    fs::write(dir.join("east-2025.csv"), "id,n\n2,2\n").unwrap();
    fs::write(dir.join("west-2024.csv"), "id,n\n3,4\n").unwrap();
    fs::write(dir.join("west-2025.csv"), "id,n\n4,8\n").unwrap();
    let mut s = Session::new().unwrap();
    let sql = format!(
        "LOAD t FROM '{dir}/${{region}}-${{year}}.csv'\n\
         FOR region IN ('east', 'west')\n\
         FOR year IN (2024, 2025);\n\
         SELECT COUNT(*) AS n FROM t",
        dir = dir.display()
    );
    s.run_sql(&sql, None, false).unwrap();
    let n: i64 = s
        .connection()
        .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 4);
    let sum: i64 = s
        .connection()
        .query_row("SELECT SUM(n) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sum, 15);
}

#[test]
fn for_tuple_pairs() {
    let dir = temp_dir();
    fs::write(dir.join("east-prod.csv"), "id,n\n1,10\n").unwrap();
    fs::write(dir.join("west-stg.csv"), "id,n\n2,20\n").unwrap();
    let mut s = Session::new().unwrap();
    let sql = format!(
        "LOAD t FROM '{dir}/${{region}}-${{env}}.csv'\n\
         FOR (region, env) IN (('east', 'prod'), ('west', 'stg'));\n\
         SELECT _region, _env, n FROM t ORDER BY _region",
        dir = dir.display()
    );
    s.run_sql(&sql, None, false).unwrap();
    let n: i64 = s
        .connection()
        .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
    let env: String = s
        .connection()
        .query_row("SELECT _env FROM t WHERE _region = 'east'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(env, "prod");
}

#[test]
fn glob_adds_source_and_merges_json() {
    let dir = temp_dir();
    fs::write(dir.join("a.json"), r#"[{"id":1,"name":"a"}]"#).unwrap();
    fs::write(dir.join("b.json"), r#"[{"id":2,"extra":true}]"#).unwrap();
    let mut s = Session::new().unwrap();
    let sql = format!(
        "LOAD t FROM '{}' WITH (format='glob'); SELECT COUNT(*) AS n FROM t",
        dir.join("*.json").display()
    );
    s.run_sql(&sql, None, false).unwrap();
    let n: i64 = s
        .connection()
        .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
}

#[test]
fn http_pagination_unions_pages() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming().take(8) {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => break,
            };
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let page = req
                .split("page=")
                .nth(1)
                .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(1);
            let body = if page <= 2 {
                format!(r#"[{{"id":{page}}}]"#)
            } else {
                "[]".into()
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let url = format!("http://{addr}/items");
    let mut s = Session::new().unwrap();
    let sql = format!(
        "LOAD t FROM '{url}' WITH (format='json', page_param='page', page_from=1, page_to=5);\n\
         SELECT COUNT(*) AS n FROM t"
    );
    s.run_sql(&sql, None, false).unwrap();
    let n: i64 = s
        .connection()
        .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2, "two non-empty pages");
}

#[test]
fn with_query_param_replaces_existing() {
    use sqlxls::functions::read_api::with_query_param;
    assert_eq!(
        with_query_param("https://x/a?page=1&q=2", "page", "3"),
        "https://x/a?page=3&q=2"
    );
    assert_eq!(
        with_query_param("https://x/a", "page", "1"),
        "https://x/a?page=1"
    );
}

#[test]
fn date_range_for_and_sql_converters() {
    let dir = temp_dir();
    for d in ["2024-01-01", "2024-01-02", "2024-01-03"] {
        fs::write(dir.join(format!("{d}.csv")), format!("day,n\n{d},1\n")).unwrap();
    }
    let mut s = Session::new().unwrap();
    let sql = format!(
        "LOAD t FROM '{}/${{d}}.csv' FOR d IN DATE '2024-01-01'..'2024-01-03';\n\
         SELECT COUNT(*) AS n FROM t",
        dir.display()
    );
    s.run_sql(&sql, None, false).unwrap();
    let n: i64 = s
        .connection()
        .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 3);
    let days: i64 = s
        .connection()
        .query_row("SELECT COUNT(DISTINCT _d) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(days, 3);

    let parsed: String = s
        .connection()
        .query_row("SELECT parse_date('15/01/2024', 'dmy')", [], |r| r.get(0))
        .unwrap();
    assert_eq!(parsed, "2024-01-15");
    let compact: String = s
        .connection()
        .query_row("SELECT parse_date(20240115)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(compact, "2024-01-15");
    let unix: String = s
        .connection()
        .query_row("SELECT from_unix(1700000000)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(unix, "2023-11-14 22:13:20");
    let ms: String = s
        .connection()
        .query_row("SELECT from_unix(1700000000000)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ms, "2023-11-14 22:13:20");
    let serial: String = s
        .connection()
        .query_row("SELECT excel_serial(44927)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(serial, "2023-01-01");
    let bad: Option<String> = s
        .connection()
        .query_row("SELECT parse_date('01/02/2024')", [], |r| r.get(0))
        .unwrap();
    assert!(bad.is_none(), "ambiguous D/M must stay NULL without fmt");
}

fn serve_http_bytes(body: Vec<u8>, content_type: &str) -> String {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let ct = content_type.to_string();
    thread::spawn(move || {
        for stream in listener.incoming().take(6) {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => break,
            };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    format!("http://{addr}/download")
}

#[test]
fn local_gbk_csv_needs_encoding() {
    let dir = temp_dir();
    let csv = dir.join("gbk.csv");
    let (bytes, _, _) = encoding_rs::GBK.encode("姓名,数量\n张三,1\n");
    fs::write(&csv, bytes.as_ref()).unwrap();

    let mut s = Session::new().unwrap();
    let err = s
        .run_sql(
            &format!(
                "LOAD t FROM '{}' WITH (format='csv'); SELECT * FROM t",
                csv.display()
            ),
            None,
            false,
        )
        .unwrap_err();
    assert!(format!("{err:#}").contains("encoding='gbk'"), "{err:#}");

    let mut s = Session::new().unwrap();
    s.run_sql(
        &format!(
            "LOAD t FROM '{}' WITH (format='csv', encoding='gbk'); SELECT \"姓名\" FROM t",
            csv.display()
        ),
        None,
        false,
    )
    .unwrap();
    let name: String = s
        .connection()
        .query_row("SELECT \"姓名\" FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "张三");
}

#[test]
fn http_gbk_csv_charset_and_encoding() {
    let (bytes, _, _) = encoding_rs::GBK.encode("id,name\n1,李四\n");
    let body = bytes.into_owned();

    let url = serve_http_bytes(body.clone(), "text/csv; charset=gbk");
    let mut s = Session::new().unwrap();
    s.run_sql(
        &format!("LOAD t FROM '{url}' WITH (format='csv'); SELECT name FROM t"),
        None,
        false,
    )
    .unwrap();
    let name: String = s
        .connection()
        .query_row("SELECT name FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "李四");

    let url = serve_http_bytes(body, "application/octet-stream");
    let mut s = Session::new().unwrap();
    s.run_sql(
        &format!("LOAD t FROM '{url}' WITH (format='csv', encoding='gbk'); SELECT name FROM t"),
        None,
        false,
    )
    .unwrap();
    let name: String = s
        .connection()
        .query_row("SELECT name FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "李四");
}

#[test]
fn http_xlsx_binary_octet_stream() {
    let dir = temp_dir();
    let xlsx = dir.join("book.xlsx");
    write_xlsx(&xlsx, &["id", "name"], &[vec!["1", "bin"]]);
    let bytes = fs::read(&xlsx).unwrap();
    assert_eq!(
        &bytes[..4],
        &[0x50, 0x4B, 0x03, 0x04],
        "xlsx is zip/pk magic"
    );

    let url = serve_http_bytes(bytes, "application/octet-stream");
    let mut s = Session::new().unwrap();
    s.run_sql(
        &format!("LOAD t FROM '{url}'; SELECT name FROM t"),
        None,
        false,
    )
    .unwrap();
    let name: String = s
        .connection()
        .query_row("SELECT name FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "bin");
}

#[test]
fn excel_header_false_auto_and_custom_columns() {
    let dir = temp_dir();
    let xlsx = dir.join("nohead.xlsx");
    write_xlsx_data_only(&xlsx, &[vec!["1", "Ada"], vec!["2", "Bob"]]);

    let mut s = Session::new().unwrap();
    s.run_sql(
        &format!(
            "LOAD t FROM '{}' WITH (format='excel', header=false); SELECT col_0, col_1 FROM t ORDER BY col_0",
            xlsx.display()
        ),
        None,
        false,
    )
    .unwrap();
    assert_eq!(table_column_names(&s, "t"), vec!["col_0", "col_1"]);
    let n: i64 = s
        .connection()
        .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
    let first: String = s
        .connection()
        .query_row(
            "SELECT CAST(col_1 AS TEXT) FROM t ORDER BY col_0 LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(first, "Ada");

    let mut s = Session::new().unwrap();
    s.run_sql(
        &format!(
            "LOAD t FROM '{}' WITH (format='excel', header=false, columns='id,name');\
             SELECT id, name FROM t ORDER BY id",
            xlsx.display()
        ),
        None,
        false,
    )
    .unwrap();
    assert_eq!(table_column_names(&s, "t"), vec!["id", "name"]);
    let name: String = s
        .connection()
        .query_row("SELECT name FROM t WHERE id = 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "Ada");
}

#[test]
fn excel_columns_renames_existing_header() {
    let dir = temp_dir();
    let xlsx = dir.join("head.xlsx");
    write_xlsx(&xlsx, &["id", "name"], &[vec!["1", "Ada"]]);
    let mut s = Session::new().unwrap();
    s.run_sql(
        &format!(
            "LOAD t FROM '{}' WITH (format='excel', columns='uid,uname'); SELECT uid, uname FROM t",
            xlsx.display()
        ),
        None,
        false,
    )
    .unwrap();
    assert_eq!(table_column_names(&s, "t"), vec!["uid", "uname"]);
    let name: String = s
        .connection()
        .query_row("SELECT uname FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "Ada");
}

#[test]
fn csv_header_false_and_custom_columns() {
    let dir = temp_dir();
    let csv = dir.join("nohead.csv");
    fs::write(&csv, "1,Ada\n2,Bob\n").unwrap();

    let mut s = Session::new().unwrap();
    s.run_sql(
        &format!(
            "LOAD t FROM '{}' WITH (format='csv', header=false); SELECT col_0, col_1 FROM t",
            csv.display()
        ),
        None,
        false,
    )
    .unwrap();
    assert_eq!(table_column_names(&s, "t"), vec!["col_0", "col_1"]);
    let n: i64 = s
        .connection()
        .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);

    let mut s = Session::new().unwrap();
    s.run_sql(
        &format!(
            "LOAD t FROM '{}' WITH (format='csv', header=false, columns='id,name');\
             SELECT name FROM t WHERE id = 2",
            csv.display()
        ),
        None,
        false,
    )
    .unwrap();
    let name: String = s
        .connection()
        .query_row("SELECT name FROM t WHERE id = 2", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "Bob");
}

#[test]
fn columns_without_header_false_still_consumes_first_row() {
    let dir = temp_dir();
    let csv = dir.join("nohead.csv");
    fs::write(&csv, "1,Ada\n2,Bob\n").unwrap();
    let mut s = Session::new().unwrap();
    s.run_sql(
        &format!(
            "LOAD t FROM '{}' WITH (format='csv', columns='id,name'); SELECT * FROM t",
            csv.display()
        ),
        None,
        false,
    )
    .unwrap();
    assert_eq!(table_column_names(&s, "t"), vec!["id", "name"]);
    let n: i64 = s
        .connection()
        .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
    let name: String = s
        .connection()
        .query_row("SELECT name FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "Bob");
}

#[test]
fn json_rejects_header_option() {
    let dir = temp_dir();
    let json = dir.join("a.json");
    fs::write(&json, r#"[{"id":1}]"#).unwrap();
    let mut s = Session::new().unwrap();
    let err = s
        .run_sql(
            &format!(
                "LOAD t FROM '{}' WITH (format='json', header=false)",
                json.display()
            ),
            None,
            false,
        )
        .unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("不支持选项") || msg.contains("header"),
        "{msg}"
    );
}
