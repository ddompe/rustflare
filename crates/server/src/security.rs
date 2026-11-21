//! Optional access control and HTTPS.
//!
//! * Password: if `RUSTFLARE_PASSWORD` is set every request needs HTTP Basic credentials with that password (any
//!   user name). The browser caches them, and the live-view image and WebSocket reuse them.
//! * HTTPS: if `--tls-dir` holds `server.crt` and `server.key` the server also listens on `--tls-bind`. The plain-HTTP
//!   port then only serves the CA certificate (`/rustflare-ca.crt`, so a phone can trust it once) and redirects
//!   everything else to HTTPS. `rustflare-tls` creates the CA and server certificate.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Constant-time comparison, so response timing does not reveal how much of a guess was right.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    let mut d = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        d |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0xff);
    }
    d == 0
}

/// Does this `Authorization` header carry the password (HTTP Basic, user name ignored)?
pub fn basic_ok(header_value: Option<&str>, password: &str) -> bool {
    let Some(v) = header_value.and_then(|v| v.strip_prefix("Basic ")) else { return false };
    let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(v.trim()) else { return false };
    let Some(pos) = raw.iter().position(|b| *b == b':') else { return false };
    same(&raw[pos + 1..], password.as_bytes())
}

/// Middleware: demand the password (when one is configured). The CA certificate is always public.
pub async fn require_password(axum::extract::State(pw): axum::extract::State<Arc<String>>, req: Request, next: Next) -> Response {
    if pw.is_empty() || req.uri().path() == "/rustflare-ca.crt" {
        return next.run(req).await;
    }
    let ok = basic_ok(req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()), &pw);
    if ok {
        return next.run(req).await;
    }
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::WWW_AUTHENTICATE, HeaderValue::from_static("Basic realm=\"RustFlare\", charset=\"UTF-8\""))
        .body(Body::from("password required"))
        .unwrap()
}

pub struct Tls {
    pub cert: PathBuf,
    pub key: PathBuf,
    pub ca: PathBuf,
}

/// The TLS files in `dir`, if the server certificate and key are there.
pub fn tls_files(dir: &Path) -> Option<Tls> {
    let t = Tls { cert: dir.join("server.crt"), key: dir.join("server.key"), ca: dir.join("ca.crt") };
    (t.cert.is_file() && t.key.is_file()).then_some(t)
}

/// The plain-HTTP router used when HTTPS is on: serves the CA, redirects the rest.
pub fn http_redirector(ca: PathBuf, https_port: u16) -> axum::Router {
    use axum::routing::get;
    axum::Router::new().route("/rustflare-ca.crt", get(move || {
        let ca = ca.clone();
        async move {
            match std::fs::read(&ca) {
                Ok(b) => ([(header::CONTENT_TYPE, "application/x-x509-ca-cert"), (header::CONTENT_DISPOSITION, "attachment; filename=\"rustflare-ca.crt\"")], b).into_response(),
                Err(_) => StatusCode::NOT_FOUND.into_response(),
            }
        }
    }))
    .fallback(move |req: Request| async move {
        let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("astropi.local");
        let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
        let loc = format!("https://{host}:{https_port}{}", req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/"));
        (StatusCode::TEMPORARY_REDIRECT, [(header::LOCATION, loc)]).into_response()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic(user: &str, pw: &str) -> String {
        format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pw}")))
    }

    #[test]
    fn password_check() {
        assert!(basic_ok(Some(&basic("anyone", "s3cret")), "s3cret"));
        assert!(basic_ok(Some(&basic("", "a:b")), "a:b"), "colons in the password are fine");
        assert!(!basic_ok(Some(&basic("x", "s3cre")), "s3cret"));
        assert!(!basic_ok(Some(&basic("x", "s3cret!")), "s3cret"));
        assert!(!basic_ok(Some("Bearer abc"), "s3cret"));
        assert!(!basic_ok(Some("Basic !!!"), "s3cret"));
        assert!(!basic_ok(None, "s3cret"));
    }

    #[test]
    fn constant_time_compare_is_correct() {
        assert!(same(b"abc", b"abc") && !same(b"abc", b"abd") && !same(b"abc", b"ab") && !same(b"", b"a"));
    }
}
