mod services;
use crate::{
    automation::dispatch::dispatch, models::*, runtime::RuntimeManager, store::PlatformStore,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::OnceLock,
};
use tauri::Manager;
use crate::AppHandle;
use yougori_cli::manifest::{self, Connection, Environment as Spec, Project, Publication};
static OPERATIONS: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
#[derive(Clone, Default, Serialize, Deserialize)]
struct Registry {
    projects: BTreeMap<String, Record>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Record {
    path: String,
    project: String,
    #[serde(default)]
    ids: BTreeMap<String, String>,
    #[serde(default)]
    applied: BTreeMap<String, Spec>,
    #[serde(default)]
    connections: Vec<String>,
    #[serde(default)]
    pending: BTreeMap<String, String>,
    #[serde(default)]
    bindings: Vec<services::Binding>,
}
fn registry_path(runtime: &RuntimeManager) -> PathBuf {
    runtime.storage_root().join("projects.json")
}
fn registry(runtime: &RuntimeManager) -> Result<Registry, String> {
    let path = registry_path(runtime);
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(Registry::default());
    }
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
        return Err("Project registry is too large".into());
    }
    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn save(runtime: &RuntimeManager, r: &Registry) -> Result<(), String> {
    let mut f =
        tempfile::NamedTempFile::new_in(runtime.storage_root()).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut f, r).map_err(|e| e.to_string())?;
    f.as_file().sync_all().map_err(|e| e.to_string())?;
    f.persist(registry_path(runtime))
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn key(path: &Path) -> String {
    format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()))
}
fn resolve_spec(mut p: Project, root: &Path) -> Result<Project, String> {
    for spec in p.environments.values_mut() {
        if let Some(source) = &spec.source {
            if source != "builtin:alpine" && !Path::new(source).is_absolute() {
                spec.source = Some(
                    root.join(source)
                        .canonicalize()
                        .map_err(|e| format!("Boot source: {e}"))?
                        .to_string_lossy()
                        .into_owned(),
                )
            }
        }
        for bind in spec.volumes.iter_mut().filter(|b| b.bind) {
            let path = root
                .join(&bind.source)
                .canonicalize()
                .map_err(|e| format!("PC folder {}: {e}", bind.source))?;
            if !path.is_dir() {
                return Err("PC bind mounts currently require folders".into());
            }
            bind.source = path.to_string_lossy().into_owned()
        }
        for pc in &mut spec.pc_access {
            pc.path = root
                .join(&pc.path)
                .canonicalize()
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned();
            if let Some(target) = &pc.target {
                spec.volumes.push(manifest::Mount {
                    source: pc.path.clone(),
                    target: target.clone(),
                    read_only: pc.read_only,
                    bind: true,
                })
            }
        }
        if let Some(cloud) = &mut spec.cloud {
            if let Some(file) = cloud["identityFile"].as_str() {
                // Spaces are valid in file names. Only a parsed SSH public key
                // is inline identity data; resolve every relative file at the project.
                if !Path::new(file).is_absolute()
                    && crate::runtime::cloud::public_identity(file)?.is_none()
                {
                    cloud["identityFile"] = root.join(file).to_string_lossy().into_owned().into()
                }
            }
        }
    }
    p.validate()?;
    Ok(p)
}
fn load(path: &str) -> Result<(PathBuf, Project), String> {
    let candidate = Path::new(path);
    let path = if candidate.is_dir() {
        manifest::locate(candidate)?
    } else {
        candidate.canonicalize().map_err(|e| e.to_string())?
    };
    let p = resolve_spec(
        manifest::parse(&manifest::read(&path)?)?,
        path.parent().ok_or("Project needs a folder")?,
    )?;
    Ok((path, p))
}
fn summary(p: &Project, record: Option<&Record>, path: &Path) -> Value {
    let services=p.environments.iter().map(|(name,e)|json!({"name":name,"type":e.kind,"image":e.image,"cpu":e.cpu,"memoryGb":manifest::gib(&e.memory).unwrap_or(0.0),"gpu":e.gpu||e.kind=="gpu","ports":e.ports,"pcAccess":e.pc_access.len()+e.volumes.iter().filter(|m|m.bind).count(),"editPc":e.permissions.edit,"variables":e.environment.keys().collect::<Vec<_>>(),"action":match record.and_then(|r|r.applied.get(name)){None=>"create",Some(old)if old==e=>"unchanged",Some(_)=>"update"}})).collect::<Vec<_>>();
    json!({"path":path,"project":p.project,"environments":services,"connections":p.connections.len(),"publications":p.publish.len(),"notice":"Apply preserves removed environments and data. Image/type changes create a replacement and keep the old environment stopped. down stops project workloads without deleting volumes."})
}
#[tauri::command]
pub async fn inspect_project(path: String, app: AppHandle) -> Result<Value, String> {
    let (path, p) = load(&path)?;
    let r = registry(&app.state::<RuntimeManager>())?;
    Ok(summary(&p, r.projects.get(&key(&path)), &path))
}

