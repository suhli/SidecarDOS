use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceInfo};
pub struct Advertisement {
    daemon: ServiceDaemon,
    name: String,
}
impl Advertisement {
    pub fn start(id: &[u8; 16], name: &str, port: u16) -> Result<Self> {
        let daemon = ServiceDaemon::new()?;
        let host = format!("sidecardos-{}.local.", hex::encode(id));
        let properties = [("version", "1"), ("id", &hex::encode(id))];
        let info = ServiceInfo::new(
            "_sidecardos._udp.local.",
            name,
            &host,
            "",
            port,
            &properties[..],
        )?
        .enable_addr_auto();
        let full = info.get_fullname().to_owned();
        daemon.register(info)?;
        Ok(Self { daemon, name: full })
    }
}
impl Drop for Advertisement {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.name);
        let _ = self.daemon.shutdown();
    }
}
