//! Host-side filtering for the MicroVM's shared control/internet NIC.
//! The guest cannot undo this policy by changing its routes or firewall.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::RwLock,
    task::JoinHandle,
};

pub(super) struct MicroVmNetwork {
    enabled: Arc<RwLock<bool>>,
    failed: Arc<AtomicBool>,
    tasks: Vec<JoinHandle<Result<(), String>>>,
    arguments: Vec<String>,
}
impl Drop for MicroVmNetwork {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
impl MicroVmNetwork {
    pub async fn new(enabled: bool, qmp_port: u16) -> Result<Self, String> {
        let mut result = Self {
            enabled: Arc::new(RwLock::new(enabled)),
            failed: Arc::new(AtomicBool::new(false)),
            tasks: Vec::new(),
            arguments: Vec::new(),
        };
        for direction in ["rx", "tx"] {
            let outgoing = TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| e.to_string())?;
            let incoming = TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| e.to_string())?;
            let out_port = outgoing.local_addr().map_err(|e| e.to_string())?.port();
            let in_port = incoming.local_addr().map_err(|e| e.to_string())?.port();
            result.arguments.extend([
                "-chardev".into(), format!("socket,id=internet-{direction}-out,host=127.0.0.1,port={out_port}"),
                "-chardev".into(), format!("socket,id=internet-{direction}-in,host=127.0.0.1,port={in_port}"),
                "-object".into(), format!("filter-redirector,id=internet-{direction},netdev=net0,queue={direction},outdev=internet-{direction}-out,indev=internet-{direction}-in"),
            ]);
            let policy = result.enabled.clone();
            let failed = result.failed.clone();
            // QEMU rx is guest -> backend; tx is backend -> guest.
            result.tasks.push(tokio::spawn(async move {
                let ((mut reader, _), (mut writer, _)) =
                    tokio::time::timeout(Duration::from_secs(30), async {
                        tokio::try_join!(outgoing.accept(), incoming.accept())
                    })
                    .await
                    .map_err(|_| "MicroVM network filter connection timed out".to_string())?
                    .map_err(|e| e.to_string())?;
                let forwarding: Result<(), String> = async {
                    reader.set_nodelay(true).map_err(|e| e.to_string())?;
                    writer.set_nodelay(true).map_err(|e| e.to_string())?;
                    let mut packet = Vec::new();
                    loop {
                        let length = reader.read_u32().await.map_err(|e| e.to_string())? as usize;
                        if !(14..=65536).contains(&length) {
                            return Err("Invalid MicroVM network frame".into());
                        }
                        packet.resize(length, 0);
                        reader
                            .read_exact(&mut packet)
                            .await
                            .map_err(|e| e.to_string())?;
                        let enabled = policy.read().await;
                        if *enabled || control_packet(&packet, direction == "rx") {
                            tokio::time::timeout(Duration::from_secs(5), async {
                                writer.write_u32(length as u32).await?;
                                writer.write_all(&packet).await
                            })
                            .await
                            .map_err(|_| "MicroVM network filter stalled".to_string())?
                            .map_err(|e| e.to_string())?;
                        }
                    }
                }
                .await;
                if forwarding.is_err() {
                    failed.store(true, Ordering::Release);
                    // QEMU's redirector bypasses an absent output socket. Keep
                    // both sockets alive on failure and additionally lower the
                    // link before waiting for normal VM teardown.
                    let _ = super::vm::qmp_execute_bounded(
                        qmp_port,
                        "set_link",
                        Some(serde_json::json!({"name":"net0", "up":false})),
                        Duration::from_secs(5),
                    )
                    .await;
                    std::future::pending::<()>().await;
                }
                forwarding
            }));
        }
        Ok(result)
    }
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }
    pub async fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        if self.failed.load(Ordering::Acquire) || self.tasks.iter().any(|task| task.is_finished()) {
            return Err("MicroVM network filter stopped. Restart this environment.".into());
        }
        // Wait for in-flight writes before acknowledging a disconnected cable.
        *self.enabled.write().await = enabled;
        Ok(())
    }
}

