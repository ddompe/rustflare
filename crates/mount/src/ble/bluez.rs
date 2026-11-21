//! BlueZ (Linux) GATT backend over D-Bus, using the pure-Rust `zbus` crate (no libdbus, ADR 0015).
//!
//! Only the system bus and the `org.bluez` API are used: `Adapter1.StartDiscovery`, `Device1.Connect`,
//! `GattCharacteristic1.StartNotify/WriteValue` and `PropertiesChanged` signals for notifications. We never pair
//! or trust a device, and the device object is removed again on disconnect so nothing stays behind in BlueZ.

use super::{is_mount, BleDevice, Gatt, CHAR_UUID};
use rustflare_core::{Error, Result};
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

const BLUEZ: &str = "org.bluez";
/// Upper bound for any single D-Bus call; keeps the engine from hanging on a wedged BlueZ.
const CALL_TIMEOUT: Duration = Duration::from_secs(8);

type Props = HashMap<String, OwnedValue>;
type Objects = HashMap<OwnedObjectPath, HashMap<String, Props>>;

fn err(what: &str, e: impl std::fmt::Display) -> Error { Error::Mount(format!("Bluetooth: {what}: {e}")) }

fn system_bus() -> Result<Connection> {
    zbus::blocking::connection::Builder::system()
        .and_then(|b| b.method_timeout(CALL_TIMEOUT).build())
        .map_err(|e| err("cannot reach the system D-Bus (is bluetoothd installed and running?)", e))
}

fn objects(conn: &Connection) -> Result<Objects> {
    let p = Proxy::new(conn, BLUEZ, "/", "org.freedesktop.DBus.ObjectManager").map_err(|e| err("object manager", e))?;
    p.call("GetManagedObjects", &()).map_err(|e| err("listing BlueZ objects (is the bluetooth service running?)", e))
}

fn adapter_path(objs: &Objects) -> Result<OwnedObjectPath> {
    let mut v: Vec<String> = objs.iter().filter(|(_, i)| i.contains_key("org.bluez.Adapter1")).map(|(p, _)| p.to_string()).collect();
    v.sort();
    v.into_iter().next().and_then(|p| OwnedObjectPath::try_from(p).ok()).ok_or_else(|| Error::Mount("Bluetooth: no Bluetooth adapter found".into()))
}

fn prop<T: TryFrom<OwnedValue>>(p: &Props, k: &str) -> Option<T> { p.get(k).and_then(|v| v.try_clone().ok()).and_then(|v| T::try_from(v).ok()) }

/// BlueZ object path of a device: `<adapter>/dev_AA_BB_...`.
pub fn device_path(adapter: &str, address: &str) -> String { format!("{adapter}/dev_{}", address.to_uppercase().replace(':', "_")) }

fn call0(conn: &Connection, path: &str, iface: &str, method: &str) -> zbus::Result<()> {
    let p = Proxy::new(conn, BLUEZ, path.to_string(), iface.to_string())?;
    p.call::<_, _, ()>(method, &())
}

/// Lists nearby mounts, scanning for about `duration`.
pub fn scan(duration: Duration) -> Result<Vec<BleDevice>> {
    let conn = system_bus()?;
    let adapter = adapter_path(&objects(&conn)?)?.to_string();
    // LE only; failure to set the filter is harmless
    let filter: HashMap<&str, Value> = HashMap::from([("Transport", Value::from("le"))]);
    if let Ok(p) = Proxy::new(&conn, BLUEZ, adapter.clone(), "org.bluez.Adapter1") {
        let _ = p.call::<_, _, ()>("SetDiscoveryFilter", &(filter,));
    }
    let started = call0(&conn, &adapter, "org.bluez.Adapter1", "StartDiscovery");
    if let Err(e) = &started {
        if !e.to_string().contains("InProgress") {
            return Err(err("starting the scan", e));
        }
    }
    std::thread::sleep(duration);
    let objs = objects(&conn);
    if started.is_ok() {
        let _ = call0(&conn, &adapter, "org.bluez.Adapter1", "StopDiscovery");
    }
    let mut out: Vec<BleDevice> = objs?
        .values()
        .filter_map(|i| i.get("org.bluez.Device1"))
        .filter_map(|d| {
            let name: String = prop(d, "Name").or_else(|| prop(d, "Alias")).unwrap_or_default();
            let uuids: Vec<String> = prop(d, "UUIDs").unwrap_or_default();
            let rssi: Option<i16> = prop(d, "RSSI");
            // only devices heard in this scan (BlueZ keeps stale cache entries without a signal strength)
            if rssi.is_none() || !is_mount(&name, &uuids) {
                return None;
            }
            Some(BleDevice { address: prop(d, "Address")?, name, rssi })
        })
        .collect();
    out.sort_by(|a, b| b.rssi.cmp(&a.rssi));
    Ok(out)
}

