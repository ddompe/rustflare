# Field networking: the Pi as a Wi-Fi hotspot

Out in a field there may be no Wi-Fi. `rustflare-hotspot` turns the Pi into an access point your phone joins directly.

**The Pi has one Wi-Fi radio.** While the hotspot is on, the Pi is *not* on your home network (and an ssh session over Wi-Fi ends). That is why `up` needs `--yes` and switches back by itself after a time limit unless you ask it to stay.

## Set up (once, at home, on the Pi)

```
sudo rustflare-hotspot create --ssid RustFlare --password 'a long password'   # password >= 8 characters; omit it to get a random one
```

This only defines the profile (WPA2, `10.42.0.1` for the Pi, addresses for phones from NetworkManager's built-in DHCP). Nothing is switched on.

## Use it

```
sudo rustflare-hotspot up --yes                 # on for 30 minutes, then back to the previous Wi-Fi automatically
sudo rustflare-hotspot up --yes --minutes 240   # longer
sudo rustflare-hotspot up --yes --keep          # stays on until you say otherwise
sudo rustflare-hotspot down                     # off now, back to the previous Wi-Fi
sudo rustflare-hotspot status
```

On the iPhone: Settings > Wi-Fi > join *RustFlare*, then open `http://10.42.0.1:8080` (or `https://10.42.0.1:8443` if you set up HTTPS; `rustflare-tls` includes the Pi's addresses, and you can pass `10.42.0.1` to it as an extra name). `http://astropi.local:8080` usually works too.

With `--keep` there is nothing to bring the Pi back to your home network except `down` or a reboot (the profile never auto-connects), so run it from the phone's ssh app or keep the default time limit while you are testing.

## Safety design

- `up` refuses without `--yes`, and `--dry-run` shows every command first.
- The automatic way back (`systemd-run` timer that runs `rustflare-hotspot down`) is scheduled **before** the switch, so a failure while switching cannot strand the Pi.
- The previous network name is remembered in `/var/lib/rustflare/hotspot.previous`.
- The profile has `autoconnect no`: after a reboot the Pi returns to its normal Wi-Fi.

## Limits

- 2.4 GHz only (`band bg`) for range; no internet for the phone through the hotspot.
- Needs NetworkManager (Raspberry Pi OS Bookworm and newer) with `dnsmasq-base` (installed by default).
