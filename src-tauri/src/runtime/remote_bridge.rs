use super::{fabric::Fabric, RuntimeManager};
use crate::{models::{ConnectionDirection, Environment, PermissionKind}, remote_access::{bridge::{self, Open}, client}};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

pub(super) struct BridgeHandle {
    task: JoinHandle<()>,
    fabric: Fabric,
    rule_id: String,
    remote_id: String,
    local_id: String,
}
impl Drop for BridgeHandle {
    fn drop(&mut self) {
        self.task.abort();
        self.fabric.remove(&self.rule_id);
        self.fabric.detach(&self.remote_id);
    }
}

impl RuntimeManager {
    pub(crate) async fn remote_connection_live(&self, id: &str) -> bool {
        self.remote_bridges.lock().await.get(id).is_some_and(|bridge| {
            !bridge.task.is_finished() && bridge.fabric.connected(&bridge.remote_id)
                && bridge.fabric.connected(&bridge.local_id)
        })
    }

    pub(crate) async fn apply_remote_connection(
        &self,
        id: &str,
        source: &Environment,
        target: &Environment,
        direction: &ConnectionDirection,
        permissions: &[PermissionKind],
        ports: &[u16],
    ) -> Result<String, String> {
        let source_remote = source.runtime.starts_with("shared://tunnel/");
        let remote = if source_remote { source } else { target };
        let local = if source_remote { target } else { source };
        if !remote.runtime.starts_with("shared://tunnel/") || local.runtime.starts_with("shared://") {
            return Err("A private tunnel connects one shared environment to one environment on this computer".into());
        }
        let mut bridges = self.remote_bridges.lock().await;
        if bridges.get(id).is_some_and(|bridge| !bridge.task.is_finished() && bridge.fabric.connected(&bridge.remote_id) && bridge.fabric.connected(&bridge.local_id)) {
            return Ok(format!("private:tunnel:{id}"));
        }
        bridges.remove(id);
        drop(bridges);

        let capabilities = crate::remote_access::request_saved(remote, "inspect", json!({})).await?;
        if capabilities["permission"] != "control" { return Err("The owner must grant Full Control before a private connection can be made".into()); }
        let remote_id = capabilities["fabricId"].as_str().filter(|id| bridge::valid_id(id)).ok_or("The owner has no private network adapter for this share")?.to_owned();
        let local_id = self.prepare_shared_fabric_peer(local).await?;
        let (url, token) = client::bridge_credentials(remote)?;
        let open = Open { token, connection_id: id.to_owned(), peer_id: local_id.clone(), direction: direction.clone(), source_remote, permissions: permissions.to_vec(), ports: ports.to_vec() };
        bridge::validate_open(&open)?;
        let (mut websocket, _) = tokio::time::timeout(std::time::Duration::from_secs(15), tokio_tungstenite::connect_async_with_config(url, Some(bridge::websocket_config()), true))
            .await.map_err(|_| "The owner's sharing tunnel did not connect in time")?
            .map_err(|_| "Cannot reach the owner's sharing tunnel. Reconnect the shared node if its link changed")?;
        websocket.send(Message::Text(serde_json::to_string(&open).map_err(|_| "Cannot encode private connection request")?.into()))
            .await.map_err(|_| "Cannot sign in to the owner's private connection")?;
        let acknowledgement = tokio::time::timeout(std::time::Duration::from_secs(20), websocket.next())
            .await.map_err(|_| "The owner did not authorize the private connection in time")?
            .ok_or("The owner closed the private connection")?
            .map_err(|_| "The owner's private connection was interrupted")?;
        let Message::Text(text) = acknowledgement else { return Err("Invalid private connection response".into()) };
        let reply: Value = serde_json::from_str(&text).map_err(|_| "Invalid private connection response")?;
        if reply["ok"] != true {
            return Err(reply["error"].as_str().unwrap_or("The owner refused the private connection").chars().take(500).collect());
        }
        if reply["fabricId"].as_str() != Some(&remote_id) { return Err("The shared environment changed during connection; reconnect its node".into()); }
        let source_id = if source_remote { &remote_id } else { &local_id };
        let target_id = if source_remote { &local_id } else { &remote_id };
        let socket = self.open_bridge_peer(id, local, &remote_id, source_id, target_id, direction, permissions, ports).await?;
        let fabric = self.fabric_clone();
        let task_fabric = fabric.clone();
        let rule_id = id.to_owned();
        let task_rule = rule_id.clone();
        let task_remote = remote_id.clone();
        let task_local = local_id.clone();
        let task = tokio::spawn(async move {
            let _ = bridge::relay(&mut websocket, socket, CancellationToken::new(), task_fabric.clone(), &task_local).await;
            task_fabric.remove(&task_rule);
            task_fabric.detach(&task_remote);
        });
        self.remote_bridges.lock().await.insert(id.to_owned(), BridgeHandle { task, fabric, rule_id, remote_id, local_id });
        Ok(format!("private:tunnel:{id}"))
    }

    pub(crate) async fn remove_remote_connection(&self, id: &str) -> bool {
        self.remote_bridges.lock().await.remove(id).is_some()
    }
}
