//! mDNS advertisement of a `{hostname}.local` name for the gateway, so it
//! can be reached from the local network without editing `/etc/hosts` or
//! configuring DNS. Advertising only; resolving `.local` names back into
//! this advertisement is the OS's mDNS client, which we don't control (see
//! module docs on Windows caveats in the project plan).

use mdns_sd::{ServiceDaemon, ServiceInfo};

const SERVICE_TYPE: &str = "_http._tcp.local.";
const MAX_HOSTNAME_BYTES: usize = 15;

/// An active mDNS advertisement. Call [`stop`](Self::stop) on shutdown to
/// send a goodbye packet before the process exits; otherwise the record
/// lingers in peers' caches until its TTL expires.
pub struct MdnsHandle {
    daemon: ServiceDaemon,
    fullname: String,
}

impl MdnsHandle {
    pub fn stop(self) {
        if let Ok(rx) = self.daemon.unregister(&self.fullname) {
            let _ = rx.recv_timeout(std::time::Duration::from_millis(500));
        }
        if let Ok(rx) = self.daemon.shutdown() {
            let _ = rx.recv_timeout(std::time::Duration::from_millis(500));
        }
    }
}

/// Starts advertising `{hostname}.local` on `port` via mDNS. Returns
/// `Ok(None)` when `hostname` is empty (mDNS advertisement disabled by
/// config) rather than starting anything.
pub fn start_advertising(hostname: &str, port: u16) -> anyhow::Result<Option<MdnsHandle>> {
    if hostname.is_empty() {
        return Ok(None);
    }
    if hostname.len() > MAX_HOSTNAME_BYTES {
        anyhow::bail!("mdns_hostname {hostname:?} exceeds {MAX_HOSTNAME_BYTES} bytes (RFC 6763)");
    }

    let daemon = ServiceDaemon::new()?;
    let host_name = format!("{hostname}.local.");
    let service_info = ServiceInfo::new(SERVICE_TYPE, hostname, &host_name, "", port, None)?.enable_addr_auto();
    let fullname = service_info.get_fullname().to_string();

    daemon.register(service_info)?;
    tracing::info!(hostname = %host_name, port, "advertising gateway via mDNS");

    Ok(Some(MdnsHandle { daemon, fullname }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_hostname_disables_mdns() {
        assert!(start_advertising("", 8787).unwrap().is_none());
    }

    #[test]
    fn oversized_hostname_is_rejected() {
        let result = start_advertising("this-name-is-too-long", 8787);
        assert!(result.is_err());
    }
}
