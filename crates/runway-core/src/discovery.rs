use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

const SERVICE_TYPE: &str = "_runway._tcp.local.";

/// Discovered agent on the LAN.
pub struct DiscoveredAgent {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub addresses: Vec<String>,
}

/// Register this agent as an mDNS service so it can be discovered on the LAN.
///
/// Service type: `_runway._tcp.local.`
///
/// Returns a [`ServiceDaemon`] handle that keeps the service registered while alive.
/// Dropping the daemon will unregister the service.
pub fn register_agent(port: u16) -> Result<ServiceDaemon> {
    let mdns = ServiceDaemon::new()
        .map_err(|e| anyhow::anyhow!("Failed to create mDNS daemon: {}", e))?;

    let hostname = hostname::get()?.to_string_lossy().to_string();
    let instance_name = format!("runway-agent-{}", &hostname);

    let service_info = ServiceInfo::new(
        SERVICE_TYPE,
        &instance_name,
        &hostname,
        "",
        port,
        None,
    )
    .map_err(|e| anyhow::anyhow!("Failed to create service info: {}", e))?;

    mdns.register(service_info)
        .map_err(|e| anyhow::anyhow!("Failed to register mDNS service: {}", e))?;

    Ok(mdns)
}

/// Discover Runway agents on the LAN via mDNS.
///
/// Listens for `timeout_secs` seconds and returns all agents found during that window.
pub async fn discover_agents(timeout_secs: u64) -> Result<Vec<DiscoveredAgent>> {
    let mdns = ServiceDaemon::new()
        .map_err(|e| anyhow::anyhow!("Failed to create mDNS daemon: {}", e))?;

    let receiver = mdns
        .browse(SERVICE_TYPE)
        .map_err(|e| anyhow::anyhow!("Failed to browse for services: {}", e))?;

    let agents = tokio::task::spawn_blocking(move || {
        let mut found = Vec::new();
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

        while std::time::Instant::now() < deadline {
            let remaining = deadline - std::time::Instant::now();
            match receiver
                .recv_timeout(remaining.min(std::time::Duration::from_secs(1)))
            {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    found.push(DiscoveredAgent {
                        name: info.get_fullname().to_string(),
                        host: info.get_hostname().to_string(),
                        port: info.get_port(),
                        addresses: info
                            .get_addresses()
                            .iter()
                            .map(|a| a.to_string())
                            .collect(),
                    });
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        found
    })
    .await?;

    let _ = mdns.stop_browse(SERVICE_TYPE);
    Ok(agents)
}
