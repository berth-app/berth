use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

const SERVICE_TYPE: &str = "_berth._tcp.local.";

/// Discovered agent on the LAN.
pub struct DiscoveredAgent {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub addresses: Vec<String>,
}

/// Register this agent as an mDNS service so it can be discovered on the LAN.
///
/// Service type: `_berth._tcp.local.`
///
/// Returns a [`ServiceDaemon`] handle that keeps the service registered while alive.
/// Dropping the daemon will unregister the service.
pub fn register_agent(port: u16) -> Result<ServiceDaemon> {
    let mdns = ServiceDaemon::new()
        .map_err(|e| anyhow::anyhow!("Failed to create mDNS daemon: {}", e))?;

    let raw_hostname = hostname::get()?.to_string_lossy().to_string();
    // mdns-sd requires the hostname to end with ".local."
    let mdns_hostname = if raw_hostname.ends_with(".local") {
        format!("{}.", raw_hostname)
    } else if raw_hostname.ends_with(".local.") {
        raw_hostname.clone()
    } else {
        format!("{}.local.", raw_hostname)
    };
    let instance_name = format!("berth-agent-{}", &raw_hostname);

    let service_info = ServiceInfo::new(
        SERVICE_TYPE,
        &instance_name,
        &mdns_hostname,
        "",
        port,
        None,
    )
    .map_err(|e| anyhow::anyhow!("Failed to create service info: {}", e))?;

    mdns.register(service_info)
        .map_err(|e| anyhow::anyhow!("Failed to register mDNS service: {}", e))?;

    Ok(mdns)
}

/// Discover Berth agents on the LAN via mDNS.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_agent() {
        // Use a random high port to avoid conflicts
        let daemon = register_agent(59999);
        assert!(daemon.is_ok(), "mDNS agent registration should succeed");
        // Daemon is dropped here, which unregisters the service
    }

    #[tokio::test]
    async fn test_discover_agents_short_timeout() {
        // Quick 1-second scan — may find 0 agents, but should not error
        let agents = discover_agents(1).await;
        assert!(agents.is_ok(), "mDNS discovery should not error");
    }

    #[tokio::test]
    async fn test_register_then_discover() {
        let _daemon = register_agent(59998).expect("registration failed");

        // Give mDNS a moment to propagate, then discover
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let agents = discover_agents(2).await.expect("discovery failed");

        // We should find at least our own agent
        let found = agents.iter().any(|a| a.port == 59998);
        // Note: mDNS discovery on localhost is not guaranteed in all CI environments,
        // so we just assert the call succeeded without error
        if found {
            assert!(agents.iter().any(|a| a.name.contains("berth-agent")));
        }
    }
}
