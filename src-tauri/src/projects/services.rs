use super::*;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Binding {
    key: String,
    id: String,
    method: String,
    #[serde(default = "owned")]
    owned: bool,
}
fn owned()->bool{true}
struct Desired {
    key: String,
    method: &'static str,
    params: Value,
    list: &'static str,
}

pub(super) async fn reconcile(
    app: &AppHandle,
    p: &Project,
    record: &mut Record,
    registry: &mut Registry,
    k: &str,
    down: bool,
) -> Result<(), String> {
    let runtime = app.state::<RuntimeManager>();
    let mut wanted = Vec::new();
    if !down {
        for (name, spec) in &p.environments {
            let id = &record.ids[name];
            for pc in spec.pc_access.iter().filter(|p| p.target.is_none()) {
                wanted.push(Desired {
                    key: format!("{id}/share/{}/{}", pc.path, pc.read_only),
                    method: "attach_host_folder",
                    params: json!({"environmentId":id,"path":pc.path,"readOnly":pc.read_only}),
                    list: "shares",
                });
            }
            for port in &spec.ports {
                let (guest, host) = port.values()?;
                wanted.push(Desired {
                    key: format!("{id}/manual/{guest}"),
                    method: "set_manual_service_port",
                    params: json!({"environmentId":id,"port":guest,"present":true}),
                    list: "manual",
                });
                if let Some(host) = host {
                    wanted.push(Desired{key:format!("{id}/loopback/{guest}/{host}"),method:"publish_environment_service",params:json!({"environmentId":id,"port":guest,"hostPort":host,"kind":"loopback"}),list:"publications"});
                }
            }
            if let Some(publish) = p.publish.get(name) {
                let (port, hostname, token_env, domain) = match publish {
                    Publication::Hostname(h) => (
                        spec.ports
                            .first()
                            .map(|p| p.values().map(|v| v.0))
                            .transpose()?
                            .unwrap_or(80),
                        Some(h.clone()),
                        None,
                        None,
                    ),
                    Publication::Detailed {
                        port,
                        hostname,
                        token_env,
                        domain,
                    } => (*port, hostname.clone(), token_env.clone(), domain.clone()),
                };
                let token=token_env.as_ref().map(|key|std::env::var(key).map_err(|_|format!("Set {key} in the engine environment or save the Cloudflare token in Desktop"))).transpose()?;
                wanted.push(Desired{key:format!("{id}/cloudflare/{port}/{}",domain.as_deref().or(hostname.as_deref()).unwrap_or("")),method:"publish_environment_service",params:json!({"environmentId":id,"port":port,"kind":"cloudflare","domain":domain,"cloudflare":hostname.map(|h|json!({"hostname":h,"token":token,"remember":false,"routesReviewed":true}))}),list:"publications"});
            }
        }
    }
    for binding in record.bindings.clone() {
        project_cancelled()?;
        if wanted.iter().any(|d| d.key == binding.key) {
            continue;
        }
        if !binding.owned { record.bindings.retain(|b| b.key!=binding.key); save_record(&runtime,k,record)?; continue; }
        if binding.method=="unpublish_environment_service" && wanted.iter().any(|desired|desired.list=="publications" && desired.key.split('/').take(3).eq(binding.key.split('/').take(3))) {
            // Let the publisher replace this same environment/guest-port route atomically.
            // Revoking it first would discard the working rollback state.
            continue;
        }
        let params = if binding.method == "manual" {
            let (id, port) = binding
                .id
                .split_once('/')
                .ok_or("Invalid project port record")?;
            json!({"environmentId":id,"port":port.parse::<u16>().map_err(|e|e.to_string())?,"present":false})
        } else if binding.method == "detach_host_folder" {
            json!({"shareId":binding.id})
        } else {
            json!({"publicationId":binding.id})
        };
        // Deleted nodes have already had their access revoked.
        if binding.method != "manual"
            || node(app, binding.id.split('/').next().unwrap_or("")).is_ok()
        {
            call(
                app,
                if binding.method == "manual" {
                    "set_manual_service_port"
                } else {
                    &binding.method
                },
                params,
            )
            .await?;
        }
        record.bindings.retain(|b| b.key != binding.key);
        registry.projects.insert(k.into(), record.clone());
        save_record(&runtime,k,record)?;
    }
    for d in wanted {
        project_cancelled()?;
        let id = d.params["environmentId"].as_str().unwrap();
        let current = if d.list == "manual" {
            json!({})
        } else {
            call(
                app,
                "list_environment_services",
                json!({"environmentId":id}),
            )
            .await?
        };
        let existing = current[d.list].as_array().and_then(|items| {
            items.iter().find(|v| {
                if d.list == "shares" {
                    v["path"] == d.params["path"] && v["readOnly"] == d.params["readOnly"]
                } else {
                    v["status"] == "active"
                        && v["port"] == d.params["port"]
                        && v["kind"] == d.params["kind"]
                        && (d.params["kind"] != "loopback" || v["hostPort"] == d.params["hostPort"])
                        && (d.params["domain"].is_null() || crate::workspace::cloudflare::find_domain(&app.state::<PlatformStore>(),d.params["domain"].as_str().unwrap()).ok().is_some_and(|domain| v["urls"].as_array().is_some_and(|urls|urls.iter().any(|url|url==&json!(format!("https://{}",domain.hostname))))))
                        && (d.params["cloudflare"].is_null() || v["urls"].as_array().is_some_and(|urls|urls.iter().any(|url|url==&json!(format!("https://{}",d.params["cloudflare"]["hostname"].as_str().unwrap_or(""))))))
                }
            })
        });
        if let Some(existing) = existing {
            let existing_id=existing["id"].as_str().unwrap_or("");
            record.bindings.retain(|b|b.key==d.key||b.id!=existing_id||b.method!="unpublish_environment_service");
            if let Some(binding) = record.bindings.iter_mut().find(|b| b.key == d.key) {
                binding.id = existing["id"].as_str().unwrap_or("").into();
            } else {
                // This publication/share may have completed before its result was delivered.
                // Reconcile its identity into the durable project registry instead of duplicating it.
                record.bindings.push(Binding { key:d.key.clone(),id:existing["id"].as_str().unwrap_or("").into(),method:if d.list=="shares"{"detach_host_folder"}else{"unpublish_environment_service"}.into(),owned:record.pending.contains_key(&format!("service:{}",d.key)) });
            }
            record.pending.remove(&format!("service:{}",d.key));
            save_record(&runtime,k,record)?;
            continue;
        }
        if d.list == "manual" {
            let present = app
                .state::<PlatformStore>()
                .snapshot()?
                .manual_service_ports
                .get(id)
                .is_some_and(|ports| ports.contains(&(d.params["port"].as_u64().unwrap() as u16)));
            if present {
                continue;
            }
        }
        record.pending.insert(format!("service:{}",d.key),"accepted".into());
        save_record(&runtime,k,record)?;
        let result = call(app, d.method, d.params.clone()).await?;
        record.pending.remove(&format!("service:{}",d.key));
        record.bindings.retain(|b| b.key != d.key && (b.method!="unpublish_environment_service"||b.id!=result["id"].as_str().unwrap_or("")));
        record.bindings.push(Binding {
            owned: true,
            key: d.key,
            id: if d.list == "manual" {
                format!("{id}/{}", d.params["port"])
            } else {
                result["id"]
                    .as_str()
                    .ok_or("Created access rule has no ID")?
                    .into()
            },
            method: if d.list == "manual" {
                "manual"
            } else if d.list == "shares" {
                "detach_host_folder"
            } else {
                "unpublish_environment_service"
            }
            .into(),
        });
        registry.projects.insert(k.into(), record.clone());
        save_record(&runtime,k,record)?;
    }
    Ok(())
}