enum Event {
    Data(Vec<u8>),
    Lost,
}

pub struct BluezGatt {
    conn: Connection,
    adapter: String,
    dev: String,
    chr: String,
    rx: Receiver<Event>,
    alive: Arc<AtomicBool>,
    closed: bool,
}

impl BluezGatt {
    /// Connects to `address` (scanning for it first if BlueZ does not know it yet) and enables notifications.
    pub fn connect(address: &str, timeout: Duration) -> Result<Self> {
        let deadline = Instant::now() + timeout;
        let conn = system_bus()?;
        let objs = objects(&conn)?;
        let adapter = adapter_path(&objs)?.to_string();
        let dev = device_path(&adapter, address);
        let known = |o: &Objects| o.keys().any(|k| k.as_str() == dev);

        if !known(&objs) {
            let started = call0(&conn, &adapter, "org.bluez.Adapter1", "StartDiscovery").is_ok();
            let scan_end = deadline.min(Instant::now() + Duration::from_secs(12));
            let mut found = false;
            while !found && Instant::now() < scan_end {
                std::thread::sleep(Duration::from_millis(500));
                found = objects(&conn).map(|o| known(&o)).unwrap_or(false);
            }
            if started {
                let _ = call0(&conn, &adapter, "org.bluez.Adapter1", "StopDiscovery");
            }
            if !found {
                return Err(Error::Mount(
                    "Bluetooth: the mount was not found. Press its pairing button, close the phone app, and keep the mount close".into(),
                ));
            }
        }

        let mut me = BluezGatt { conn: conn.clone(), adapter, dev: dev.clone(), chr: String::new(), rx: channel().1, alive: Arc::new(AtomicBool::new(false)), closed: false };
        match me.finish_connect(deadline) {
            Ok(()) => Ok(me),
            Err(e) => {
                me.disconnect();
                Err(e)
            }
        }
    }

