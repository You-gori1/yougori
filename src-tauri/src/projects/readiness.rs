//! An address is never a readiness claim. Probes return status codes, not bodies or credentials.
use super::*;
use crate::workspace::{WorkspaceManager, PublicationKind, BoxStream};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use yougori_cli::manifest::HealthCheck;
static HEALTH_LOCK:OnceLock<std::sync::Mutex<()>>=OnceLock::new();

fn stage(status: &str, detail: &str) -> Value { json!({"status":status,"detail":detail}) }
fn status_stage(status: Result<u16,String>, expected: u16) -> Value {
    match status { Ok(code) => json!({"status":if code==expected{"ready"}else{"failed"},"httpStatus":code,"expectedStatus":expected}), Err(detail) => stage("failed", &detail) }
}
pub(crate) fn configured_probe(environment_id: &str, runtime: &RuntimeManager) -> Option<HealthCheck> {
    if let Ok(probes) = saved_probes(runtime) { if let Some(probe) = probes.get(environment_id) { return Some(probe.clone()) } }
    registry(runtime).ok()?.projects.values().find_map(|record| record.ids.iter().find_map(|(name,id)| {
        (id == environment_id).then(|| record.applied.get(name).and_then(|spec| spec.health.clone())).flatten()
    }))
}
fn saved_probes(runtime:&RuntimeManager)->Result<BTreeMap<String,HealthCheck>,String> {
    let path=runtime.storage_root().join("deployment-health.json");
    if !path.exists(){return Ok(BTreeMap::new())}
    if std::fs::metadata(&path).map_err(|e|e.to_string())?.len()>1024*1024{return Err("Saved health checks exceed 1 MiB".into())}
    serde_json::from_slice(&std::fs::read(path).map_err(|e|e.to_string())?).map_err(|_|"Invalid saved deployment health checks".into())
}
fn persist_probes(runtime:&RuntimeManager,probes:&BTreeMap<String,HealthCheck>)->Result<(),String>{
    let mut file=tempfile::NamedTempFile::new_in(runtime.storage_root()).map_err(|e|e.to_string())?;
    serde_json::to_writer(&mut file,probes).map_err(|e|e.to_string())?;file.as_file().sync_all().map_err(|e|e.to_string())?;
    file.persist(runtime.storage_root().join("deployment-health.json")).map_err(|e|e.to_string())?;Ok(())
}
pub(super) fn reconcile_project_probes(project:&Project,record:&Record,runtime:&RuntimeManager)->Result<(),String>{
    let _guard=HEALTH_LOCK.get_or_init(Default::default).lock().map_err(|_|"Health check registry is busy")?;
    let mut probes=saved_probes(runtime)?;let original=probes.clone();
    for(name,spec)in &project.environments {
        let Some(id)=record.ids.get(name)else{continue};
        if let Some(probe)=&spec.health{probes.insert(id.clone(),probe.clone());}
        else if let Some(old)=record.desired.as_ref().and_then(|previous|previous.environments.get(name)).and_then(|spec|spec.health.as_ref()){
            if probes.get(id)==Some(old){probes.remove(id);}
        }
    }
    if probes!=original{persist_probes(runtime,&probes)?;}Ok(())
}
#[tauri::command]
pub fn get_environment_health_check(environment_id:String,runtime:tauri::State<'_,RuntimeManager>)->Option<HealthCheck>{configured_probe(&environment_id,&runtime)}
#[tauri::command]
pub fn set_environment_health_check(environment_id:String,health:Option<HealthCheck>,app:AppHandle)->Result<Value,String> {
    node(&app,&environment_id)?;
    if let Some(probe)=&health{probe.validate()?;}
    let runtime=app.state::<RuntimeManager>();
    let _guard=HEALTH_LOCK.get_or_init(Default::default).lock().map_err(|_|"Health check registry is busy")?;
    let mut probes=saved_probes(&runtime)?;
    if let Some(probe)=health{probes.insert(environment_id.clone(),probe);}else{probes.remove(&environment_id);}
    persist_probes(&runtime,&probes)?;
    Ok(json!({"environmentId":environment_id,"configured":probes.contains_key(&environment_id),"secretValuesStored":false}))
}

async fn local_probe(env: &crate::models::Environment, probe: &HealthCheck, token: Option<&str>, runtime: &RuntimeManager) -> Result<u16,String> {
    tokio::time::timeout(Duration::from_secs(probe.timeout_seconds), async {
        let mut stream: BoxStream = if env.kind == EnvironmentKind::Cloud {
            Box::new(runtime.cloud.service_stream(&env.id, probe.port).await.map_err(|_| "Cannot connect to the application through its cloud connection")?)
        } else {
            let (endpoint, credential) = runtime.workspace_endpoint(env).await.map_err(|_| "Guest control connection is not ready")?;
            Box::new(crate::workspace::agent_stream(&endpoint, &credential, env.runtime_id.as_deref().unwrap_or(&env.id), probe.port).await.map_err(|_| "Application port is not accepting connections")?)
        };
        let body=probe.body.as_ref().map(Value::to_string).unwrap_or_default();
        let authorization=token.map(|v|format!("Authorization: Bearer {v}\r\n")).unwrap_or_default();
        let request=format!("{} {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\n{authorization}Content-Length: {}\r\n\r\n{body}", probe.method,probe.path,body.len());
        stream.write_all(request.as_bytes()).await.map_err(|_| "Cannot send application probe")?;
        let mut line=String::new();
        BufReader::new(stream.take(8192)).read_line(&mut line).await.map_err(|_| "Application returned an invalid HTTP status")?;
        http_status(&line)
    }).await.map_err(|_| "Application probe exceeded its configured timeout".to_string())?
}
fn http_status(line: &str) -> Result<u16,String> {
    let mut words=line.split_whitespace();
    if !words.next().is_some_and(|s| s.starts_with("HTTP/1.")) { return Err("Application did not return an HTTP response".into()) }
    words.next().and_then(|v|v.parse::<u16>().ok()).filter(|v| (100..=599).contains(v)).ok_or("Application returned an invalid HTTP status".into())
}
async fn public_probe(base: &str, probe: &HealthCheck, token: Option<&str>) -> Result<u16,String> {
    let client=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(probe.timeout_seconds)).build().map_err(|_| "Cannot initialize public readiness check")?;
    public_probe_with_client(base,probe,token,&client).await
}
async fn public_probe_with_client(base: &str, probe: &HealthCheck, token: Option<&str>,client:&reqwest::Client) -> Result<u16,String> {
    let base=url::Url::parse(base).map_err(|_| "Publication URL is invalid")?;
    if base.scheme()!="https" || base.host_str().is_none() || !base.username().is_empty() || base.password().is_some() { return Err("Public readiness requires a credential-free HTTPS URL".into()) }
    let url=base.join(&probe.path).map_err(|_| "Health path is invalid")?;
    if url.origin()!=base.origin() { return Err("Health path must stay on this publication's origin".into()) }
    let method=reqwest::Method::from_bytes(probe.method.as_bytes()).map_err(|_| "Invalid health method")?;
    let mut request=client.request(method,url);
    if let Some(token)=token { request=request.bearer_auth(token); }
    if let Some(body)=&probe.body { request=request.json(body); }
    Ok(request.send().await.map_err(|_| "Public HTTPS probe could not connect or timed out")?.status().as_u16())
}

