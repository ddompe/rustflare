# 16. Optional HTTPS (private CA) and password

Date: 2026-11-21
Status: accepted

## Context

The server had no access control and only plain HTTP. Browsers (iOS in particular) only share geolocation over a secure context, and a password over plain HTTP is not protected. The target is a Pi on a field Wi-Fi, used from an iPhone Home Screen app.

## Decision

- **Password**: optional (`RUSTFLARE_PASSWORD`), enforced by a middleware using HTTP Basic (any user name), compared in constant time. Basic needs no login page, works for the `<img>` live view and the WebSocket, and the browser remembers it.
- **HTTPS**: optional; enabled when `server.crt`/`server.key` exist in the TLS folder. `rustflare-tls` creates a private CA and a server certificate signed by it (SAN: host names and IPs, 800 days because iOS rejects more than 825). rustls with the `ring` provider (no C toolchain beyond what cross-compiling already uses). The plain-HTTP port then only serves the CA certificate and redirects the rest, so a phone can bootstrap trust.
- A bare self-signed certificate was rejected: iOS warns on every visit and location permission is unreliable with it. A public certificate needs a domain and internet access in the field.

## Consequences

- Default behaviour is unchanged (plain HTTP, no password).
- The phone must trust the CA once (documented in docs/guide/phone-setup.md).
- Not an internet-facing design: no rate limiting, one shared password.
