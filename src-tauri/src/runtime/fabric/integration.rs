use super::*;
use crate::models::{CommandResult, Priority, ResourcePolicy, ResourceRange, SelectedConnectionFolder};
use std::path::Path;

fn fixture(id: &str, kind: EnvironmentKind, policy: &ResourcePolicy) -> Environment {
    serde_json::from_value(serde_json::json!({"id":id,"name":id,"kind":kind,"status":"running","runtime":"builtin:alpine","description":"test","createdAt":"2026-09-08T00:00:00Z","cpuUsage":0,"memoryUsageGb":0,"storageDeltaGb":0,"networkRxMbps":0,"resourcePolicy":policy})).unwrap()
}
async fn execute(
    runtime: &RuntimeManager,
    environment: &Environment,
    command: &str,
) -> Result<CommandResult, String> {
    if environment.kind == EnvironmentKind::Container {
        runtime
            .execute_container_command(&environment.id, command)
            .await
    } else {
        runtime
            .execute_micro_vm_command(&environment.id, command)
            .await
    }
}
async fn ready(runtime: &RuntimeManager, id: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        match runtime.execute_micro_vm_command(id, "true").await {
            Ok(result) if result.exit_code == 0 => return Ok(()),
            result if Instant::now() >= deadline => {
                return Err(format!("guest not ready: {result:?}"))
            }
            _ => tokio::time::sleep(Duration::from_millis(200)).await,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "boots disposable MicroVMs and an isolated OCI appliance; downloads a small Alpine image"]
async fn fabric_real_microvms_and_container_exchange_data_without_internet() -> Result<(), String> {
    let data = tempfile::tempdir().unwrap();
    let runtime = RuntimeManager::new(Path::new(env!("CARGO_MANIFEST_DIR")), data.path())?;
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
    let a = fixture("fabric-micro-a", EnvironmentKind::MicroVm, &policy);
    let b = fixture("fabric-micro-b", EnvironmentKind::MicroVm, &policy);
    let c = fixture("fabric-container", EnvironmentKind::Container, &policy);
    let result=async {
        for env in [&a,&b] {
            let vm=runtime.provision_micro_vm(&env.id,"builtin:alpine").await?;
            runtime.start_micro_vm(&env.id,&vm.disk_path,&vm.source_path,&policy).await?;
            ready(&runtime,&env.id).await?;
            let addresses=execute(&runtime,env,"ip -4 addr; ip route").await?;
            if !addresses.stdout.contains(&ip_text(&env.id)) { return Err(format!("missing private IP: {}",addresses.stdout)); }
            let server=execute(&runtime,env,r#"printf '%s\n' '#!/bin/sh' 'while IFS= read -r header; do [ "$header" = "$(printf "\r")" ] && break; done' 'printf "HTTP/1.0 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\nprivate-data"' 'cat >/dev/null' > /tmp/fabric-server; chmod 700 /tmp/fabric-server; sh -c 'while true; do nc -l -p 45678 -e /tmp/fabric-server; done' </dev/null >/tmp/fabric-server.log 2>&1 &"#).await?;
            if server.exit_code!=0 { return Err(server.stderr); }
            execute(&runtime,env,"ip route del default").await?;
        }
        runtime.provision_container(&c.id,"quay.io/libpod/alpine:latest","sleep 2147483647",&policy,false,false).await?;
        runtime.container_action(&c.id,"start",false).await?;
        for (id,source,target) in [("fabric-ab",&a,&b),("fabric-cb",&c,&b),("fabric-ca",&c,&a)] {
            runtime.apply_environment_connection(id,source,target,&ConnectionDirection::OneWay,&[PermissionKind::Ports],&[45678]).await?;
            let output=execute(&runtime,source,&format!("wget -T 4 -qO- http://{}:45678/value",ip_text(&target.id))).await?;
            if output.exit_code!=0 || output.stdout!="private-data" { return Err(format!("{} -> {} failed: {output:?}",source.id,target.id)); }
            eprintln!("{} -> {}: HTTP content read through private TCP rule",source.id,target.id);
            let denied=execute(&runtime,source,&format!("ping -c 1 -W 1 {}",ip_text(&target.id))).await?;
            if denied.exit_code==0 { return Err("port-only rule allowed ping".into()); }
        }
        let reverse=execute(&runtime,&b,&format!("wget -T 2 -qO- http://{}:45678/value",ip_text(&a.id))).await?;
        if reverse.exit_code==0 { return Err("one-way connection allowed reverse initiation".into()); }
        runtime.apply_environment_connection("fabric-ab",&a,&b,&ConnectionDirection::Bidirectional,&[PermissionKind::Network],&[]).await?;
        let reverse=execute(&runtime,&b,&format!("wget -T 4 -qO- http://{}:45678/value",ip_text(&a.id))).await?;
        if reverse.stdout!="private-data" { return Err(format!("bidirectional request failed: {reverse:?}")); }
        runtime.remove_environment_connection("fabric-ab",&a.id,&b.id,false).await?;
        let revoked=execute(&runtime,&a,&format!("wget -T 2 -qO- http://{}:45678/value",ip_text(&b.id))).await?;
        if revoked.exit_code==0 { return Err("deleted connection still allowed requests".into()); }
        runtime.container_action(&c.id,"stop",false).await?;
        runtime.container_action(&c.id,"start",false).await?;
        runtime.apply_environment_connection("fabric-cb",&c,&b,&ConnectionDirection::OneWay,&[PermissionKind::Ports],&[45678]).await?;
        let restarted=execute(&runtime,&c,&format!("wget -T 4 -qO- http://{}:45678/value",ip_text(&b.id))).await?;
        if restarted.stdout!="private-data" { return Err(format!("container restart lost private connectivity: {restarted:?}")); }
        for (id,source,target) in [("files-ab",&a,&b),("files-ca",&c,&a),("files-bc",&b,&c)] {
            runtime.apply_environment_connection(id,source,target,&ConnectionDirection::Bidirectional,&[PermissionKind::Files],&[]).await?;
            let path=format!("/yougori/shared/{id}/test.txt");
            let write=execute(&runtime,source,&format!("printf source > {path}")).await?;
            let read=execute(&runtime,target,&format!("cat {path}")).await?;
            if write.exit_code!=0 || read.stdout!="source" {return Err(format!("FUSE {} -> {}: write={write:?}, read={read:?}",source.id,target.id));}
            let write=execute(&runtime,target,&format!("printf target > {path}")).await?;
            let read=execute(&runtime,source,&format!("cat {path}")).await?;
            if write.exit_code!=0 || read.stdout!="target" {return Err(format!("reverse FUSE failed: {write:?}, {read:?}"));}
            runtime.remove_environment_connection(id,&source.id,&target.id,false).await?;
            let revoked=execute(&runtime,source,&format!("cat {path}")).await?;
            if revoked.exit_code==0 {return Err("removed mount still readable".into());}
        }
        let prepare=execute(&runtime,&a,"mkdir -p /tmp/yougori-project && printf original > /tmp/yougori-project/note.txt").await?;
        if prepare.exit_code!=0 { return Err(format!("prepare selected folder: {prepare:?}")); }
        runtime.apply_environment_connection_with_folders("selected-ac",&a,&c,&ConnectionDirection::Bidirectional,&[PermissionKind::Data],&[],&[SelectedConnectionFolder { environment_id:a.id.clone(),path:"/tmp/yougori-project".into() }, SelectedConnectionFolder { environment_id:a.id.clone(),path:"/".into() }],false).await?;
        let mounted=execute(&runtime,&c,"cat /yougori/shared/selected-ac-selected-0/note.txt").await?;
        if mounted.stdout!="original" { return Err(format!("selected folder not mounted immediately: {mounted:?}")); }
        let root_mounted=execute(&runtime,&c,"cat /yougori/shared/selected-ac-selected-1/tmp/yougori-project/note.txt").await?;
        if root_mounted.stdout!="original" { return Err(format!("selected root not mounted immediately: {root_mounted:?}")); }
        let changed=execute(&runtime,&c,"printf changed > /yougori/shared/selected-ac-selected-0/note.txt").await?;
        let original=execute(&runtime,&a,"cat /tmp/yougori-project/note.txt").await?;
        if changed.exit_code!=0 || original.stdout!="changed" { return Err(format!("selected folder did not stay live: {changed:?}, {original:?}")); }
        runtime.remove_environment_connection("selected-ac",&a.id,&c.id,false).await?;
        if execute(&runtime,&c,"cat /yougori/shared/selected-ac-selected-0/note.txt").await?.exit_code==0 { return Err("selected folder stayed mounted after disconnect".into()); }
        // Full VMs use the very same browser protocol; exercise both directions
        // with a PC-style endpoint alongside both managed guest kinds.
        let mut vm=a.clone();vm.kind=EnvironmentKind::FullVm;
        for (id,source,target) in [("files-vc",&vm,&c),("files-cv",&c,&vm),("files-vm",&vm,&b),("files-mv",&b,&vm)] {
            runtime.apply_environment_connection(id,source,target,&ConnectionDirection::Bidirectional,&[PermissionKind::Data],&[]).await?;
            let managed=if source.kind==EnvironmentKind::FullVm {target} else {source};
            let command=format!("printf mixed > /yougori/shared/{id}/mixed.txt");
            let write=execute(&runtime,managed,&command).await?;if write.exit_code!=0{return Err(format!("mixed mount: {write:?}"));}
            let command=format!(r#"wget -T 5 -qO- --header='X-Yougori-Files: 1' --post-data='{{"connectionId":"{id}","operation":"read","path":"mixed.txt","length":5}}' http://10.192.0.1:7444/api"#);
            let read=execute(&runtime,&a,&command).await?;if read.exit_code!=0 || !read.stdout.contains("bWl4ZWQ="){return Err(format!("mixed VM browser: {read:?}"));}
            runtime.remove_environment_connection(id,&source.id,&target.id,false).await?;
        }
        eprintln!("Direction, ports, shared mounts in both directions, mixed VM API, revocation and container restart verified.");
        Ok(())
    }.await;
    runtime.shutdown_all().await;
    result
}