pub(crate) async fn environment_readiness(environment_id: &str, explicit_probe: Option<&HealthCheck>, store: &PlatformStore, runtime: &RuntimeManager, manager: &WorkspaceManager) -> Result<Value,String> {
    let state=store.snapshot()?;
    let env=state.environments.iter().find(|e|e.id==environment_id).ok_or("Environment not found")?;
    let running=env.status==EnvironmentStatus::Running;
    let configured=configured_probe(environment_id,runtime);
    let probe=explicit_probe.or(configured.as_ref());
    if let Some(probe)=probe { probe.validate()?; }
    let publications=serde_json::to_value(crate::workspace::publication_metadata(environment_id,store,manager).await?).map_err(|_|"Cannot summarize publication metadata")?.as_array().cloned().unwrap_or_default();
    let public=publications.iter().filter(|p|p["kind"]==json!(PublicationKind::Cloudflare)).collect::<Vec<_>>();
    let public_configured=!public.is_empty();
    let tunnel_ready=public.iter().all(|p|p["status"]=="active");
    let mut stages=json!({"saved":stage("ready","Environment exists in persistent state"),"running":stage(if running{"ready"}else{"failed"},if running{"Runtime reports running"}else{"Runtime is stopped or failed"}),"process":stage("unverified","No application probe was configured"),"localHttp":stage("unverified","Configure health.port and health.path to verify the application"),"tunnel":stage(if !public_configured{"notConfigured"}else if tunnel_ready{"ready"}else{"failed"},"Tunnel state does not prove public application readiness"),"publicHttps":stage(if public_configured{"unverified"}else{"notConfigured"},"A URL appearing in metadata does not prove HTTPS readiness"),"authenticated":stage("notConfigured","Select a health.bearer_secret vault reference for an authenticated request")});
    let mut verified_publicly=false;
    let mut application_ready=false;
    if let Some(probe)=probe {
        let token=probe.bearer_secret.as_ref().map(|name|super::secrets::resolve(name)).transpose();
        match token {
            Err(error) => { stages["authenticated"]=stage("failed", &error); stages["localHttp"]=stage("blocked","Application credential is unavailable"); }
            Ok(token) => {
                if running {
                    stages["localHttp"]=status_stage(local_probe(env,probe,token.as_deref(),runtime).await,probe.expected_status);
                    application_ready=stages["localHttp"]["status"]=="ready";
                    stages["process"]=stage(if application_ready{"ready"}else{"unverified"},if application_ready{"Application responded to the configured request"}else{"Runtime running alone does not verify the application process"});
                    if public_configured && tunnel_ready {
                        let matching=public.iter().filter(|p|p["port"]==probe.port).collect::<Vec<_>>();
                        let mut results=Vec::new();
                        for publication in matching { for url in publication["urls"].as_array().into_iter().flatten().filter_map(Value::as_str).filter(|u|u.starts_with("https://")) {
                            let result=status_stage(public_probe(url,probe,token.as_deref()).await,probe.expected_status);
                            results.push(json!({"publicationId":publication["id"],"url":url,"probe":result}));
                        }}
                        verified_publicly=!results.is_empty()&&results.iter().all(|r|r["probe"]["status"]=="ready");
                        let failed_status=results.iter().find(|result|result["probe"]["status"]!="ready").and_then(|result|result["probe"]["httpStatus"].as_u64());
                        stages["publicHttps"]=json!({"status":if verified_publicly{"ready"}else{"failed"},"httpStatus":failed_status,"checks":results});
                    }
                }
                if token.is_some() { stages["authenticated"]=stage(if application_ready&&(!public_configured||verified_publicly){"ready"}else{"failed"},"Verified using the configured credential; credential and response body are omitted"); }
            }
        }
    }
    let ready=running && if probe.is_some() { application_ready&&(!public_configured||verified_publicly) } else { !public_configured };
    let retryable=running&&probe.is_some()&&stages["localHttp"]["status"]!="blocked"&&![401,403].iter().any(|status|stages["localHttp"]["httpStatus"]==*status||stages["publicHttps"]["httpStatus"]==*status);
    Ok(json!({"environmentId":environment_id,"ready":ready,"retryable":retryable,"level":if verified_publicly{"publicApplication"}else if application_ready{"localApplication"}else{"runtime"},"applicationVerified":application_ready,"verifiedPublicly":verified_publicly,"checkedAt":chrono::Utc::now().to_rfc3339(),"stages":stages,"recoveryAction":if ready{None}else{Some(if !running{"Start or recover this environment"}else if probe.is_none(){"Configure a health check before treating this deployment as publicly ready"}else{"Inspect the failed readiness stage, application logs, and the scoped publication preflight"})}}))
}