fn control_packet(packet: &[u8], outbound: bool) -> bool {
    if packet.len() < 14 {
        return false;
    }
    match u16::from_be_bytes([packet[12], packet[13]]) {
        // ARP has no routable payload; needed for the control gateway.
        0x0806 => packet.len() >= 42 && packet[14..20] == [0, 1, 8, 0, 6, 4],
        0x0800 => {
            if packet.len() < 34 || packet[14] >> 4 != 4 {
                return false;
            }
            let header = usize::from(packet[14] & 15) * 4;
            let total = usize::from(u16::from_be_bytes([packet[16], packet[17]]));
            if header != 20 || total < header || packet.len() < 14 + total {
                return false;
            }
            let peer = if outbound {
                &packet[30..34]
            } else {
                &packet[26..30]
            };
            // Existing authenticated guest-agent sessions, selected PC shares,
            // and explicitly published local services use QEMU's host address.
            if peer == [10, 0, 2, 2] {
                return true;
            }
            // Permit DHCP bootstrap/renewal only. DNS and IPv6 cannot bypass
            // disconnection through the shared user-network backend.
            let fragments = u16::from_be_bytes([packet[20], packet[21]]);
            if packet[23] != 17 || fragments & 0x3fff != 0 || total < header + 8 {
                return false;
            }
            let udp = 14 + header;
            let source = u16::from_be_bytes([packet[udp], packet[udp + 1]]);
            let target = u16::from_be_bytes([packet[udp + 2], packet[udp + 3]]);
            outbound && packet[30..34] == [255, 255, 255, 255] && source == 68 && target == 67
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ipv4(source: [u8; 4], target: [u8; 4]) -> Vec<u8> {
        let mut packet = vec![0; 42];
        packet[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        packet[14] = 0x45;
        packet[16..18].copy_from_slice(&28u16.to_be_bytes());
        packet[23] = 17;
        packet[26..30].copy_from_slice(&source);
        packet[30..34].copy_from_slice(&target);
        packet
    }
    #[test]
    fn offline_preserves_control_but_blocks_external_traffic_both_ways() {
        let guest = [10, 0, 2, 15];
        let host = [10, 0, 2, 2];
        let external = [1, 1, 1, 1];
        assert!(control_packet(&ipv4(guest, host), true));
        assert!(control_packet(&ipv4(host, guest), false));
        assert!(!control_packet(&ipv4(guest, external), true));
        assert!(!control_packet(&ipv4(external, guest), false));
        assert!(!control_packet(&ipv4(guest, [10, 0, 2, 3]), true));
        assert!(!control_packet(&[0; 13], true));
        let mut ipv6 = vec![0; 80];
        ipv6[12..14].copy_from_slice(&0x86ddu16.to_be_bytes());
        assert!(!control_packet(&ipv6, true));
        let mut dhcp = ipv4([0; 4], [255; 4]);
        dhcp[34..38].copy_from_slice(&[0, 68, 0, 67]);
        assert!(control_packet(&dhcp, true));
        dhcp[20] = 0x20;
        assert!(!control_packet(&dhcp, true));
        let mut options = ipv4(guest, host);
        options[14] = 0x46;
        assert!(!control_packet(&options, true));
        let mut truncated = ipv4(guest, host);
        truncated[17] = 255;
        assert!(!control_packet(&truncated, true));
        let mut arp = vec![0; 42];
        arp[12..20].copy_from_slice(&[8, 6, 0, 1, 8, 0, 6, 4]);
        assert!(control_packet(&arp, true));
        assert!(control_packet(&arp, false));
    }
    #[tokio::test]
    async fn filter_failure_retains_redirect_sockets_and_rejects_policy_updates() {
        let network = MicroVmNetwork::new(false, 0).await.unwrap();
        let mut sockets = Vec::new();
        for argument in network
            .arguments()
            .iter()
            .filter(|value| value.starts_with("socket,"))
        {
            let port: u16 = argument.rsplit("port=").next().unwrap().parse().unwrap();
            sockets.push(
                tokio::net::TcpStream::connect(("127.0.0.1", port))
                    .await
                    .unwrap(),
            );
        }
        sockets[0].write_u32(1).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !network.failed.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(network.set_enabled(true).await.is_err());
        // Closing the reader would make QEMU bypass its redirector.
        let mut byte = [0];
        assert!(
            tokio::time::timeout(Duration::from_millis(100), sockets[0].read(&mut byte))
                .await
                .is_err()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "boots a disposable bundled MicroVM to verify host-enforced internet and control traffic"]
    async fn microvm_internet_toggle_preserves_control_and_host_access() -> Result<(), String> {
        use crate::models::{Priority, ResourcePolicy, ResourceRange};
        use crate::runtime::RuntimeManager;
        use std::path::Path;
        let data = tempfile::tempdir().unwrap();
        let runtime = RuntimeManager::new(Path::new(env!("CARGO_MANIFEST_DIR")), data.path())?;
        let probe = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
        probe.connect("1.1.1.1:80").map_err(|e| e.to_string())?;
        let host_ip = probe.local_addr().map_err(|e| e.to_string())?.ip();
        let listener = TcpListener::bind("0.0.0.0:0")
            .await
            .map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let http = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut request = [0; 4096];
                    let _ = stream.read(&mut request).await;
                    let _ = stream
                        .write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\nOK")
                        .await;
                });
            }
        });
        let result = async {
            let vm = runtime
                .provision_micro_vm("internet-test", "builtin:alpine")
                .await?;
            let policy = ResourcePolicy {
                cpu: ResourceRange {
                    min: 1.,
                    preferred: 1.,
                    max: 1.,
                    current: 0.,
                },
                memory_gb: ResourceRange {
                    min: 0.5,
                    preferred: 0.5,
                    max: 0.5,
                    current: 0.,
                },
                priority: Priority::Normal,
                dynamic: true,
            };
            runtime
                .start_micro_vm_with_network(
                    "internet-test",
                    &vm.disk_path,
                    &vm.source_path,
                    &policy,
                    false,
                )
                .await?;
            for enabled in [false, true, false, true] {
                runtime.update_vm_internet("internet-test", enabled).await?;
                let management = runtime
                    .execute_micro_vm_command(
                        "internet-test",
                        &format!("wget -T 4 -qO- http://10.0.2.2:{port}"),
                    )
                    .await?;
                if management.exit_code != 0 || management.stdout != "OK" {
                    return Err(format!(
                        "control/PC access failed with internet={enabled}: {management:?}"
                    ));
                }
                let external = runtime
                    .execute_micro_vm_command(
                        "internet-test",
                        &format!("wget -T 3 -qO- http://{host_ip}:{port}"),
                    )
                    .await?;
                if (external.exit_code == 0) != enabled {
                    return Err(format!("internet={enabled} not enforced: {external:?}"));
                }
            }
            runtime.vm_action("internet-test", "pause").await?;
            runtime.update_vm_internet("internet-test", false).await?;
            runtime.vm_action("internet-test", "resume").await?;
            let blocked = runtime
                .execute_micro_vm_command(
                    "internet-test",
                    &format!("wget -T 3 -qO- http://{host_ip}:{port}"),
                )
                .await?;
            if blocked.exit_code == 0 {
                return Err("paused policy change was not enforced".into());
            }
            runtime.vm_action("internet-test", "stop").await?;
            runtime
                .start_micro_vm_with_network(
                    "internet-test",
                    &vm.disk_path,
                    &vm.source_path,
                    &policy,
                    false,
                )
                .await?;
            let blocked = runtime
                .execute_micro_vm_command(
                    "internet-test",
                    &format!("wget -T 3 -qO- http://{host_ip}:{port}"),
                )
                .await?;
            if blocked.exit_code == 0 {
                return Err("offline boot leaked internet traffic".into());
            }
            Ok(())
        }
        .await;
        runtime.shutdown_all().await;
        http.abort();
        result
    }
}
