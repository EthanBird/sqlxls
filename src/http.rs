use crate::args::Args;
use anyhow::{Context, Result};
use std::time::Duration;

const UA: &str = concat!("sqlxls/", env!("CARGO_PKG_VERSION"));

/// 跳过 HTTPS 证书校验：`insecure=true`（curl `-k`）或 `verify=false`。
pub fn tls_insecure(args: &Args) -> bool {
    if args.get_bool(99, &["insecure"]) {
        return true;
    }
    match args.get(99, &["verify", "ssl_verify", "tls_verify"]) {
        Some(v) => v.as_bool() == Some(false),
        None => false,
    }
}

pub fn client(insecure: bool) -> Result<reqwest::blocking::Client> {
    let mut b = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(UA);
    if insecure {
        eprintln!(
            "⚠️  已跳过 HTTPS 证书校验（insecure=true / verify=false）。只用于你信任的地址。"
        );
        b = b.danger_accept_invalid_certs(true);
    }
    b.build().context("无法创建 HTTP 客户端")
}

pub fn send_failed(err: anyhow::Error, url: &str) -> anyhow::Error {
    let msg = format!("{err:#}");
    if looks_like_tls(&msg) {
        err.context(format!(
            "请求失败: {url}\nHTTPS 证书校验未通过（自签、过期、公司代理中间人证书常见）。若你信任该地址，加上 insecure=true（或 verify=false），相当于 curl -k。"
        ))
    } else {
        err.context(format!("请求失败: {url}"))
    }
}

pub fn looks_like_tls(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("certificate")
        || m.contains("unknownissuer")
        || m.contains("unknown issuer")
        || m.contains("notvalidforname")
        || m.contains("webpki")
        || m.contains("pkix")
        || ((m.contains("tls") || m.contains("ssl"))
            && (m.contains("error") || m.contains("fail") || m.contains("invalid")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::{Args, Value};
    use std::collections::HashMap;

    fn named(pairs: &[(&str, Value)]) -> Args {
        Args {
            positional: vec![],
            named: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect::<HashMap<_, _>>(),
        }
    }

    #[test]
    fn default_verifies() {
        assert!(!tls_insecure(&named(&[])));
    }

    #[test]
    fn insecure_true() {
        assert!(tls_insecure(&named(&[("insecure", Value::Bool(true))])));
        assert!(!tls_insecure(&named(&[("insecure", Value::Bool(false))])));
    }

    #[test]
    fn verify_false() {
        assert!(tls_insecure(&named(&[("verify", Value::Bool(false))])));
        assert!(!tls_insecure(&named(&[("verify", Value::Bool(true))])));
        assert!(tls_insecure(&named(&[("ssl_verify", Value::Int(0))])));
    }

    #[test]
    fn tls_error_shape() {
        assert!(looks_like_tls("invalid peer certificate: UnknownIssuer"));
        assert!(!looks_like_tls("connection refused"));
    }
}