#[cfg(test)] mod tests {
    use super::*;
    #[test]fn applied_project_health_replaces_stale_probes_and_preserves_unrelated_edits(){
        let data=tempfile::tempdir().unwrap();let runtime=RuntimeManager::new(Path::new(env!("CARGO_MANIFEST_DIR")),data.path()).unwrap();
        let old:Project=serde_json::from_value(json!({"project":"health-test","environments":{"api":{"health":{"port":3000,"path":"/old"}}}})).unwrap();
        let mut desired=old.clone();desired.environments.get_mut("api").unwrap().health.as_mut().unwrap().path="/new".into();
        let record:Record=serde_json::from_value(json!({"path":"health-test/yougori.yaml","project":"health-test","ids":{"api":"api-id"},"desired":old})).unwrap();
        let mut probes=BTreeMap::from([("api-id".into(),record.desired.as_ref().unwrap().environments["api"].health.clone().unwrap()),("unrelated-id".into(),desired.environments["api"].health.clone().unwrap())]);
        persist_probes(&runtime,&probes).unwrap();reconcile_project_probes(&desired,&record,&runtime).unwrap();
        assert_eq!(saved_probes(&runtime).unwrap()["api-id"].path,"/new");assert!(saved_probes(&runtime).unwrap().contains_key("unrelated-id"));
        let mut removed=record.desired.clone().unwrap();removed.environments.get_mut("api").unwrap().health=None;
        // Independently edited UI configuration is not erased by removing the old manifest probe.
        reconcile_project_probes(&removed,&record,&runtime).unwrap();assert_eq!(saved_probes(&runtime).unwrap()["api-id"].path,"/new");
        probes.insert("api-id".into(),record.desired.as_ref().unwrap().environments["api"].health.clone().unwrap());persist_probes(&runtime,&probes).unwrap();
        reconcile_project_probes(&removed,&record,&runtime).unwrap();assert!(!saved_probes(&runtime).unwrap().contains_key("api-id"));assert!(saved_probes(&runtime).unwrap().contains_key("unrelated-id"));
    }
    #[test] fn status_parse_and_readiness_do_not_treat_cloudflare_530_as_success(){assert_eq!(http_status("HTTP/1.1 530 Origin error\r\n").unwrap(),530);assert_eq!(status_stage(Ok(530),200)["status"],"failed");assert!(http_status("ready").is_err());}
    #[test] fn status_diagnostics_omit_response_content(){let result=status_stage(Ok(401),200);assert_eq!(result["httpStatus"],401);assert!(result.get("body").is_none());}
    #[tokio::test]async fn running_runtime_does_not_verify_an_application_or_a_public_url(){
        let data=tempfile::tempdir().unwrap();let runtime=RuntimeManager::new(Path::new(env!("CARGO_MANIFEST_DIR")),data.path()).unwrap();let store=PlatformStore::load(data.path().join("state.json")).unwrap();let manager=WorkspaceManager::new(data.path());
        let environment:crate::models::Environment=serde_json::from_value(json!({"id":"env-readiness","name":"readiness","kind":"fullVm","status":"running","runtime":"builtin:alpine","provider":"qemu","runtimeId":"env-readiness","createdAt":"2026-01-01T00:00:00Z","description":"disposable readiness metadata fixture","cpuUsage":0,"memoryUsageGb":0,"storageDeltaGb":0,"networkRxMbps":0,"resourcePolicy":{"cpu":{"min":1,"preferred":1,"max":1,"current":1},"memoryGb":{"min":1,"preferred":1,"max":1,"current":1},"priority":"normal","dynamic":false}})).unwrap();
        store.mutate(|state|{state.environments.push(environment);Ok(())}).unwrap();
        let runtime_only=environment_readiness("env-readiness",None,&store,&runtime,&manager).await.unwrap();assert_eq!(runtime_only["ready"],true);assert_eq!(runtime_only["applicationVerified"],false);assert_eq!(runtime_only["stages"]["process"]["status"],"unverified");
        store.mutate(|state|{state.saved_environment_services.push(crate::models::SavedEnvironmentService{id:"pub-saved".into(),environment_id:"env-readiness".into(),port:3000,kind:PublicationKind::Cloudflare,host_port:3200,domain:None,cloudflare_hostname:Some("api.example.test".into()),remembered_account:true});Ok(())}).unwrap();
        let public=environment_readiness("env-readiness",None,&store,&runtime,&manager).await.unwrap();assert_eq!(public["ready"],false);assert_eq!(public["verifiedPublicly"],false);assert_eq!(public["stages"]["publicHttps"]["status"],"unverified");assert_eq!(public["retryable"],false);
    }
    async fn https_fixture(response:&'static str)->(String,reqwest::Client,tokio::task::JoinHandle<String>){
        use tokio_rustls::{rustls::{self,pki_types::PrivatePkcs8KeyDer},TlsAcceptor};
        let key=rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let certificate=reqwest::Certificate::from_der(key.cert.der()).unwrap();
        let config=rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider())).with_safe_default_protocol_versions().unwrap().with_no_client_auth().with_single_cert(vec![key.cert.der().clone()],PrivatePkcs8KeyDer::from(key.signing_key.serialize_der()).into()).unwrap();
        let acceptor=TlsAcceptor::from(std::sync::Arc::new(config));
        let listener=tokio::net::TcpListener::bind(("127.0.0.1",0)).await.unwrap();
        let url=format!("https://127.0.0.1:{}",listener.local_addr().unwrap().port());
        let task=tokio::spawn(async move{
            let (tcp,_)=listener.accept().await.unwrap();
            let mut stream=acceptor.accept(tcp).await.unwrap();
            let mut request=Vec::new();
            let mut byte=[0u8;1];
            while !request.ends_with(b"\r\n\r\n") {stream.read_exact(&mut byte).await.unwrap();request.push(byte[0]);assert!(request.len()<8192);}
            let headers=String::from_utf8(request).unwrap();
            let length=headers.lines().find_map(|line|line.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v|v.trim().parse::<usize>().ok())).unwrap_or(0);
            let mut body=vec![0;length];stream.read_exact(&mut body).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();stream.shutdown().await.unwrap();
            format!("{headers}{}",String::from_utf8(body).unwrap())
        });
        let client=reqwest::Client::builder().no_proxy().add_root_certificate(certificate).redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(3)).build().unwrap();
        (url,client,task)
    }
    #[tokio::test]async fn authenticated_public_probe_reports_real_530_without_response_content(){
        let (url,client,task)=https_fixture("HTTP/1.1 530 Origin error\r\nContent-Length: 11\r\nConnection: close\r\n\r\nsecret-body").await;
        let probe:HealthCheck=serde_json::from_value(json!({"port":3000,"path":"/qualify","method":"POST","body":{"sample":"qualification"},"bearer_secret":"test-ref"})).unwrap();
        let result=status_stage(public_probe_with_client(&url,&probe,Some("private-test-credential"),&client).await,200);
        assert_eq!(result["status"],"failed");assert_eq!(result["httpStatus"],530);
        let request=task.await.unwrap();assert!(request.starts_with("POST /qualify HTTP/1.1"));assert!(request.to_ascii_lowercase().contains("authorization: bearer private-test-credential"));assert!(request.contains("qualification"));
        assert!(!result.to_string().contains("private-test-credential"));assert!(!result.to_string().contains("secret-body"));
    }
    #[tokio::test]async fn public_health_never_redirects_a_credential_or_accepts_cross_origin_paths(){
        let (url,client,task)=https_fixture("HTTP/1.1 302 Found\r\nLocation: https://unrelated.invalid/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        let mut probe:HealthCheck=serde_json::from_value(json!({"port":3000,"path":"/health"})).unwrap();
        assert_eq!(public_probe_with_client(&url,&probe,Some("private-test-credential"),&client).await.unwrap(),302);task.await.unwrap();
        probe.path="//unrelated.invalid/health".into();assert!(public_probe_with_client(&url,&probe,Some("private-test-credential"),&client).await.is_err());
        assert!(public_probe_with_client("http://127.0.0.1/",&probe,None,&client).await.is_err());
    }
}
