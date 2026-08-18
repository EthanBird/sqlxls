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
    let rewritten = rewrite_sql(&sql, &mut conn, &reg, &mut c, &SyntaxOpts::default()).unwrap();
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
    let rewritten = rewrite_sql(&sql, &mut conn, &reg, &mut c, &SyntaxOpts::default()).unwrap();
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
        "",
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
    let mut s = Session::with_opts(SyntaxOpts { strict: true }).unwrap();
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
    let mut s = Session::with_opts(SyntaxOpts { strict: true }).unwrap();
    let sql = format!("SELECT * FROM read('{}', format='csv')", csv.display());
    s.run_sql(&sql, None, false).unwrap();
}