    fn finish_connect(&mut self, deadline: Instant) -> Result<()> {
        call0(&self.conn, &self.dev, "org.bluez.Device1", "Connect").map_err(|e| err("connecting", e))?;
        self.alive.store(true, Ordering::SeqCst);
        // wait for GATT service discovery
        let devp = Proxy::new(&self.conn, BLUEZ, self.dev.clone(), "org.bluez.Device1").map_err(|e| err("device", e))?;
        loop {
            if devp.get_property::<bool>("ServicesResolved").unwrap_or(false) {
                break;
            }
            if Instant::now() > deadline {
                return Err(Error::Mount("Bluetooth: connected but the mount's services were not discovered in time".into()));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let objs = objects(&self.conn)?;
        let want = CHAR_UUID;
        self.chr = objs
            .iter()
            .filter(|(p, _)| p.as_str().starts_with(&format!("{}/", self.dev)))
            .find(|(_, i)| i.get("org.bluez.GattCharacteristic1").and_then(|c| prop::<String>(c, "UUID")).is_some_and(|u| u.eq_ignore_ascii_case(want)))
            .map(|(p, _)| p.to_string())
            .ok_or_else(|| Error::Mount("Bluetooth: this device does not have the mount's control characteristic (ff01)".into()))?;

        // subscribe before StartNotify so no early notification is missed
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(BLUEZ)
            .map_err(|e| err("match rule", e))?
            .interface("org.freedesktop.DBus.Properties")
            .map_err(|e| err("match rule", e))?
            .member("PropertiesChanged")
            .map_err(|e| err("match rule", e))?
            .path_namespace(ObjectPath::try_from(self.dev.as_str()).map_err(|e| err("path", e))?)
            .map_err(|e| err("match rule", e))?
            .build();
        let iter = MessageIterator::for_match_rule(rule, &self.conn, Some(64)).map_err(|e| err("subscribing to notifications", e))?;
        let (tx, rx) = channel();
        self.rx = rx;
        let (chr, dev, alive) = (self.chr.clone(), self.dev.clone(), self.alive.clone());
        std::thread::Builder::new()
            .name("ble-notify".into())
            .spawn(move || {
                for msg in iter {
                    let Ok(msg) = msg else { continue };
                    let hdr = msg.header();
                    let path = hdr.path().map(|p| p.to_string()).unwrap_or_default();
                    let Ok((iface, changed, _inv)) = msg.body().deserialize::<(String, Props, Vec<String>)>() else { continue };
                    if path == chr && iface == "org.bluez.GattCharacteristic1" {
                        if let Some(v) = prop::<Vec<u8>>(&changed, "Value") {
                            let _ = tx.send(Event::Data(v));
                        }
                    } else if path == dev && iface == "org.bluez.Device1" && prop::<bool>(&changed, "Connected") == Some(false) {
                        alive.store(false, Ordering::SeqCst);
                        let _ = tx.send(Event::Lost);
                        return; // after a disconnect this connection's job is done
                    }
                }
                alive.store(false, Ordering::SeqCst);
                let _ = tx.send(Event::Lost);
            })
            .map_err(|e| err("thread", e))?;
        call0(&self.conn, &self.chr, "org.bluez.GattCharacteristic1", "StartNotify").map_err(|e| err("enabling notifications", e))?;
        Ok(())
    }
}

impl Gatt for BluezGatt {
    fn write(&mut self, data: &[u8]) -> io::Result<()> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "Bluetooth link lost"));
        }
        let p = Proxy::new(&self.conn, BLUEZ, self.chr.clone(), "org.bluez.GattCharacteristic1").map_err(io::Error::other)?;
        let opts: HashMap<&str, Value> = HashMap::from([("type", Value::from("request"))]);
        p.call::<_, _, ()>("WriteValue", &(data.to_vec(), opts)).map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e.to_string()))
    }

    fn recv(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        match self.rx.recv_timeout(timeout) {
            Ok(Event::Data(v)) => Ok(Some(v)),
            Ok(Event::Lost) | Err(RecvTimeoutError::Disconnected) => Err(io::Error::new(io::ErrorKind::ConnectionAborted, "Bluetooth link lost")),
            Err(RecvTimeoutError::Timeout) => Ok(None),
        }
    }

    fn disconnect(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.alive.store(false, Ordering::SeqCst);
        if !self.chr.is_empty() {
            let _ = call0(&self.conn, &self.chr, "org.bluez.GattCharacteristic1", "StopNotify");
        }
        let _ = call0(&self.conn, &self.dev, "org.bluez.Device1", "Disconnect");
        // forget the (never bonded) device object so nothing is left behind in BlueZ
        if let Ok(p) = Proxy::new(&self.conn, BLUEZ, self.adapter.clone(), "org.bluez.Adapter1") {
            if let Ok(path) = ObjectPath::try_from(self.dev.as_str()) {
                let _ = p.call::<_, _, ()>("RemoveDevice", &(path,));
            }
        }
    }
}

impl Drop for BluezGatt {
    fn drop(&mut self) { self.disconnect(); }
}

#[cfg(test)]
mod tests {
    #[test]
    fn device_path_format() {
        assert_eq!(super::device_path("/org/bluez/hci0", "aa:bb:cc:dd:ee:01"), "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_01");
    }
}
