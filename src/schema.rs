use std::collections::HashSet;

/// 清洗列名：空列补 `col_N`，重复列加后缀，去掉首尾空白。
pub fn unique_column_names<I, S>(raw: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (i, name) in raw.into_iter().enumerate() {
        let trimmed = name.as_ref().trim();
        let base = if trimmed.is_empty() {
            format!("col_{}", i)
        } else {
            trimmed.to_string()
        };
        let mut safe = base.clone();
        let mut n = 1;
        while seen.contains(&safe) {
            safe = format!("{}_{}", base, n);
            n += 1;
        }
        seen.insert(safe.clone());
        out.push(safe);
    }
    out
}

pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
