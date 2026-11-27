# Phone setup

By default RustFlare uses plain HTTP and no password. That is fine on your own Wi-Fi. This page covers the optional extras: HTTPS, a password and a Home Screen app.

## Home Screen app

On an iPhone, open `http://<pi-name>.local:8080` in Safari, tap Share, then *Add to Home Screen*. The page opens full screen like an app. Set Auto-Lock to Never for the session so the screen stays on.

## HTTPS and trusting the certificate

HTTPS unlocks the **Use this device's location** button and makes the Home Screen app work reliably. Over plain `http://` the browser refuses to share the phone's location (iOS and Chrome both do), so RustFlare does not show the button there: you type the latitude and longitude instead, and the page tells you that HTTPS is the way to get the button.

A bare self-signed certificate makes iOS warn on every visit. So `rustflare-tls` creates a small private certificate authority and a server certificate signed by it. You tell the phone once to trust that authority, and after that `https://<pi-name>.local:8443` is a normal, warning-free page.

On the Pi:

```sh
sudo rustflare-tls
sudo systemctl restart rustflare
```

The service now serves HTTPS on port 8443. Plain HTTP on port 8080 only offers the certificate and a landing page that walks you through the steps.

On the iPhone, once:

1. In Safari open `http://<pi-name>.local:8080/rustflare-ca.crt` and allow the download.
2. Settings > **Profile Downloaded** > Install.
3. Settings > General > About > **Certificate Trust Settings** > switch on *RustFlare local CA*.
4. Open `https://<pi-name>.local:8443`, then Share > Add to Home Screen again so the app uses the secure address.

If the Pi's name or IP changes, run `sudo rustflare-tls` again (you can add extra names or IPs as arguments). The authority stays the same, so the phone keeps trusting it.

## Password

Edit `/etc/default/rustflare`, set `RUSTFLARE_PASSWORD=...` and restart the service. The browser asks once (the user name can be anything). Use it together with HTTPS: over plain HTTP the password is not protected. The certificate stays downloadable without a password so a new phone can be set up.

This is access control for a telescope on a LAN, not a hardened internet service. Do not expose the port to the internet.

See also [Configuration](../reference/configuration.md).
