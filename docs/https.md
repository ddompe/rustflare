# HTTPS and password

By default RustFlare listens on plain HTTP with no password: fine on your own Wi-Fi, not on a shared network. Two optional layers fix that. They also unlock the **Use this device's location** button, because browsers only share location over HTTPS.

## Why a private CA and not just a self-signed certificate

A bare self-signed certificate makes iOS show a warning on every visit and the Home Screen app and the location permission do not work reliably with it. Instead `rustflare-tls` creates a tiny private certificate authority and a server certificate signed by it. You tell the phone **once** to trust that CA; after that `https://astropi.local:8443` is a normal, warning-free, secure page. (A public certificate would need a real domain name; for a Pi on a field network a private CA is the practical choice. Tailscale's `tailscale cert` is an alternative if you already use Tailscale.)

## Set it up

On the Pi:

```
sudo rustflare-tls            # creates /var/lib/rustflare/tls/{ca,server}.crt and keys
sudo systemctl restart rustflare
```

The service now serves HTTPS on port 8443. Plain HTTP on port 8080 only offers the CA certificate and redirects everything else to HTTPS.

On the iPhone (once):

1. In Safari open `http://astropi.local:8080/rustflare-ca.crt` and allow the download.
2. Settings > **Profile Downloaded** > Install.
3. Settings > General > About > **Certificate Trust Settings** > switch on *RustFlare local CA*.
4. Open `https://astropi.local:8443`, then Share > Add to Home Screen again so the app uses the secure address.

If the Pi's name or IP changes, run `sudo rustflare-tls` again (add extra names or IPs as arguments); the CA stays the same, so the phone keeps trusting it.

## Password

Edit `/etc/default/rustflare`, set `RUSTFLARE_PASSWORD=...`, restart. The browser asks once (user name: anything). Use it together with HTTPS: over plain HTTP the password is not protected. The CA certificate stays downloadable without a password so a new phone can be set up.

This is access control for a telescope on a LAN, not a hardened internet service: do not expose the port to the internet.
