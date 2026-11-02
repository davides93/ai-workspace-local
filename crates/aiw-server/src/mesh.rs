//! Device mesh on the LAN: announces this device and discovers the others through mDNS
//! (`_aiw._tcp.local.`), and builds the pairing QR code that a phone camera can open.

use aiw_engine::Engine;
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, OnceLock};

pub const SERVICE: &str = "_aiw._tcp.local.";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Discovered {
    pub id: String,
    pub name: String,
    pub os: String,
    pub tier: String,
    pub version: String,
    /// `http://ip:port` of its API.
    pub address: String,
    pub last_seen: String,
}

struct Mesh {
    _daemon: ServiceDaemon,
    seen: Arc<Mutex<BTreeMap<String, Discovered>>>,
}

static MESH: OnceLock<Mesh> = OnceLock::new();

pub(crate) fn host_label(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "aiw".into()
    } else {
        s.chars().take(40).collect()
    }
}

/// Starts announcing and browsing (once per process). Failures (no multicast) only disable
/// discovery: pairing by address keeps working.
pub fn start(engine: &Engine, port: u16) {
    if MESH.get().is_some() {
        return;
    }
    match create(engine, port) {
        Ok(m) => {
            let _ = MESH.set(m);
        }
        Err(e) => tracing::warn!("scoperta mDNS non disponibile: {e}"),
    }
}

fn create(engine: &Engine, port: u16) -> Result<Mesh, String> {
    let d = engine.device();
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let host = format!("{}.local.", host_label(&d.name));
    let props = [("id", d.id.as_str()), ("os", d.os.as_str()), ("tier", d.tier.as_str()), ("ver", aiw_core::VERSION), ("a2a", "/a2a")];
    let info = ServiceInfo::new(SERVICE, &d.name, &host, (), port, &props[..]).map_err(|e| e.to_string())?.enable_addr_auto();
    daemon.register(info).map_err(|e| e.to_string())?;
    let events = daemon.browse(SERVICE).map_err(|e| e.to_string())?;
    let seen: Arc<Mutex<BTreeMap<String, Discovered>>> = Arc::default();
    let sink = seen.clone();
    let me = d.id.clone();
    std::thread::Builder::new()
        .name("aiw-mdns".into())
        .spawn(move || {
            while let Ok(ev) = events.recv() {
                match ev {
                    ServiceEvent::ServiceResolved(info) => {
                        if let Some(dev) = from_resolved(&info).filter(|d| d.id != me) {
                            sink.lock().unwrap().insert(info.get_fullname().to_string(), dev);
                        }
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => {
                        sink.lock().unwrap().remove(&fullname);
                    }
                    _ => {}
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Mesh { _daemon: daemon, seen })
}

/// The address other devices should use: a LAN one, loopback only when nothing else is announced
/// (two instances on the same machine). Sorted, so the choice is stable across announcements.
fn best_address(mut ips: Vec<std::net::Ipv4Addr>) -> Option<std::net::Ipv4Addr> {
    ips.sort();
    ips.iter().find(|ip| !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified()).or(ips.first()).copied()
}

fn from_resolved(info: &ResolvedService) -> Option<Discovered> {
    let ip = best_address(info.get_addresses_v4().into_iter().collect())?;
    let fullname = info.get_fullname();
    let name = fullname.strip_suffix(&format!(".{SERVICE}")).unwrap_or(fullname).replace("\\032", " ").replace('\\', "");
    Some(Discovered {
        id: info.get_property_val_str("id")?.to_string(),
        name,
        os: info.get_property_val_str("os").unwrap_or_default().to_string(),
        tier: info.get_property_val_str("tier").unwrap_or_default().to_string(),
        version: info.get_property_val_str("ver").unwrap_or_default().to_string(),
        address: format!("http://{ip}:{}", info.get_port()),
        last_seen: chrono::Utc::now().to_rfc3339(),
    })
}

/// Devices currently announced on the LAN (empty when discovery is not running).
pub fn discovered() -> Vec<Discovered> {
    MESH.get().map(|m| m.seen.lock().unwrap().values().cloned().collect()).unwrap_or_default()
}

pub fn discovery_running() -> bool {
    MESH.get().is_some()
}

/// This device's address on the LAN (the interface used to reach other networks).
pub fn lan_ip() -> Option<IpAddr> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?; // TEST-NET-1: nothing is sent
    s.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
}

/// Link that pairs a phone in one step (its camera opens the web UI with the code) and its QR.
pub fn pairing_qr(base: &str, code: &str) -> Result<(String, String), String> {
    let url = format!("{base}/?pair={code}");
    let qr = qrcode::QrCode::new(url.as_bytes()).map_err(|e| e.to_string())?;
    let svg = qr.render::<qrcode::render::svg::Color>().min_dimensions(220, 220).quiet_zone(true).build();
    Ok((url, svg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_encodes_the_pairing_link() {
        let (url, svg) = pairing_qr("http://192.168.1.10:7777", "123456").unwrap();
        assert_eq!(url, "http://192.168.1.10:7777/?pair=123456");
        assert!(svg.starts_with("<?xml") && svg.contains("<svg") && svg.contains("<path"), "{}", &svg[..80]);
        assert_eq!(host_label("Studio PC (2)"), "studio-pc--2");
    }

    #[test]
    fn prefers_lan_addresses() {
        let ip = |s: &str| s.parse::<std::net::Ipv4Addr>().unwrap();
        assert_eq!(best_address(vec![ip("127.0.0.1"), ip("192.168.1.20"), ip("169.254.3.4")]), Some(ip("192.168.1.20")));
        assert_eq!(best_address(vec![ip("127.0.0.1")]), Some(ip("127.0.0.1")));
        assert_eq!(best_address(vec![]), None);
    }
}
