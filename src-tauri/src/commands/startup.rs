use super::*;

#[tauri::command]
pub async fn update_container_startup_command(
    environment_id: String,
    command: String,
    store: State<'_, PlatformStore>,
    runtime: State<'_, RuntimeManager>,
) -> Result<PlatformState, String> {
    update(&environment_id, &command, &store, &runtime).await
}

pub(crate) async fn update(
    environment_id: &str,
    command: &str,
    store: &PlatformStore,
    runtime: &RuntimeManager,
) -> Result<PlatformState, String> {
    let command = command.trim();
    if command.len() > 32 * 1024 || command.contains('\0') {
        return Err("Startup command must be at most 32 KB and cannot contain null characters".into());
    }
    let lock = environment_network_lock(environment_id).await;
    let _environment_serial = lock.lock().await;
    let state = store.snapshot()?;
    let environment = state.environments.iter().find(|env| env.id == environment_id)
        .ok_or("Environment not found")?;
    let _container_serial = environment_container_policy_guard(runtime, environment).await?;
    if environment.kind != EnvironmentKind::Container || !provider(environment).is_container() {
        return Err("Startup commands are available for local containers only".into());
    }
    if environment.status != EnvironmentStatus::Stopped {
        return Err("Stop the container before changing its startup command".into());
    }
    if environment.container_command.as_deref().unwrap_or_default() == command {
        return Ok(state);
    }
    if state.pending_factory_resets.iter().any(|p| p.environment.id == environment_id) {
        return Err("Finish the pending Factory reset before changing the startup command".into());
    }
    runtime.update_container_startup(environment, command).await?;
    let persisted = store.mutate(|state| {
        let env = state.environments.iter_mut().find(|env| env.id == environment_id)
            .ok_or("Environment not found")?;
        env.container_command = (!command.is_empty()).then(|| command.to_owned());
        Ok(())
    });
    match persisted {
        Ok(state) => Ok(state),
        Err(error) => {
            let mut changed = environment.clone();
            changed.container_command = Some(command.to_owned());
            match runtime.update_container_startup(&changed, environment.container_command.as_deref().unwrap_or_default()).await {
                Ok(()) => Err(error),
                Err(rollback) => Err(format!("{error}; restoring the previous startup command failed: {rollback}")),
            }
        }
    }
}

#[cfg(test)]
mod tests;
