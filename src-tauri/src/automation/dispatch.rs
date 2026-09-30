use crate::{
    backup::BackupManager,
    commands, guest_apps, guest_keyboard, local_backup,
    models::*,
    runtime::{self, RuntimeManager},
    store::PlatformStore,
    workspace::{self, WorkspaceManager},
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tauri::Manager;
use crate::AppHandle;

fn arg<T: DeserializeOwned>(params: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(params.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("Invalid {key}: {e}"))
}
fn encoded(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

fn merge_limits(policy: &mut ResourcePolicy, p: &Value) -> Result<(), String> {
    for (key, range) in [
        ("cpu", &mut policy.cpu),
        ("memoryGb", &mut policy.memory_gb),
    ] {
        if let Some(value) = p.get(key).filter(|v| !v.is_null()) {
            for (field, value) in value
                .as_object()
                .ok_or("A resource range must be an object")?
            {
                let number = value
                    .as_f64()
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .ok_or("Resource values must be positive numbers")?;
                match field.as_str() {
                    "min" => range.min = number,
                    "preferred" => range.preferred = number,
                    "max" => range.max = number,
                    _ => return Err(format!("Unknown resource field: {field}")),
                }
            }
        }
    }
    if p.get("priority").is_some_and(|v| !v.is_null()) {
        policy.priority = arg(p, "priority")?;
    }
    commands::validate_policy(policy)
}

pub(super) fn validate(method: &str, p: &Value) -> Result<(), String> {
    // Validate nested native types even on dry runs, without touching runtime state.
    match method {
        "runpod_create_pod" => {
            let _: crate::neocloud::runpod::PodRequest = arg(p, "request")?;
        }
        "runpod_create_endpoint" => {
            let _: crate::neocloud::runpod::EndpointRequest = arg(p, "request")?;
        }
        "create_neocloud_environment" => {
            let _: crate::neocloud::CreateRequest = arg(p, "request")?;
        }
        "add_cloud_environment" => {
            let _: crate::runtime::cloud::Profile = arg(p,"request")?;
        },
        "host_terminal_action" => {
            let request: crate::host_terminal::HostRequest = arg(p, "request")?;
            crate::host_terminal::validate_request(&request)?;
        }
        "create_environment" => {
            let _: CreateEnvironmentRequest = arg(p, "request")?;
        }
        "create_connection" => {
            let _: CreateConnectionRequest = arg(p, "request")?;
        }
        "update_resource_policy" => {
            let _: ResourcePolicy = arg(p, "resourcePolicy")?;
        }
        "add_backup_destination" => {
            let _: AddDestinationRequest = arg(p, "request")?;
        }
        "execute_environment_command" => {
            let _: ExecuteCommandRequest = arg(p, "request")?;
        }
        "execute_connected_command" => {
            let _: ExecuteConnectedCommandRequest = arg(p, "request")?;
        }
        "update_settings" => {
            let _: AppSettings = arg(p, "settings")?;
        }
        "publish_environment_service" => {
            let _: Option<workspace::cloudflare::AccountOptions> = arg(p, "cloudflare")?;
        }
        "set_guest_keyboard_capture" => {
            let _: Option<guest_keyboard::CaptureBounds> = arg(p, "bounds")?;
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn dispatch<'a>(app: &'a AppHandle, method: &'a str, p: &'a Value) -> std::pin::Pin<Box<dyn std::future::Future<Output=Result<Value,String>> + Send + 'a>> {
    dispatch_with_progress(app, method, p, std::sync::Arc::new(|_| {}))
}

pub(super) fn dispatch_with_progress<'a>(app: &'a AppHandle, method: &'a str, p: &'a Value, progress: std::sync::Arc<dyn Fn(Value) + Send + Sync>) -> std::pin::Pin<Box<dyn std::future::Future<Output=Result<Value,String>> + Send + 'a>> {
    Box::pin(async move {
    let store = app.state::<PlatformStore>();
    let runtime = app.state::<RuntimeManager>();
    let backup = app.state::<BackupManager>();
    let manager = app.state::<WorkspaceManager>();
    macro_rules! a {
        ($key:literal) => {
            arg(p, $key)?
        };
    }
    let window = || {
        app.get_webview_window(p["label"].as_str().unwrap_or(""))
            .filter(|w| w.label().starts_with("environment-env-"))
            .ok_or_else(|| "Guest window is closed or the label is invalid".to_string())
    };
    match method {
        "runpod_status" => crate::neocloud::runpod::runpod_status().await,
        "runpod_connect" => crate::neocloud::runpod::runpod_connect(a!("apiKey")).await,
        "runpod_catalog" => crate::neocloud::runpod::runpod_catalog().await,
        "runpod_disconnect" => crate::neocloud::runpod::runpod_disconnect().await,
        "runpod_template" => crate::neocloud::runpod::runpod_template(a!("id")).await,
        "runpod_search_templates" => crate::neocloud::runpod::runpod_search_templates(a!("term")).await,
        "runpod_hub" => crate::neocloud::runpod::runpod_hub(a!("search")).await,
        "runpod_hub_repo" => crate::neocloud::runpod::runpod_hub_repo(a!("id")).await,
        "runpod_links" => crate::neocloud::runpod::runpod_links(a!("environmentId"), store).await,
        "runpod_logs" => crate::neocloud::runpod::runpod_logs(a!("environmentId"), store).await,
        "runpod_create_endpoint" => encoded(crate::neocloud::runpod::runpod_create_endpoint(a!("request"), app.clone(), store).await?),
        "runpod_endpoint_run" => crate::neocloud::runpod::runpod_endpoint_run(a!("environmentId"), a!("input"), store).await,
        "runpod_action" => encoded(crate::neocloud::runpod::runpod_action(a!("environmentId"), a!("action"), a!("confirmation"), app.clone(), store, runtime).await?),
        "runpod_resources" => crate::neocloud::runpod::runpod_resources(store).await,
        "runpod_attach" => encoded(crate::neocloud::runpod::runpod_attach(a!("kind"), a!("resourceId"), app.clone(), store).await?),
        "runpod_volume" => crate::neocloud::runpod::runpod_volume(a!("action"), a!("id"), a!("name"), a!("location"), a!("sizeGb")).await,
        "runpod_registry" => crate::neocloud::runpod::runpod_registry(a!("action"), a!("id"), a!("name"), a!("username"), a!("password")).await,
        "runpod_gpu_offers" => crate::neocloud::runpod::runpod_gpu_offers(a!("containerDiskGb"), a!("gpuCount")).await,
        "runpod_create_pod" => encoded(crate::neocloud::runpod::runpod_create_pod(a!("request"), app.clone(), store).await?),
        "neocloud_providers" => encoded(crate::neocloud::neocloud_providers()),
        "neocloud_install" => crate::neocloud::neocloud_install(a!("provider")).await,
        "neocloud_account" => crate::neocloud::neocloud_account(a!("provider"), a!("location")).await,
        "neocloud_authenticate" => crate::neocloud::neocloud_authenticate(a!("provider"), a!("apiKey"), a!("location")).await,
        "neocloud_forget_account" => encoded(crate::neocloud::neocloud_forget_account(a!("provider"))?),
        "neocloud_catalog" => crate::neocloud::neocloud_catalog(a!("provider"), a!("product"), a!("location")).await,
        "neocloud_discover" => crate::neocloud::neocloud_discover(a!("provider"), a!("location")).await,
        "neocloud_prices" => crate::neocloud::neocloud_prices(a!("provider"), a!("product"), a!("offer"), a!("location"), a!("hours"), a!("maxHourly"), a!("minVramGb"), a!("limit")).await,
        "neocloud_plan" => crate::neocloud::neocloud_plan(a!("request")),
        "create_neocloud_environment" => encoded(crate::neocloud::create_neocloud_environment(a!("request"), a!("costAcknowledged"), store).await?),
        "neocloud_action" => encoded(crate::neocloud::neocloud_action(a!("environmentId"), a!("action"), a!("confirmation"), store, runtime).await?),
        "neocloud_recover_id" => encoded(crate::neocloud::neocloud_recover_id(a!("environmentId"), a!("resourceId"), store).await?),
        "run_model" => crate::model_runner::run_model_with_resources(a!("model"),a!("port"),a!("resources"),app.clone()).await,
        "run_neocloud_model" => crate::model_runner::run_neocloud_model(a!("model"),a!("environmentId"),a!("port"),app.clone()).await,
        "start_model" => crate::model_runner::start_model(a!("environmentId"),app.clone()).await,
        "stop_model" => crate::model_runner::stop_model(a!("environmentId"),app.clone()).await,
        "test_cloud_connection" => encoded(commands::cloud::test_cloud_connection(a!("request"),a!("environmentId"),store,runtime).await?),
        "duplicate_local_environment" => encoded(local_backup::duplicate_local_environment(a!("environmentId"),a!("name"),a!("storageDrive"),store,runtime,backup).await?),
        "duplicate_environment" => encoded(crate::duplication::duplicate_environment(a!("request"),store,runtime).await?),
        "inspect_duplication_source" => crate::duplication::inspect_duplication_source(a!("environmentId"),a!("source"),store,runtime).await,
        "cleanup_environment_duplication" => encoded(crate::duplication::cleanup_environment_duplication(a!("operationId"),store,runtime).await?),
        "model_api" => crate::model_runner::model_api(a!("environmentId"),a!("port"),app.clone()).await,
        "model_status" => crate::model_runner::model_status(a!("environmentId"),app.clone()).await,
        "model_chat" => crate::model_runner::model_chat(a!("environmentId"),a!("messages"),a!("maxTokens"),a!("temperature"),app.clone()).await,
        "model_chat_begin" => crate::model_runner::model_chat_begin(a!("environmentId"),a!("messages"),a!("maxTokens"),a!("temperature"),app.clone()).await,
        "model_chat_read" => crate::model_runner::model_chat_read(a!("requestId"),a!("offset"),a!("stop")),
        "environment_changes" => crate::changes::environment_changes(a!("environmentId"),a!("baseline"),a!("offset"),app.clone()).await,
        "inspect_project" => crate::projects::inspect_project(a!("path"),app.clone()).await,
        "discover_projects" => crate::projects::discover_projects(app.clone()).await,
        "import_compose" => crate::projects::import_compose(a!("path"),a!("write"),a!("project"),app.clone()).await,
        "project_action" => crate::projects::project_action(a!("path"),a!("action"),app.clone()).await,
        "run_workload" => crate::projects::run_workload(a!("request"),a!("start"),app.clone()).await,
        "cloud_authenticate" => crate::cloud_deployment::cloud_authenticate(a!("provider"),a!("account"),a!("sso")).await,
        "deploy_cloud_environment" => encoded(crate::cloud_deployment::deploy_cloud_environment(a!("request"),a!("riskAcknowledged"),store).await?),
        "cloud_deployment_action" => crate::cloud_deployment::cloud_deployment_action(a!("environmentId"),a!("action"),store,runtime).await,
        "finish_app_close" => encoded(crate::lifecycle::finish_app_close(a!("keepRunning"),app.clone(),store,runtime).await?),
        "open_isolated_cli" => encoded(crate::isolated_cli::open_isolated_cli(app.clone(),store,runtime).await?),
        "grant_isolated_cli_environment" => crate::isolated_cli::grant_isolated_cli_environment(a!("environmentId"),a!("permission"),app.clone()).await,
        "delete_cloud_deployment" => encoded(crate::cloud_deployment::delete_cloud_deployment(a!("environmentId"),a!("confirmation"),store,runtime).await?),
        "configure_cloud_environment" => encoded(commands::cloud::configure_cloud_environment(a!("environmentId"),a!("request"),store,runtime).await?),
        "get_environment_logs" => encoded(commands::workloads::get_environment_logs(a!("environmentId"),store,runtime).await?),
        "manage_oci_images" => commands::workloads::manage_oci_images(a!("action"),a!("image"),runtime).await,
        "create_remote_share" => encoded(crate::remote_access::create_remote_share(a!("request"),app.clone(),app.state::<crate::remote_access::RemoteAccess>()).await?),
        "start_remote_tunnel" => {
            // `domain` names a saved domain; its vaulted token is reused.
            let (cloudflare, host_port) = match p["domain"].as_str() {
                Some(name) if p.get("cloudflare").is_none_or(Value::is_null) => { let (options, port) = workspace::cloudflare::domain_account(&store, name)?; (Some(options), Some(port)) }
                Some(_) => return Err("Use either domain or cloudflare, not both".into()),
                None => (a!("cloudflare"), a!("hostPort")),
            };
            encoded(crate::remote_access::start_remote_tunnel(cloudflare,host_port,app.clone(),app.state::<crate::remote_access::RemoteAccess>()).await?)
        }
        "stop_remote_tunnel" => encoded(crate::remote_access::stop_remote_tunnel(app.clone(),app.state::<crate::remote_access::RemoteAccess>()).await?),
        "list_remote_shares" => encoded(crate::remote_access::list_remote_shares(app.state::<crate::remote_access::RemoteAccess>()).await?),
        "update_remote_share" => encoded(crate::remote_access::update_remote_share(a!("shareId"),a!("permission"),a!("password"),a!("revoke"),app.clone(),app.state::<crate::remote_access::RemoteAccess>()).await?),
        "remove_remote_share" => encoded(crate::remote_access::remove_remote_share(a!("shareId"),app.clone(),app.state::<crate::remote_access::RemoteAccess>()).await?),
        "connect_remote_share" => encoded(crate::remote_access::connect_remote_share(a!("link"),a!("username"),a!("password"),a!("environmentId"),store,runtime).await?),
        "reconnect_remote_share" => encoded(crate::remote_access::client::reconnect_remote_share(a!("environmentId"),store,runtime).await?),
        "download_remote_folder" => encoded(crate::remote_access::client::download_remote_folder(a!("environmentId"),a!("path"),a!("destination"),store).await?),
        "remote_share_request" => encoded(crate::remote_access::remote_share_request(a!("environmentId"),a!("method"),a!("params"),store).await?),
        "create_environment_share" => encoded(crate::peer_sharing::create_environment_share(a!("environmentId"),a!("address"),a!("permission"),app.clone(),store,app.state::<crate::peer_sharing::Sharing>()).await?),
        "list_environment_shares" => encoded(crate::peer_sharing::list_environment_shares(app.state::<crate::peer_sharing::Sharing>()).await?),
        "revoke_environment_share" => encoded(crate::peer_sharing::revoke_environment_share(a!("shareId"),app.clone(),app.state::<crate::peer_sharing::Sharing>()).await?),
        "import_environment_share" => encoded(crate::peer_sharing::import_environment_share(a!("invitation"),store).await?),
        "get_storage_location" => encoded(commands::storage::get_storage_location(runtime)),
        "set_storage_location" => encoded(commands::storage::set_storage_location(a!("path"), app.clone(), store, runtime).await?),
        "scan_cloud_host" => encoded(commands::cloud::scan_cloud_host(a!("host"),a!("port")).await?),
        "add_cloud_environment" => encoded(commands::cloud::add_cloud_environment(a!("request"),store,runtime)?),
        "get_cloud_connection" => encoded(commands::cloud::get_cloud_connection(a!("environmentId"),store,runtime).await?),
        "get_host_terminal_info" => crate::host_terminal::info(app, "cli-host"),
        "set_up_agent_access" => encoded(crate::host_terminal::setup_access(app).await?),
        "host_terminal_action" => encoded(
            crate::host_terminal::action_for_owner(app, a!("request"), "cli-host".into()).await?,
        ),
        "get_platform_state" => encoded(commands::get_platform_state(store,runtime).await?),
        "install_environment_skills" => encoded(
            commands::connection_skills::install::install_environment_skills(a!("environmentId"), store, runtime, manager).await?,
        ),
        "get_connection_skills" => encoded(
            commands::connection_skills::get_connection_skills(a!("environmentId"), store, runtime, manager)
                .await?,
        ),
        "create_environment" => {
            encoded(commands::create_environment(a!("request"), app.clone(), store, runtime).await?)
        }
        "set_environment_status" => encoded(
            commands::set_environment_status(a!("environmentId"), a!("status"), store, runtime)
                .await?,
        ),
        "restart_environment" => {
            let id: String = a!("environmentId");
            if store.snapshot()?.environments.iter().any(|e|e.id==id && e.kind==EnvironmentKind::Cloud) {return Err("Cloud nodes support Connect/Disconnect, not Restart".into());}
            commands::set_environment_status(
                id.clone(),
                EnvironmentStatus::Stopped,
                store.clone(),
                runtime.clone(),
            )
            .await?;
            encoded(
                commands::set_environment_status(id, EnvironmentStatus::Running, store, runtime)
                    .await?,
            )
        }
        "recover_environment_runtime" => {
            let id: String = a!("environmentId");
            let env = store
                .snapshot()?
                .environments
                .into_iter()
                .find(|e| e.id == id)
                .ok_or("Environment not found")?;
            if env
                .provider
                .as_ref()
                .is_some_and(RuntimeProviderKind::is_container)
                || env.kind == EnvironmentKind::Container
            {
                encoded(
                    commands::recover_container_runtime(id, a!("confirmed"), store, runtime)
                        .await?,
                )
            } else {
                encoded(commands::recover_vm_runtime(id, a!("confirmed"), store, runtime).await?)
            }
        }
        "delete_environment" => encoded(
            commands::delete_environment(
                a!("environmentId"),
                a!("recoverRuntime"),
                store,
                runtime,
                backup,
            )
            .await?,
        ),
        "factory_reset_environment" => encoded(
            commands::factory_reset::factory_reset_environment(
                a!("environmentId"),
                a!("confirmation"),
                store,
                runtime,
                backup,
                manager,
            )
            .await?,
        ),
        "recover_container_runtime" => encoded(
            commands::recover_container_runtime(
                a!("environmentId"),
                a!("confirmed"),
                store,
                runtime,
            )
            .await?,
        ),
        "recover_vm_runtime" => encoded(
            commands::recover_vm_runtime(a!("environmentId"), a!("confirmed"), store, runtime)
                .await?,
        ),
        "rename_environment" => encoded(commands::rename_environment(a!("environmentId"), a!("name"), store).await?),
        "update_container_startup_command" => encoded(commands::startup::update_container_startup_command(a!("environmentId"), a!("command"), store, runtime).await?),
        "update_resource_policy" => encoded(
            commands::update_resource_policy(
                a!("environmentId"),
                a!("resourcePolicy"),
                store,
                runtime,
            )
            .await?,
        ),
        "configure_resource_limits" => {
            let id: String = a!("environmentId");
            let mut policy = store
                .snapshot()?
                .environments
                .into_iter()
                .find(|e| e.id == id)
                .ok_or("Environment not found")?
                .resource_policy;
            merge_limits(&mut policy, p)?;
            encoded(commands::update_resource_policy(id, policy, store, runtime).await?)
        }
        "reclaim_storage" => encoded(commands::storage::reclaim_storage(store, runtime).await?),
        "get_storage_allocation" => encoded(
            commands::storage::get_storage_allocation(
                a!("environmentId"),
                a!("newVm"),
                a!("storageDrive"),
                store,
                runtime,
            )
            .await?,
        ),
        "expand_environment_storage" => encoded(
            commands::storage::expand_environment_storage(
                a!("environmentId"),
                a!("capacityGb"),
                store,
                runtime,
            )
            .await?,
        ),
        "update_container_network" => encoded(
            commands::update_container_network(a!("environmentId"), a!("enabled"), store, runtime)
                .await?,
        ),
        "update_environment_gpu" => encoded(
            commands::update_environment_gpu(a!("environmentId"), a!("enabled"), store, runtime)
                .await?,
        ),
        "create_connection" => {
            encoded(commands::create_connection(a!("request"), store, runtime).await?)
        }
        "set_connection_active" => encoded(
            commands::set_connection_active(a!("connectionId"), a!("active"), store, runtime)
                .await?,
        ),
        "delete_connection" => {
            encoded(commands::delete_connection(a!("connectionId"), store, runtime).await?)
        }
        "attach_host_folder" => encoded(
            workspace::attach_host_folder(
                a!("environmentId"),
                a!("path"),
                a!("readOnly"),
                store,
                runtime,
                manager,
            )
            .await?,
        ),
        "detach_host_folder" => {
            encoded(workspace::detach_host_folder(a!("shareId"), runtime, manager).await?)
        }
        "copy_files_to_environment" => {
            let id: String = a!("environmentId");
            encoded(crate::file_import::copy_files_into(&id, a!("paths"), a!("destination"), &store, &runtime, move |measurement| {
                if let Ok(value) = serde_json::to_value(measurement) { progress(value); }
            }).await?)
        },
        "list_imported_drives" => encoded(crate::file_import::list_imported_drives(a!("environmentId"), store, runtime)?),
        "set_imported_drive_attached" => {
            let id: String = a!("environmentId");
            let transfer: String = a!("transferId");
            encoded(crate::file_import::set_drive_attached(&id, &transfer, a!("attached"), &store, &runtime).await?)
        },
        "list_environment_services" => encoded(
            workspace::list_environment_services(a!("environmentId"), store, runtime, manager)
                .await?,
        ),
        "get_manual_service_ports" => encoded(workspace::get_manual_service_ports(store)?),
        "set_manual_service_port" => encoded(workspace::set_manual_service_port(
            a!("environmentId"),
            a!("port"),
            a!("present"),
            app.clone(),
            store,
        )?),
        // Only reachable over an OS-authenticated local transport. Webview
        // commands retain their original main-window credential restrictions.
        "publish_environment_service" => {
            // `domain` names a saved domain and implies a Cloudflare account publication.
            let (kind, host_port, cloudflare) = match p["domain"].as_str() {
                Some(name) if p.get("cloudflare").is_none_or(Value::is_null) => {
                    let (options, port) = workspace::cloudflare::domain_account(&store, name)?;
                    (workspace::PublicationKind::Cloudflare, Some(port), Some(options))
                }
                Some(_) => return Err("Use either domain or cloudflare, not both".into()),
                None => (a!("kind"), a!("hostPort"), a!("cloudflare")),
            };
            encoded(workspace::publish_service(a!("environmentId"), a!("port"), kind, host_port, cloudflare, &store, &runtime, &manager).await?)
        }
        "list_saved_domains" => encoded(store.snapshot()?.saved_domains),
        "model_usage" => encoded(crate::model_runner::model_usage(a!("environmentId"), false, app.clone()).await?),
        "reset_model_usage" => encoded(crate::model_runner::model_usage(a!("environmentId"), true, app.clone()).await?),
        "list_volumes" => {
            let scan: Option<bool> = a!("scan");
            let size: Option<bool> = a!("size");
            encoded(crate::file_export::volumes(&store, &runtime, scan.unwrap_or(false), size.unwrap_or(false)).await?)
        }
        "cloud_options" => encoded(crate::cloud_options::cloud_options(a!("provider"), a!("account"), a!("region"), a!("kind")).await?),
        "remove_volume" => {
            let name: String = a!("name");
            encoded(crate::file_export::remove_named_volume(&name, &store, &runtime).await?)
        }
        "copy_files_from_environment" => encoded(crate::file_export::copy_files_from_environment(a!("environmentId"), a!("path"), a!("destination"), store, runtime).await?),
        "copy_files_between_environments" => encoded(crate::file_export::copy_files_between_environments(a!("sourceId"), a!("targetId"), a!("path"), store, runtime).await?),
        "model_chat_history" => encoded(crate::model_runner::model_chat_history(a!("environmentId"), store)?),
        "save_model_chat_history" => encoded(crate::model_runner::save_model_chat_history(a!("environmentId"), a!("history"), app.clone(), store)?),
        "model_api_status" => encoded(crate::model_runner::model_api_status(a!("environmentId"), app.clone()).await?),
        "add_saved_domain" => {
            let hostname: String = a!("hostname");
            let host_port: u16 = a!("hostPort");
            let port: Option<u16> = a!("port");
            let token: String = a!("token");
            let id = uuid::Uuid::new_v4().to_string();
            encoded(workspace::cloudflare::save_preset("public-presets", port.unwrap_or(host_port), &id, &hostname, host_port, &token, &store, &manager).await?)
        }
        "remember_saved_domain" => {
            let id = uuid::Uuid::new_v4().to_string();
            encoded(workspace::cloudflare::copy_saved_to_preset(&arg::<String>(p, "environmentId")?, a!("port"), &id, &store, &manager).await?)
        }
        "start_saved_domain_tunnel" => encoded(workspace::cloudflare::start_domain_tunnel(&arg::<String>(p, "domain")?, &store, &manager).await?),
        "stop_saved_domain_tunnel" => encoded(workspace::cloudflare::stop_domain_tunnel(&arg::<String>(p, "domain")?, &store, &manager).await?),
        "update_saved_domain" => {
            let domain = workspace::cloudflare::find_domain(&store, &arg::<String>(p, "domain")?)?;
            let port: Option<u16> = a!("port");
            let host_port: Option<u16> = a!("hostPort");
            if port.is_none() && host_port.is_none() { return Err("Pass --port and/or --tunnel-port".into()); }
            encoded(workspace::cloudflare::update_preset(&domain.id, port.unwrap_or(domain.port), host_port.unwrap_or(domain.host_port), &store, &manager).await?)
        }
        "remove_saved_domain" => {
            let domain = workspace::cloudflare::find_domain(&store, &arg::<String>(p, "domain")?)?;
            workspace::cloudflare::forget_preset(&domain.credential_environment_id, domain.port, &domain.id, &store, &manager).await?;
            encoded(store.snapshot()?.saved_domains)
        }
        "unpublish_environment_service" => encoded(
            workspace::unpublish_environment_service(a!("publicationId"), manager, store, runtime)
                .await?,
        ),
        "saved_cloudflare_account" => encoded(
            workspace::cloudflare::saved_for_local_client(
                a!("environmentId"),
                a!("port"),
                &store,
                &manager,
            )
            .await?,
        ),
        "forget_cloudflare_account" => encoded(
            workspace::cloudflare::forget_for_local_client(
                a!("environmentId"),
                a!("port"),
                &store,
                &manager,
            )
            .await?,
        ),
        "create_snapshot" => encoded(
            commands::create_snapshot(a!("environmentId"), a!("name"), store, runtime, backup)
                .await?,
        ),
        "delete_snapshot" => {
            encoded(commands::delete_snapshot(a!("snapshotId"), store, runtime, backup).await?)
        }
        "restore_snapshot" => {
            encoded(commands::restore_snapshot(a!("snapshotId"), store, runtime).await?)
        }
        "export_local_backup" => encoded(
            local_backup::export_local_backup(a!("environmentId"), a!("folder"), store, runtime)
                .await?,
        ),
        "import_local_backup" => encoded(
            local_backup::import_local_backup(
                a!("path"),
                a!("targetProvider"),
                store,
                runtime,
                backup,
            )
            .await?,
        ),
        "add_backup_destination" => {
            encoded(commands::add_backup_destination(a!("request"), store, backup).await?)
        }
        "delete_backup_destination" => encoded(commands::delete_backup_destination(
            a!("destinationId"),
            store,
            backup,
        )?),
        "run_backup" => encoded(
            commands::run_backup(
                a!("environmentId"),
                a!("destinationId"),
                store,
                runtime,
                backup,
            )
            .await?,
        ),
        "restore_backup" => {
            encoded(commands::restore_backup(a!("backupId"), store, runtime, backup).await?)
        }
        "update_settings" => {
            encoded(commands::update_settings(a!("settings"), store, runtime, backup).await?)
        }
        "reset_platform_state" => {
            encoded(commands::reset_platform_state(store, runtime, backup).await?)
        }
        "refresh_host_metrics" => encoded(commands::refresh_host_metrics(store, runtime).await?),
        "get_cuda_runtime_status" => {
            encoded(runtime::cuda::get_cuda_runtime_status(a!("storageDrive"), runtime).await?)
        }
        "install_cuda_runtime" => encoded(runtime::cuda::install_cuda_runtime(a!("storageDrive"), runtime).await?),
        "verify_environment_cuda" => encoded(
            runtime::cuda::verify_environment_cuda(a!("environmentId"), store, runtime).await?,
        ),
        "get_shared_gpu_settings" => encoded(runtime::gpu::get_shared_gpu_settings(runtime).await?),
        "set_shared_gpu_selection" => {
            encoded(runtime::gpu::set_shared_gpu_selection(a!("selectedId"), runtime).await?)
        }
        "execute_environment_command" => {
            encoded(commands::execute_environment_command(a!("request"), store, runtime).await?)
        }
        "execute_connected_command" => {
            encoded(commands::execute_connected_command(a!("request"), store, runtime).await?)
        }
        "list_environment_folders" => {
            encoded(commands::list_environment_folders(a!("environmentId"), a!("path"), store, runtime).await?)
        }
        "request_connected_files" => {
            encoded(commands::request_connected_files(a!("environmentId"), a!("request"), store, runtime).await?)
        }
        "read_environment_console" => {
            encoded(commands::read_environment_console(a!("environmentId"), store, runtime).await?)
        }
        "get_guest_session" => {
            encoded(commands::get_guest_session(a!("environmentId"), store, runtime).await?)
        }
        "terminal_action" => {
            workspace::terminal_action_for_owner(
                a!("environmentId"),
                a!("sessionId"),
                a!("action"),
                a!("data"),
                a!("offset"),
                a!("cols"),
                a!("rows"),
                "cli",
                &store,
                &runtime,
                &manager,
            )
            .await
        }
        "prepare_terminal_installer" | "install_terminal_tool" => {
            let command = workspace::installers::prepare_for_owner(
                a!("environmentId"),
                a!("sessionId"),
                a!("tool"),
                "cli",
                &store,
                &runtime,
                &manager,
            )
            .await?;
            if method == "prepare_terminal_installer" {
                return encoded(command);
            }
            use base64::Engine;
            let data = base64::engine::general_purpose::STANDARD.encode(format!("{command}\r"));
            workspace::terminal_action_for_owner(
                a!("environmentId"),
                a!("sessionId"),
                "write".into(),
                Some(data),
                None,
                None,
                None,
                "cli",
                &store,
                &runtime,
                &manager,
            )
            .await?;
            Ok(
                json!({"started":true,"sessionId":p["sessionId"],"message":"Read this terminal's output for install progress and result."}),
            )
        }
        "micro_vm_apps" => {
            guest_apps::micro_vm_apps(
                a!("environmentId"),
                a!("action"),
                a!("sessionId"),
                a!("name"),
                a!("command"),
                a!("package"),
                store,
                runtime,
            )
            .await
        }
        "open_environment_window" => encoded(
            commands::open_environment_window(a!("environmentId"), app.clone(), store).await?,
        ),
        "open_micro_vm_app_window" => encoded(
            guest_apps::open_micro_vm_app_window(
                a!("environmentId"),
                a!("sessionId"),
                app.clone(),
                store,
            )
            .await?,
        ),
        "close_environment_window" => encoded(commands::close_environment_window(window()?)?),
        "list_environment_windows" => encoded(workspace::list_environment_windows(app.clone())),
        "focus_environment_window" => encoded(workspace::focus_environment_window(
            a!("label"),
            app.clone(),
        )?),
        "title_environment_window" => encoded(workspace::title_environment_window(
            a!("environmentId"),
            window()?,
            store,
        )?),
        "set_guest_keyboard_capture" => encoded(guest_keyboard::set_guest_keyboard_capture(
            window()?,
            a!("token"),
            a!("bounds"),
        )?),
        "open_workspace_url" => encoded(workspace::open_workspace_url(a!("url"))?),
        "open_service_window" => encoded(workspace::open_service_window(a!("environmentId"),a!("url"),app.clone(),store).await?),
        // Vault decisions stay in the focused dashboard: the CLI can only bring it forward.
        "open_personal_vault" => {
            let view: Option<String> = a!("view");
            let view = view.unwrap_or_else(|| "home".into());
            if !matches!(view.as_str(), "home" | "approvals" | "add") {
                return Err("view must be home, approvals or add".into());
            }
            crate::require_windows("Personal Vault")?;
            dispatch(app, "app_show", &json!({})).await?;
            let _ = tauri::Emitter::emit_to(app, "main", "yougori-open-vault", &view);
            Ok(json!({"opened": view, "message": "Personal Vault is open in Yougori. Review and decide there; the CLI cannot approve requests or add items."}))
        }
        // Only a reachability flag and a count leave the broker: no item names, clients or values.
        "vault_summary" => {
            #[cfg(windows)]
            {
                let status = yougori_vault::client::call(&yougori_vault::protocol::Request::Status {}).await;
                let waiting = status.as_ref().ok().map(|s| s["pending"].as_array().into_iter().flatten().filter(|r| r["ready"] == true).count());
                Ok(json!({"running": status.is_ok(), "pendingApprovals": waiting.unwrap_or(0)}))
            }
            // The Personal Vault broker ships with the Windows app only.
            #[cfg(not(windows))]
            Ok(json!({"running": false, "pendingApprovals": 0, "supported": false}))
        }
        // The standalone engine has no dashboard. With nothing running it steps aside so the
        // desktop app (which asked, or which the CLI starts next) can own the environments.
        "app_show" if crate::ENGINE_ONLY => {
            let busy = store.snapshot()?.environments.iter()
                .filter(|e| !crate::peer_sharing::is_shared(e) && matches!(e.status, EnvironmentStatus::Running | EnvironmentStatus::Paused | EnvironmentStatus::Provisioning))
                .count();
            if busy > 0 {
                return Err(format!("The background Yougori engine is running {busy} environment{} and has no dashboard. Stop {} (`yougori stop ENV`) or quit the engine (`yougori app quit`), then open Yougori.", if busy == 1 { "" } else { "s" }, if busy == 1 { "it" } else { "them" }));
            }
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                crate::exit_engine(&app, 0);
            });
            Ok(json!({"visible": false, "handover": true}))
        }
        "app_show" => {
            if let Some(window) = app.get_webview_window("main") {
                window.show().map_err(|e| e.to_string())?;
                window.unminimize().map_err(|e| e.to_string())?;
                window.set_focus().map_err(|e| e.to_string())?;
            } else {
                let config = app
                    .config()
                    .app
                    .windows
                    .iter()
                    .find(|w| w.label == "main")
                    .ok_or("Dashboard configuration missing")?;
                tauri::WebviewWindowBuilder::from_config(app, config)
                    .map_err(|e| e.to_string())?
                    .build()
                    .map_err(|e| e.to_string())?;
            }
            Ok(json!({"visible":true}))
        }
        "app_quit" => {
            // Complete the acknowledgement before the event loop exits. The
            // normal ExitRequested handler gracefully tears down all runtimes.
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                crate::exit_engine(&app, 0);
            });
            Ok(json!({"shutdownRequested":true}))
        }
        _ => Err(format!("No backend handler for {method}")),
    }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_resource_edits_preserve_other_limits_and_priority() {
        let mut policy:ResourcePolicy=serde_json::from_value(json!({"cpu":{"min":1,"preferred":2,"max":4,"current":2},"memoryGb":{"min":1,"preferred":4,"max":8,"current":4},"priority":"high"})).unwrap();
        merge_limits(&mut policy, &json!({"cpu":{"preferred":3}})).unwrap();
        assert_eq!(policy.cpu.preferred, 3.0);
        assert_eq!(policy.memory_gb.preferred, 4.0);
        assert_eq!(policy.priority, Priority::High);
        assert!(merge_limits(&mut policy, &json!({"cpu":{"preferrred":8}})).is_err());
        assert!(merge_limits(&mut policy, &json!({"cpu":{"preferred":9}})).is_err());
    }
    #[test]
    fn every_desktop_command_has_a_cli_contract_and_dispatch() {
        let source = include_str!("../lib.rs");
        let handlers = source
            .split("tauri::generate_handler![")
            .nth(1)
            .unwrap()
            .split("])")
            .next()
            .unwrap();
        let dispatch = include_str!("dispatch.rs");
        for line in handlers.lines() {
            let name = line
                .trim()
                .trim_end_matches(',')
                .rsplit("::")
                .next()
                .unwrap_or("");
            if name.is_empty() {
                continue;
            }
            // Vault management is restricted to the focused desktop UI and broker.
            // Never expose its desktop facade through ordinary agent automation.
            if matches!(name, "vault_status" | "vault_control" | "vault_add_items" | "vault_export_plugin" | "vault_bootstrap" | "vault_create" | "vault_connection" | "validate_edit_app_folder" | "save_cloudflare_preset" | "copy_saved_cloudflare_to_preset" | "forget_cloudflare_preset") {
                assert!(yougori_cli::catalog::find(name).is_err());
                continue;
            }
            // Desktop plumbing with no CLI meaning: token streaming over a webview channel,
            // and the one-time move of saved domains out of browser storage.
            if matches!(name, "model_chat_stream" | "model_chat_cancel" | "import_saved_domains") {
                assert!(yougori_cli::catalog::find(name).is_err());
                continue;
            }
            yougori_cli::catalog::find(name)
                .unwrap_or_else(|_| panic!("Missing CLI contract for desktop command {name}"));
            assert!(
                dispatch.contains(&format!("\"{name}\"")),
                "Missing dispatch for {name}"
            );
        }
        for method in yougori_cli::catalog::methods() {
            validate(method.name, &method.example)
                .unwrap_or_else(|e| panic!("{}: {e}", method.name));
        }
    }
}
