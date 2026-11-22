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
        let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host).to_string();
        let path = req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/").to_string();
        if path == "/" || path.starts_with("/?") {
            // A landing page instead of a blind redirect: a phone that does not trust the CA yet would show nothing at
            // all after the redirect. The page tests whether HTTPS works from this browser and either continues there or
            // explains the three steps.
            return ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-store")], landing_page(&host, https_port)).into_response();
        }
        (StatusCode::TEMPORARY_REDIRECT, [(header::LOCATION, format!("https://{host}:{https_port}{path}"))]).into_response()
    })
}

/// The plain-HTTP landing page (also useful on its own: it has the certificate download).
pub fn landing_page(host: &str, https_port: u16) -> String {
    let host: String = host.chars().filter(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(*c)).collect();
    format!(r##"<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>RustFlare</title>
<style>body{{font:17px/1.5 -apple-system,system-ui,sans-serif;background:#0d0c0b;color:#eee;margin:0;padding:1.2rem;max-width:34rem}}h1{{color:#ff7a3d;margin:.2rem 0}}a.b{{display:block;background:#ff7a3d;color:#1a0b00;padding:.9rem 1rem;border-radius:10px;text-align:center;font-weight:700;text-decoration:none;margin:.8rem 0}}li{{margin:.5rem 0}}.m{{color:#aaa}}code{{background:#222;padding:.1rem .3rem;border-radius:4px}}</style>
<h1>RustFlare</h1><p id="st" class="m">Checking the secure connection&hellip;</p>
<a class="b" href="https://{host}:{https_port}/">Open RustFlare (https)</a>
<div id="help" style="display:none"><p><b>This phone does not trust the RustFlare certificate yet.</b> Do these once:</p><ol>
<li><a href="/rustflare-ca.crt">Download the certificate</a> and tap Allow.</li>
<li>Settings &rsaquo; <b>Profile Downloaded</b> &rsaquo; Install.</li>
<li>Settings &rsaquo; General &rsaquo; About &rsaquo; <b>Certificate Trust Settings</b> &rsaquo; switch on <b>RustFlare local CA</b>. <span class="m">(Installing is not enough: this switch is the step that is usually missed.)</span></li>
<li>Come back to this page: it continues by itself.</li></ol></div>
<script>fetch("https://{host}:{https_port}/rustflare-probe",{{mode:"no-cors",cache:"no-store"}}).then(function(){{st.textContent="Secure connection works, opening…";location.replace("https://{host}:{https_port}/")}}).catch(function(){{st.textContent="Secure connection failed from this browser.";help.style.display="block"}})</script>"##)
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

    #[test]
    fn landing_page_carries_the_secure_address_and_cleans_the_host() {
        let p = landing_page("astropi.local\"><script>", 8443);
        assert!(p.contains("https://astropi.local:8443/") || p.contains("astropi.local"), "{p}");
        assert!(!p.contains("\"><script>"), "the host must not be able to inject markup");
        assert!(p.contains("/rustflare-ca.crt") && p.contains("Certificate Trust Settings"));
    }
}
