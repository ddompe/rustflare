# ZWO AM3 Bluetooth (BLE) protocol notes

*Unofficial. Worked out from a packet capture of ZWO's own iPhone app talking to an AM3 and from read-only tests with a Raspberry Pi. Not affiliated with or endorsed by ZWO. Verified on **one** mount (AM3, firmware 1.8.8) only; other mounts (AM3N, AM5, AM5N) probably behave the same but nobody has checked.*

**Summary.** The mount's Bluetooth Low Energy link is a plain *serial-over-BLE bridge*: the **same ASCII `:…#` command set** that the mount speaks over its USB serial port (see ZWO's *Serial Communication Protocol* v1.8 and INDIGO's `indigo_mount_asi` driver) is written to one GATT characteristic, and replies come back as notifications on the same characteristic. There is no BLE bonding or link encryption. The only extra step is a **session-opening command** (`:BS20240222071513#`) that the mount must receive before it answers anything else.

## 1. GATT layout

| UUID | Kind | Flags | Notes |
|---|---|---|---|
| `00001800-…` / `00001801-…` | standard GAP / GATT | | Generic Access, Service Changed (indicate) |
| `000000ff-0000-1000-8000-00805f9b34fb` | service | | **control service** |
| `0000ff01-0000-1000-8000-00805f9b34fb` | characteristic | read, write, notify | **commands go here; replies come back here** |
| `414d0001-6ecf-49f1-a13f-f5c2d9174f9f` | service | | vendor service (`41 4D` = "AM") |
| `414d0002-6ecf-49f1-a13f-f5c2d9174f9f` | characteristic | read, write, notify | not used for control in the captured session; reading it (and `ff01`) returns the placeholder `de ed be ef` |