#[tauri::command]
pub async fn discover_projects(app: AppHandle) -> Result<Value, String> {
    let runtime = app.state::<RuntimeManager>();
    let r = registry(&runtime)?;
    let mut paths = r
        .projects
        .values()
        .map(|r| PathBuf::from(&r.path))
        .collect::<Vec<_>>();
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(path) = manifest::locate(&cwd) {
            paths.push(path)
        }
    }
    if let Some(terminals) = app.try_state::<crate::host_terminal::HostTerminalManager>() {
        for cwd in terminals.project_directories() {
            if let Ok(path) = manifest::locate(&cwd) {
                paths.push(path)
            }
        }
    }
    if let Ok(root) = crate::host_terminal::default_workspace() {
        for folder in std::iter::once(root.clone()).chain(
            std::fs::read_dir(&root)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .take(64),
        ) {
            for name in ["yougori.yaml", "yougori.yml"] {
                let path = folder.join(name);
                if path.is_file() {
                    paths.push(path)
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    let mut projects = Vec::new();
    for path in paths {
        if !path.is_file() {
            continue;
        }
        match load(&path.to_string_lossy()) {
            Ok((path, p)) => projects.push(summary(&p, r.projects.get(&key(&path)), &path)),
            Err(e) => projects.push(json!({"path":path,"error":e})),
        }
    }
    Ok(json!(projects))
}
#[tauri::command]
pub async fn import_compose(
    path: String,
    write: bool,
    project: Option<Project>,
    app: AppHandle,
) -> Result<Value, String> {
    let path = Path::new(&path).canonicalize().map_err(|e| e.to_string())?;
    let project = match project {
        Some(project) => {
            project.validate()?;
            project
        }
        None => manifest::compose::convert(&path)?,
    };
    let output = path
        .parent()
        .ok_or("Compose folder unavailable")?
        .join("yougori.yaml");
    let mut result = summary(&project, None, &output);
    result["generated"] = json!(false);
    if write {
        // No overwrite, including a concurrently created project file. Secrets use owner-only tempfile permissions.
        let mut f =
            tempfile::NamedTempFile::new_in(output.parent().unwrap()).map_err(|e| e.to_string())?;
        use std::io::Write;
        f.write_all(serde_yaml_ng_text(&project)?.as_bytes())
            .map_err(|e| e.to_string())?;
        f.as_file().sync_all().map_err(|e| e.to_string())?;
        f.persist_noclobber(&output)
            .map_err(|e| format!("Cannot create yougori.yaml (existing files are kept): {e}"))?;
        let runtime = app.state::<RuntimeManager>();
        let _lock = OPERATIONS.get_or_init(Default::default).lock().await;
        let mut r = registry(&runtime)?;
        r.projects.entry(key(&output)).or_insert(Record {
            path: output.to_string_lossy().into_owned(),
            project: project.project.clone(),
            ids: BTreeMap::new(),
            applied: BTreeMap::new(),
            connections: vec![],
            pending: BTreeMap::new(),
            bindings: vec![],
        });
        save(&runtime, &r)?;
        result["generated"] = json!(true);
    }
    Ok(result)
}
fn serde_yaml_ng_text(project: &Project) -> Result<String, String> {
    yougori_cli::manifest::to_yaml(project)
}

async fn call(app: &AppHandle, method: &str, p: Value) -> Result<Value, String> {
    dispatch(app, method, &p).await
}
fn node(app: &AppHandle, id: &str) -> Result<crate::models::Environment, String> {
    app.state::<PlatformStore>()
        .snapshot()?
        .environments
        .into_iter()
        .find(|e| e.id == id)
        .ok_or("Project environment was removed; run apply to recreate it".into())
}
async fn status(app: &AppHandle, id: &str, running: bool) -> Result<(), String> {
    let e = node(app, id)?;
    if (e.status == EnvironmentStatus::Running) == running && e.status != EnvironmentStatus::Error {
        return Ok(());
    }
    call(
        app,
        "set_environment_status",
        json!({"environmentId":id,"status":if running{"running"}else{"stopped"}}),
    )
    .await?;
    Ok(())
}
async fn services(app: &AppHandle, id: &str, spec: &Spec) -> Result<(), String> {
    for pc in spec.pc_access.iter().filter(|p| p.target.is_none()) {
        call(
            app,
            "attach_host_folder",
            json!({"environmentId":id,"path":pc.path,"readOnly":pc.read_only}),
        )
        .await?;
    }
    let current = call(
        app,
        "list_environment_services",
        json!({"environmentId":id}),
    )
    .await?;
    for port in &spec.ports {
        let (guest, host) = port.values()?;
        call(
            app,
            "set_manual_service_port",
            json!({"environmentId":id,"port":guest,"present":true}),
        )
        .await?;
        if let Some(host) = host {
            let exists = current["publications"].as_array().is_some_and(|a| {
                a.iter()
                    .any(|p| p["port"] == guest && p["hostPort"] == host && p["kind"] == "loopback")
            });
            if !exists {
                call(
                    app,
                    "publish_environment_service",
                    json!({"environmentId":id,"port":guest,"hostPort":host,"kind":"loopback"}),
                )
                .await?;
            }
        }
    }
    Ok(())
}
async fn configure(app: &AppHandle, id: &str, spec: &Spec, project: &str) -> Result<(), String> {
    let env = node(app, id)?;
    let request = spec.create_request(project, "unused")?;
    call(
        app,
        "update_resource_policy",
        json!({"environmentId":id,"resourcePolicy":request["resourcePolicy"]}),
    )
    .await?;
    let capacity = manifest::gib(&spec.storage)?;
    if env
        .storage_limit_gb
        .is_some_and(|old| (old - capacity).abs() > 0.01)
    {
        call(
            app,
            "expand_environment_storage",
            json!({"environmentId":id,"capacityGb":capacity}),
        )
        .await?;
    }
    if env.network_access != spec.internet {
        call(
            app,
            "update_container_network",
            json!({"environmentId":id,"enabled":spec.internet}),
        )
        .await?;
    }
    if env.kind == EnvironmentKind::Container {
        let runtime = app.state::<RuntimeManager>();
        let mut options = spec.options(project)?;
        let old = runtime.workload_options(&env.id)?;
        options.hosts = old.hosts.clone();
        if options != old {
            runtime.save_workload_options(&env.id, &options)?;
            if let Err(e) = runtime.update_workload_configuration(&env).await {
                runtime.save_workload_options(&env.id, &old)?;
                return Err(e);
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn project_action(path: String, action: String, app: AppHandle) -> Result<Value, String> {
    if !["up", "apply", "down"].contains(&action.as_str()) {
        return Err("Choose up, apply or down".into());
    }
    let _lock = OPERATIONS.get_or_init(Default::default).lock().await;
    let (path, p) = if action == "down" {
        let path = Path::new(&path).canonicalize().map_err(|e| e.to_string())?;
        let p = manifest::parse(&manifest::read(&path)?)?;
        (path, p)
    } else {
        load(&path)?
    };
    let runtime = app.state::<RuntimeManager>();
    let mut r = registry(&runtime)?;
    let k = key(&path);
    if r.projects
        .values()
        .any(|v| v.project == p.project && v.path != path.to_string_lossy())
    {
        return Err(
            "Another project folder already uses this project name; choose a unique project name"
                .into(),
        );
    }
    let mut record = r.projects.get(&k).cloned().unwrap_or(Record {
        path: path.to_string_lossy().into_owned(),
        project: p.project.clone(),
        ids: BTreeMap::new(),
        applied: BTreeMap::new(),
        connections: vec![],
        pending: BTreeMap::new(),
        bindings: vec![],
    });
    if record.project != p.project {
        return Err("Project name changed. Use a separate project folder for a new project, or restore the original name.".into());
    }
    if action == "down" {
        services::reconcile(&app, &p, &mut record, &mut r, &k, true).await?;
        for id in record.ids.values().rev() {
            if node(&app, id).is_ok() {
                status(&app, id, false).await?
            }
        }
        return Ok(
            json!({"project":p.project,"status":"stopped","environments":record.ids,"dataPreserved":true}),
        );
    }
    // Validate all immutable/precondition checks before mutating the first node.
    for (name, spec) in &p.environments {
        if let Some(cloud) = &spec.cloud {
            let mut cloud = cloud.clone();
            cloud["name"] = json!(format!("{}-{name}", p.project));
            let profile: crate::runtime::cloud::Profile =
                serde_json::from_value(cloud).map_err(|e| e.to_string())?;
            crate::runtime::cloud::validate(&profile, true)?;
        }
        if spec.kind == "microvm" && !spec.image.is_empty() && spec.volumes.iter().any(|v| v.bind) {
            return Err("For microVM images, use named guest volumes or connect a PC folder through My PC after starting".into());
        }
    }
    for name in p.order()? {
        let spec = &p.environments[&name];
        let old = record.applied.get(&name);
        let replace = old.is_some_and(|old| {
            old.kind != spec.kind
                || old.image != spec.image
                || old.source != spec.source
                || old.gpu != spec.gpu
                || old.storage_drive != spec.storage_drive
                || old.cloud != spec.cloud
                || old.shared != spec.shared
                || (spec.kind == "microvm"
                    && !spec.image.is_empty()
                    && old.options(&p.project).ok() != spec.options(&p.project).ok())
        });
        let mut id = record
            .ids
            .get(&name)
            .filter(|id| node(&app, id).is_ok())
            .cloned();
        if replace {
            if let Some(old) = &id {
                status(&app, old, false).await?
            }
            id = None;
        }
        if id.is_none() {
            let desired = record
                .pending
                .entry(name.clone())
                .or_insert_with(|| {
                    if replace {
                        format!(
                            "{}-{}-{}",
                            p.project,
                            name,
                            &uuid::Uuid::new_v4().simple().to_string()[..6]
                        )
                    } else {
                        format!("{}-{name}", p.project)
                    }
                })
                .clone();
            r.projects.insert(k.clone(), record.clone());
            save(&runtime, &r)?;
            let ownership = format!("Yougori project {} / {}", k, name);
            if let Some(existing) = app
                .state::<PlatformStore>()
                .snapshot()?
                .environments
                .iter()
                .find(|e| e.name == desired)
                .cloned()
            {
                if existing.description != ownership {
                    return Err(format!("{desired} already exists outside this project"));
                }
                if existing.status == EnvironmentStatus::Error {
                    return Err(format!(
                        "{desired} needs attention; inspect its error before retrying"
                    ));
                }
                id = Some(existing.id)
            } else {
                let before_ids = app
                    .state::<PlatformStore>()
                    .snapshot()?
                    .environments
                    .into_iter()
                    .map(|e| e.id)
                    .collect::<Vec<_>>();
                let result = if spec.kind == "cloud" {
                    let mut profile = spec.cloud.clone().unwrap();
                    profile["name"] = desired.clone().into();
                    call(&app, "add_cloud_environment", json!({"request":profile})).await?
                } else if spec.kind == "shared" {
                    call(&app,"import_environment_share",json!({"invitation":serde_json::from_str::<Value>(spec.shared.as_deref().unwrap_or("")).map_err(|_|"shared must contain the copied JSON invitation")?})).await?
                } else {
                    let mut request = spec.create_request(&p.project, &name)?;
                    request["name"] = desired.clone().into();
                    request["description"] = ownership.clone().into();
                    if spec.kind == "microvm" && !spec.image.is_empty() {
                        request["microvmWorkload"] =
                            json!({"image":spec.image,"options":spec.options(&p.project)?});
                    }
                    run_workload(request, false, app.clone()).await?
                };
                id = result["environments"]
                    .as_array()
                    .and_then(|a| a.iter().find(|v| v["name"] == desired))
                    .and_then(|v| v["id"].as_str())
                    .map(str::to_owned)
                    .or_else(|| result["id"].as_str().map(str::to_owned));
                if id.is_none() {
                    id = app
                        .state::<PlatformStore>()
                        .snapshot()?
                        .environments
                        .iter()
                        .find(|e| {
                            e.name == desired
                                || (!before_ids.contains(&e.id)
                                    && matches!(spec.kind.as_str(), "cloud" | "shared"))
                        })
                        .map(|e| e.id.clone())
                }
                if let Some(id) = &id {
                    if matches!(spec.kind.as_str(), "cloud" | "shared") {
                        app.state::<PlatformStore>().mutate(|state| {
                            let e = state
                                .environments
                                .iter_mut()
                                .find(|e| &e.id == id)
                                .ok_or("Created node unavailable")?;
                            e.name = desired.clone();
                            e.description = ownership.clone();
                            Ok(())
                        })?;
                    }
                }
            }
            let created = id
                .clone()
                .ok_or("Created project node was not returned; inspect Desktop before retrying")?;
            record.ids.insert(name.clone(), created);
            record.pending.remove(&name);
            record.applied.insert(name.clone(), spec.clone());
            r.projects.insert(k.clone(), record.clone());
            save(&runtime, &r)?;
        } else if old != Some(spec) && !matches!(spec.kind.as_str(), "cloud" | "shared") {
            let id = id.as_ref().unwrap();
            status(&app, id, false).await?;
            configure(&app, id, spec, &p.project).await?;
            record.applied.insert(name.clone(), spec.clone());
            r.projects.insert(k.clone(), record.clone());
            save(&runtime, &r)?;
        }
        let _ = id;
    }
    // Service names resolve to stable private addresses; aliases grant no network access.
    for (name, spec) in &p.environments {
        if matches!(spec.kind.as_str(), "container" | "gpu") {
            let id = &record.ids[name];
            let env = node(&app, id)?;
            let mut options = runtime.workload_options(env.runtime_id.as_deref().unwrap_or(id))?;
            let hosts = p
                .environments
                .keys()
                .map(|name| {
                    (
                        name.clone(),
                        crate::runtime::fabric::ip_text(&record.ids[name]),
                    )
                })
                .collect();
            if options.hosts != hosts {
                status(&app, id, false).await?;
                options.hosts = hosts;
                runtime.save_workload_options(id, &options)?;
                runtime.update_workload_configuration(&env).await?;
            }
        }
    }
    let mut desired = Vec::new();
    for c in &p.connections {
        let (a, b) = c.endpoints()?;
        let (permissions, ports) = match c {
            Connection::Detailed {
                permissions, ports, ..
            } => (
                if permissions.is_empty() {
                    vec![if ports.is_empty() {
                        "network".to_owned()
                    } else {
                        "ports".to_owned()
                    }]
                } else {
                    permissions.clone()
                },
                ports.iter().map(u16::to_string).collect::<Vec<_>>(),
            ),
            _ => (vec!["network".to_owned()], vec![]),
        };
        desired.push(json!({"sourceId":record.ids[a],"targetId":record.ids[b],"direction":"oneWay","permissions":permissions,"ports":ports}));
    }
    let matches = |c: &crate::models::Connection, d: &Value| -> bool {
        c.source_id == d["sourceId"].as_str().unwrap_or("")
            && c.target_id == d["targetId"].as_str().unwrap_or("")
            && serde_json::to_value(&c.permissions).ok().as_ref() == Some(&d["permissions"])
            && serde_json::to_value(&c.ports).ok().as_ref() == Some(&d["ports"])
            && c.direction == ConnectionDirection::OneWay
            && c.active
    };
    let current = app.state::<PlatformStore>().snapshot()?;
    for id in record.connections.clone() {
        if let Some(c) = current.connections.iter().find(|c| c.id == id) {
            if desired.iter().any(|d| matches(c, d)) {
                continue;
            }
            call(&app, "delete_connection", json!({"connectionId":id})).await?;
        }
        record.connections.retain(|v| v != &id);
    }
    r.projects.insert(k.clone(), record.clone());
    save(&runtime, &r)?;
    for request in desired {
        let before = app.state::<PlatformStore>().snapshot()?.connections;
        if before.iter().any(|c| matches(c, &request)) {
            continue;
        }
        call(&app, "create_connection", json!({"request":request})).await?;
        let after = app.state::<PlatformStore>().snapshot()?.connections;
        for conn in after {
            if !before.iter().any(|v| v.id == conn.id) {
                record.connections.push(conn.id)
            }
        }
        r.projects.insert(k.clone(), record.clone());
        save(&runtime, &r)?;
    }
    for name in p.order()? {
        let id = &record.ids[&name];
        status(&app, id, true).await?;
    }
    services::reconcile(&app, &p, &mut record, &mut r, &k, false).await?;
    r.projects.insert(k, record.clone());
    save(&runtime, &r)?;
    Ok(
        json!({"project":p.project,"status":"running","environments":record.ids,"dataPreserved":true}),
    )
}

#[tauri::command]
pub async fn run_workload(
    mut request: Value,
    start: bool,
    app: AppHandle,
) -> Result<Value, String> {
    let runtime = app.state::<RuntimeManager>();
    let micro = request
        .as_object_mut()
        .ok_or("Invalid workload request")?
        .remove("microvmWorkload");
    if let Some(spec) = &micro {
        let options: yougori_cli::workload::Options =
            serde_json::from_value(spec["options"].clone()).map_err(|e| e.to_string())?;
        options.validate()?;
        if !options.binds.is_empty() {
            return Err("MicroVM image workloads use guest volumes; connect PC folders with My PC after starting".into());
        }
    }
    let ports = request
        .as_object_mut()
        .ok_or("Invalid workload request")?
        .remove("ports")
        .unwrap_or(json!([]));
    let ports: Vec<manifest::Port> = serde_json::from_value(ports).map_err(|e| e.to_string())?;
    for port in &ports {
        port.values()?;
    }
    let typed: CreateEnvironmentRequest =
        serde_json::from_value(request.clone()).map_err(|e| format!("Invalid workload: {e}"))?;
    if let Some(options) = &typed.workload {
        options.validate()?;
    }
    if typed.provider == RuntimeProviderKind::YougoriCuda && !typed.auto_setup_cuda {
        let selected = runtime.storage_runtime_on_drive(typed.storage_drive.as_deref())?;
        let cuda_runtime = selected.as_deref().unwrap_or(&runtime);
        let status = cuda_runtime.cuda_status().await;
        if !status.supported {
            return Err(status.detail);
        }
        if !status.installed || status.update_available {
            cuda_runtime.install_cuda().await?;
        }
    }
    let name = typed.name;
    let result = call(&app, "create_environment", json!({"request":request})).await?;
    let id = result["environments"]
        .as_array()
        .and_then(|a| a.iter().find(|v| v["name"] == name))
        .and_then(|v| v["id"].as_str())
        .ok_or("Created workload was not returned")?
        .to_owned();
    if let Some(micro) = micro {
        runtime.save_micro_workload(&id, &micro)?;
    }
    if start {
        status(&app, &id, true).await.map_err(|error| format!("Environment {id} was created but could not start: {error}. Inspect or retry starting this environment before creating another."))?;
        let spec = Spec {
            ports,
            ..Default::default()
        };
        services(&app, &id, &spec).await.map_err(|error| format!("Environment {id} was created but port setup failed: {error}. Inspect its state and retry publishing on this environment before creating another."))?;
    }
    Ok(json!({"id":id,"name":name,"started":start}))
}

#[cfg(test)]
mod tests;
