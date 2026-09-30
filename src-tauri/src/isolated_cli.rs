//! The dashboard CLI runs in a managed microVM, with no implicit PC mounts.
use crate::{commands,models::*,runtime::RuntimeManager,store::PlatformStore};
use tauri::{Manager, State};
use crate::AppHandle;
use serde_json::{json,Value};
static CREATE_LOCK:tokio::sync::Mutex<()>=tokio::sync::Mutex::const_new(());

#[tauri::command]
pub async fn open_isolated_cli(app:AppHandle,store:State<'_,PlatformStore>,runtime:State<'_,RuntimeManager>)->Result<String,String>{
    let _serial=CREATE_LOCK.lock().await;
    let state=store.snapshot()?;
    let existing=state.cli_environment_id.as_ref().and_then(|id|state.environments.iter().find(|e|&e.id==id)).cloned();
    let id=if let Some(env)=existing{
        if env.kind!=EnvironmentKind::MicroVm||env.provider!=Some(RuntimeProviderKind::Qemu){return Err("The configured CLI environment is not an isolated microVM".into())}env.id
    }else{
        let name=if state.environments.iter().any(|e|e.name=="Yougori CLI"){format!("Yougori CLI {}",&uuid::Uuid::new_v4().simple().to_string()[..6])}else{"Yougori CLI".into()};
        let request:CreateEnvironmentRequest=serde_json::from_value(json!({"name":name,"kind":"microVm","provider":"qemu","runtime":"builtin:alpine","storageGb":6,"networkAccess":true,"description":"Isolated Yougori CLI. No PC access by default. Grant selected PC folders and environment control explicitly.","resourcePolicy":{"cpu":{"min":1,"preferred":1,"max":2},"memoryGb":{"min":0.5,"preferred":0.5,"max":2},"priority":"normal"}})).map_err(|e|e.to_string())?;
        let state=commands::create_environment(request,app.clone(),store.clone(),runtime.clone()).await?;
        let id=state.environments.iter().find(|e|e.name==name).ok_or("CLI environment was not created")?.id.clone();
        store.mutate(|s|{s.cli_environment_id=Some(id.clone());Ok(())})?;id
    };
    if store.snapshot()?.environments.iter().find(|e|e.id==id).is_some_and(|e|e.status!=EnvironmentStatus::Running){commands::set_environment_status(id.clone(),EnvironmentStatus::Running,store.clone(),runtime.clone()).await?;}
    let ready=runtime.execute_micro_vm_command(&id,"true").await?;
    if ready.exit_code!=0{return Err("The CLI guest shell is not ready".into())}
    Ok(id)
}

#[tauri::command]
pub async fn grant_isolated_cli_environment(environment_id:String,permission:String,app:AppHandle)->Result<Value,String>{
    let store=app.state::<PlatformStore>();let runtime=app.state::<RuntimeManager>();
    let state=store.snapshot()?;let cli_id=state.cli_environment_id.as_ref().ok_or("Open the isolated CLI first")?;
    if cli_id==&environment_id{return Err("This is the CLI workspace itself; its shell already operates locally".into())}
    let cli=state.environments.iter().find(|e|&e.id==cli_id&&e.status==EnvironmentStatus::Running&&e.kind==EnvironmentKind::MicroVm).ok_or("Start the isolated CLI first")?;
    let name=state.environments.iter().find(|e|e.id==environment_id).ok_or("Environment not found")?.name.clone();
    let (grant,invitation)=crate::peer_sharing::cli_grant(environment_id.clone(),permission,app.clone()).await?;
    if let Err(error)=runtime.workspace_request(cli,"/v1/cli/context",json!({"contextId":environment_id,"context":{"name":name,"invitation":invitation}})).await{
        crate::peer_sharing::revoke_environment_share(grant.id,app.clone(),app.state::<crate::peer_sharing::Sharing>()).await?;return Err(error)
    }
    Ok(json!({"grant":grant,"command":format!("yougori exec {environment_id} -- sh -lc 'pwd'")}))
}
