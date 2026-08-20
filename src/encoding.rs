//! 文本解码：UTF-8 默认；GBK / GB18030 等必须显式 `encoding=` 或 HTTP `charset=`。

use anyhow::{bail, Result};

pub fn trim_utf8_bom(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    }
}

pub fn charset_from_content_type(ct: &str) -> Option<String> {
    let ct = ct.to_ascii_lowercase();
    for part in ct.split(';').skip(1) {
        let part = part.trim();
        let Some(v) = part.strip_prefix("charset=") else {
            continue;
        };
        let v = v.trim().trim_matches('"').trim_matches('\'');
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

pub fn resolve_encoding(explicit: Option<&str>, content_type: &str) -> Option<String> {
    explicit
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| charset_from_content_type(content_type))
}

pub fn decode_bytes(bytes: &[u8], encoding: Option<&str>) -> Result<String> {
    let trimmed = trim_utf8_bom(bytes);
    let label = encoding.map(str::trim).filter(|s| !s.is_empty());
    match label {
        None => strict_utf8(trimmed),
        Some(label) if is_utf8_label(label) => strict_utf8(trimmed),
        Some(label) => {
            let Some(enc) = encoding_rs::Encoding::for_label(label.as_bytes()) else {
                bail!(
                    "未知 encoding='{label}'。常用: utf-8、gbk、gb2312、gb18030、big5、utf-16le、utf-16be"
                );
            };
            if enc == encoding_rs::UTF_8 {
                return strict_utf8(trimmed);
            }
            let (cow, _, _) = enc.decode(trimmed);
            Ok(cow.into_owned())
        }
    }
}

pub fn decode_http(bytes: &[u8], encoding: Option<&str>, content_type: &str) -> Result<String> {
    let resolved = resolve_encoding(encoding, content_type);
    decode_bytes(bytes, resolved.as_deref()).map_err(|e| {
        if encoding.is_none() && charset_from_content_type(content_type).is_none() {
            e.context("远程文本解码失败")
        } else {
            e
        }
    })
}

fn is_utf8_label(label: &str) -> bool {
    matches!(
        label.to_ascii_lowercase().as_str(),
        "utf-8" | "utf8" | "utf-8-sig" | "utf8-sig" | "ascii" | "us-ascii"
    )
}

fn strict_utf8(bytes: &[u8]) -> Result<String> {
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => bail!(
            "文本不是合法 UTF-8。中文 GBK/GB18030 请写 encoding='gbk'，或让 HTTP Content-Type 带 charset=gbk"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gbk_roundtrip() {
        let (bytes, _, _) = encoding_rs::GBK.encode("姓名,数量\n张三,1\n");
        let text = decode_bytes(&bytes, Some("gbk")).unwrap();
        assert!(text.contains("张三"), "{text}");
        let err = decode_bytes(&bytes, None).unwrap_err();
        assert!(format!("{err:#}").contains("encoding='gbk'"), "{err:#}");
    }

    #[test]
    fn charset_header() {
        assert_eq!(
            charset_from_content_type("text/csv; charset=GBK"),
            Some("gbk".into())
        );
        let (bytes, _, _) = encoding_rs::GBK.encode("a,b\n1,2\n");
        let text = decode_http(&bytes, None, "text/csv; charset=gbk").unwrap();
        assert!(text.starts_with("a,b"));
    }
}