- Connecting and listing the services needs **no pairing**. A standard BLE pairing request is answered by the mount hanging up, so do not try to bond.
- **Enable notifications first**: write `01 00` to the Client Characteristic Configuration descriptor of `ff01` (the handle after the characteristic's value handle). The official app also enables indications on Service Changed.
- Commands are sent as **ATT Write Requests (write *with* response)** to `ff01`. One command per write (commands of 3 to 30 bytes were seen).
- Replies arrive as **notifications on `ff01`**, 1 to 59 bytes each (the longest is the `:GAST#` reply). Because 59-byte replies arrive in one piece, the ATT MTU is larger than the default 23; the MTU exchange itself was not visible in the capture. Every reply that has a terminator ends with `#`.

## 2. Connecting

- The mount accepts **one** BLE central at a time. While the phone app holds the link, the mount does not advertise, and other devices cannot find it. Close the app (and switch the phone's Bluetooth off if it keeps reconnecting).
- In our test the mount became findable and connectable again only after its **pairing button** was pressed *and* the phone app was closed; we did not separate the two causes. The official app asks for the button press the first time it meets a mount; in the captured session (already set up) it connected without it.
- The advertised name is `AM3_` followed by the mount's name (e.g. `AM3_<yourname>`).

## 3. Session opener

Before any other command is answered, send:

```
:BS20240222071513#      →  OK#
```

The 14-digit token **looks like a timestamp (`YYYYMMDDHHMMSS`, here 2024-02-22 07:15:13)**. The official app sent exactly this value in the captured session, and it is the same for every device we could compare (one), so it appears to be an application constant rather than a per-device secret. Without the opener the mount ignores ordinary queries (a `:GVP#` sent first got no reply; after the opener it was answered immediately). It is unknown whether newer app versions use a different value.

## 4. Commands seen in the capture

Same syntax as the USB serial protocol: `:` + command + `#`. Replies are shown as the mount sent them; `<…>` marks fields that depend on the mount or the site.

### Queries (reply)

| Command | Reply | Meaning |
|---|---|---|
| `:GVP#` | `AM3#` | product / model |
| `:GV#` | `1.8.8#` | firmware version |
| `:GMA#` | `<12 hex digits>#` | the mount's Bluetooth address |
| `:GBu#` | `0#` | unknown (possibly buzzer) |
| `:GMGE#` | `<lat>&<lon>#` | stored site: `±dd*mm:ss&±ddd*mm:ss`; longitude is **west-positive** (LX200 style) |
| `:GMTI#` | `MM/DD/YY&hh:mm:ss&±hh:mm#` | stored local date, time and UTC offset (**sign inverted**: a site at UTC−6 was sent as `+06:00`) |
| `:GAST#` | `<date&time&offset>#<RA&Dec>#<flags>#` | **everything in one reply**, polled once a second by the app (see §5) |
| `:GU#` | `<flags>#` | status flags only (e.g. `NGM010400281#`) |
| `:GAT#` | `1#` | tracking state (`1` after tracking was enabled) |
| `:GT#` | `0#` / `1#` / `2#` | tracking rate: 0 sidereal, 1 lunar, 2 solar (inferred from the order of calls in the capture, and consistent with `:TQ#`, `:TL#`, `:TS#`) |
| `:GRl#` | `1440#` | maximum slew rate as a multiple of sidereal |
| `:GMeq#` | `<RA&Dec>#` | stored equatorial target (`hh:mm:ss&±dd*mm:ss`) |

### Set commands (single-character reply `1` = accepted, `0` = refused, no `#`)

| Command | Reply | Meaning |
|---|---|---|
| `:SMGE<lat>&<lon>#` | `1` | set the site (the app syncs it on connect) |
| `:SMTI<date&time&offset>#` | `1` | set date, time and UTC offset (the app syncs it on connect) |
| `:SMeq<RA>&<Dec>#` | `0` | set the equatorial target (the reply was `0` in the capture) |
| `:Te#` / `:Td#` | `1` | tracking on / off |

### Motion and rates (no direct reply; the next `:GAST#` reply shows the effect)

| Command | Meaning |
|---|---|
| `:R<n>#` | select slew rate (the app used `6` and `9`; `9` is the fastest) |
| `:MT<dir><n>#` | start moving (`<dir>` = `n`, `e`, `s`, `w`); the app sends it after `:R9#` and always followed it with the matching stop. The meaning of the trailing digit (`3` in the capture) is not established |
| `:Q<dir>#` | stop that direction |
| `:Q#` | stop everything |
| `:hC#` | go to the home (zero) position; the mount slews there and stops tracking |
| `:TQ#` / `:TL#` / `:TS#` | tracking rate: sidereal / lunar / solar (no reply, per INDIGO) |

For comparison, over USB the RustFlare driver uses `:R<id>#` followed by `:M<dir>#` for slewing and `:Mg<dir><ms>#` pulses with `:Rg<rate>#` for guiding; those worked on this firmware over USB. Whether the BLE link accepts them the same way has not been tested.

## 5. The `:GAST#` reply

```
11/21/26&15:20:38&+06:00#01:47:36&+90*00:00#nNHGM040400880#
└─ local date & time & UTC offset ─┘└─ RA & Dec ─────────┘└─ flags ─┘
```

- **RA & Dec** is the current pointing. While tracking is off the RA field increases by one second per second (the sky moves under a fixed mount), as expected. At the home position the Dec reads `+90*00:00`.
- **Flags** (letters confirmed from INDIGO's driver and the capture): **`n` present = tracking is OFF**; **`N` present = not slewing**; **`H` = at home**; `Z` = alt-azimuth mode. `G` and `M` were present in every sample, and the digits after them change while slewing; their meaning is unknown.

## 6. A tested recipe (BlueZ `bluetoothctl`)

This is what was used from a Raspberry Pi; the exact bytes are ASCII hex.

```
power on
scan on                                   # until the mount's AM3_… name appears (mount in pairing mode, phone app closed)
connect <address>
menu gatt
select-attribute <device path>/service0028/char0029    # the 0000ff01 characteristic (BlueZ names may differ)
notify on
write "<hex of :BS20240222071513#>" 0 request    # → notification "OK#"
write "0x3a 0x47 0x56 0x50 0x23" 0 request   # ":GVP#" → "AM3#"
notify off
back
disconnect <address>
```

Replies show up as `[CHG] Attribute … Value:` lines followed by a hex dump. Always disconnect afterwards: a lingering connection keeps the phone app out.

## 7. How this was captured

1. Install Apple's *Bluetooth for iOS* logging profile on an iPhone and restart it; connect the phone to a Mac; open PacketLogger (from Apple's *Additional Tools for Xcode*): **File → New iOS Trace**.
2. In the ZWO app: connect to the mount, wait, disconnect, reconnect, switch tracking off/on, make one tiny slew, send the mount home.
3. Save the `.pklg` trace. Its records are little-endian: `length (u32) | timestamp (u32 s, u32 µs) | type (u8) | payload`, with HCI ACL data as types 2 (host to controller) and 3 (controller to host). Rebuild L2CAP (channel 4 = ATT) per connection handle and read the ATT Write Requests and Handle Value Notifications.

## 8. Not known / not tested

- The meaning of the trailing digit in `:MT<dir><n>#`, of the `G`/`M`/digit part of the flags, of `:GBu#`, and of the vendor service `414d…`.
- Whether the BLE link accepts the USB-style slew and guide commands; whether more than one client can be bound; what the pairing-button step stores in the mount; behaviour on other firmware versions and other ZWO mounts.
- The mount also has Wi-Fi. INDIGO's driver talks to it with the same text protocol over **TCP port 4030** (`asi://<host>` in INDIGO), which would be a simpler transport than BLE if the mount is on the network. Not tested here.

## 9. Safety

`:MT…#`, `:hC#`, `:R…#`+`:M…#` and `:Te#`/`:Td#` **move or stop the mount**. Never send them without watching the mount, and always follow a move with its stop. `:SM…#` commands overwrite stored site, time or target. Everything above was observed on one mount with the owner present; use it at your own risk.

## References

- ZWO, *ZWO Mount Serial Communication Protocol* v1.8 (linked from INDIGO's `mount_asi` driver documentation).
- INDIGO's ZWO mount driver: <https://github.com/indigo-astronomy/indigo/blob/master/indigo_drivers/mount_asi/indigo_mount_asi.c>.
- RustFlare's USB driver: `crates/mount/src/zwo.rs`; the decision record: [ADR 0007](../adr/0007-zwo-mount-serial-driver.md); the tracking issue for a BLE transport is on the project board.
