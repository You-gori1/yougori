use super::*;
use crate::runtime::cloud::{HostKey, Profile};

#[tauri::command]
pub async fn test_cloud_connection(
    mut request: Profile,
    environment_id: Option<String>,
    store: State<'_, PlatformStore>,
    runtime: State<'_, RuntimeManager>,
) -> Result<Profile, String> {
    if let Some(id) = environment_id {
        if !store.snapshot()?.environments.iter().any(|e| e.id == id && e.kind == EnvironmentKind::Cloud && !crate::peer_sharing::is_shared(e)) {
            return Err("Cloud environment not found".into());
        }
        // Editing credentials for the same server must keep its existing pin.
        if let Ok(saved) = runtime.cloud.profile(&id) {
            if saved.host == request.host && saved.port == request.port {
                request.host_key = saved.host_key;
            }
        }
    }
    crate::runtime::cloud::test_connection(request).await
}

#[tauri::command]
pub async fn configure_cloud_environment(environment_id:String,request:Profile,store:State<'_,PlatformStore>,runtime:State<'_,RuntimeManager>)->Result<PlatformState,String>{
    crate::runtime::cloud::validate(&request,true)?;
    let environment=store.snapshot()?.environments.into_iter().find(|e|e.id==environment_id&&e.kind==EnvironmentKind::Cloud&&!crate::peer_sharing::is_shared(e)).ok_or("Cloud environment not found")?;
    if runtime.cloud.connected(&environment_id).await{return Err("Disconnect the cloud environment before changing its SSH identity".into())}
    runtime.cloud.save(&environment_id,&request)?;
    store.mutate(|s|{if let Some(e)=s.environments.iter_mut().find(|e|e.id==environment.id){e.runtime=format!("{} · {}@{}:{}",request.vendor,request.username,request.host,request.port);e.last_error=None;}Ok(())})
}

#[tauri::command]
pub async fn scan_cloud_host(host: String, port: u16) -> Result<Vec<HostKey>, String> {
    crate::runtime::cloud::scan(host, port).await
}
#[tauri::command]
pub fn add_cloud_environment(
    request: Profile,
    store: State<'_, PlatformStore>,
    runtime: State<'_, RuntimeManager>,
) -> Result<PlatformState, String> {
    crate::runtime::cloud::validate(&request, true)?;
    let id = format!("env-{}", Uuid::new_v4());
    let name = request.name.trim().to_owned();
    if store
        .snapshot()?
        .environments
        .iter()
        .any(|e| e.name.eq_ignore_ascii_case(&name))
    {
        return Err("An environment with this name already exists".into());
    }
    runtime.cloud.save(&id, &request)?;
    let result=store.mutate(|state| {
        if state.environments.iter().any(|e|e.name.eq_ignore_ascii_case(&name)) {return Err("An environment with this name already exists".into());}
        let range=ResourceRange{min:0.0,preferred:0.0,max:0.0,current:0.0};
        state.environments.push(Environment{
            id:id.clone(),name,kind:EnvironmentKind::Cloud,status:EnvironmentStatus::Stopped,
            runtime:format!("{} · {}@{}:{}",request.vendor,request.username,request.host,request.port),
            provider:Some(RuntimeProviderKind::CloudSsh),runtime_id:Some(id.clone()),runtime_path:None,
            control_endpoint:None,console_endpoint:None,container_command:None,network_access:false,gpu_access:false,
            sandbox_policy:None,last_error:None,description:"Existing Linux cloud server · private SSH connection · power managed outside Yougori".into(),
            branch_type:None,created_at:now(),last_opened_at:None,cpu_usage:0.0,memory_usage_gb:0.0,storage_delta_gb:0.0,storage_limit_gb:None,storage_drive:None,network_rx_mbps:0.0,
            resource_policy:ResourcePolicy{cpu:range.clone(),memory_gb:range,priority:Priority::Normal,dynamic:true},
        });
        Ok(())
    });
    if result.is_err() {
        let _ = runtime.cloud.forget(&id);
    }
    result
}
#[tauri::command]
pub async fn get_cloud_connection(
    environment_id: String,
    store: State<'_, PlatformStore>,
    runtime: State<'_, RuntimeManager>,
) -> Result<serde_json::Value, String> {
    let state = store.snapshot()?;
    if !state
        .environments
        .iter()
        .any(|e| e.id == environment_id && e.kind == EnvironmentKind::Cloud)
    {
        return Err("Cloud environment not found".into());
    }
    let profile = runtime.cloud.profile(&environment_id)?;
    let info = runtime
        .cloud
        .session(&environment_id)
        .await
        .ok()
        .map(|s| s.info);
    Ok(serde_json::json!({"profile":profile,"connection":info}))
}
