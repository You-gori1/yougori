//! A demand-start, elevated task avoids asking for UAC on every Desktop launch.
//! Only the elevated installer creates it. Its executable, arguments, principal,
//! and ACL are checked again before an unelevated Desktop may start it.

use std::{mem::ManuallyDrop, path::Path};
use windows::{
    core::{BSTR, Interface},
    Win32::{
        Foundation::{VARIANT_FALSE, VARIANT_TRUE},
        System::{
            Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED},
            TaskScheduler::{
                IExecAction, ITaskService, TaskScheduler, TASK_ACTION_EXEC,
                TASK_CREATE_OR_UPDATE, TASK_DONT_ADD_PRINCIPAL_ACE,
                TASK_INSTANCES_IGNORE_NEW, TASK_LOGON_INTERACTIVE_TOKEN,
                TASK_RUNLEVEL_HIGHEST,
            },
            Variant::{VariantClear, VARIANT, VT_BSTR},
        },
    },
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, *},
    Storage::FileSystem::{FILE_ALL_ACCESS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE},
};

struct ComGuard;
impl Drop for ComGuard {
    fn drop(&mut self) { unsafe { CoUninitialize() } }
}

struct StringVariant(VARIANT);
impl StringVariant {
    fn new(value: &str) -> Self {
        let mut variant = VARIANT::default();
        unsafe {
            let body = &mut *variant.Anonymous.Anonymous;
            body.vt = VT_BSTR;
            body.Anonymous.bstrVal = ManuallyDrop::new(BSTR::from(value));
        }
        Self(variant)
    }
}
impl Drop for StringVariant {
    fn drop(&mut self) { unsafe { let _ = VariantClear(&mut self.0); } }
}

fn name(sid: &str) -> String { format!("Yougori Protected Vault {sid}") }
fn arguments(sid: &str) -> String { format!("--serve --owner {sid}") }
fn security(sid: &str) -> String {
    format!("O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;GRGX;;;{sid})")
}
fn com_service() -> Result<(ComGuard, ITaskService), String> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().map_err(|error| error.to_string())?;
        let guard = ComGuard;
        let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| error.to_string())?;
        let empty = VARIANT::default();
        service.Connect(&empty, &empty, &empty, &empty).map_err(|error| error.to_string())?;
        Ok((guard, service))
    }
}

pub(crate) fn install(executable: &Path, sid: &str) -> Result<(), String> {
    let (_com, service) = com_service()?;
    unsafe {
        let folder = service.GetFolder(&BSTR::from("\\")).map_err(|error| error.to_string())?;
        let definition = service.NewTask(0).map_err(|error| error.to_string())?;
        let principal = definition.Principal().map_err(|error| error.to_string())?;
        principal.SetUserId(&BSTR::from(sid)).map_err(|error| error.to_string())?;
        principal.SetLogonType(TASK_LOGON_INTERACTIVE_TOKEN).map_err(|error| error.to_string())?;
        principal.SetRunLevel(TASK_RUNLEVEL_HIGHEST).map_err(|error| error.to_string())?;
        let action = definition.Actions().map_err(|error| error.to_string())?
            .Create(TASK_ACTION_EXEC).map_err(|error| error.to_string())?
            .cast::<IExecAction>().map_err(|error| error.to_string())?;
        action.SetPath(&BSTR::from(executable.to_string_lossy().as_ref())).map_err(|error| error.to_string())?;
        action.SetArguments(&BSTR::from(arguments(sid))).map_err(|error| error.to_string())?;
        let settings = definition.Settings().map_err(|error| error.to_string())?;
        settings.SetAllowDemandStart(VARIANT_TRUE).map_err(|error| error.to_string())?;
        settings.SetDisallowStartIfOnBatteries(VARIANT_FALSE).map_err(|error| error.to_string())?;
        settings.SetStopIfGoingOnBatteries(VARIANT_FALSE).map_err(|error| error.to_string())?;
        settings.SetExecutionTimeLimit(&BSTR::from("PT0S")).map_err(|error| error.to_string())?;
        settings.SetMultipleInstances(TASK_INSTANCES_IGNORE_NEW).map_err(|error| error.to_string())?;
        let user = StringVariant::new(sid);
        let acl = StringVariant::new(&security(sid));
        let empty = VARIANT::default();
        let flags = TASK_CREATE_OR_UPDATE.0 | TASK_DONT_ADD_PRINCIPAL_ACE.0;
        let task = folder.RegisterTaskDefinition(
            &BSTR::from(name(sid)), &definition, flags, &user.0, &empty,
            TASK_LOGON_INTERACTIVE_TOKEN, &acl.0,
        ).map_err(|error| format!("Register protected vault startup: {error}"))?;
        // Fail installation if Windows did not keep the restricted task ACL.
        verify(&task, executable, sid)
    }
}

pub(crate) fn run(executable: &Path, sid: &str) -> Result<(), String> {
    let (_com, service) = com_service()?;
    unsafe {
        let folder = service.GetFolder(&BSTR::from("\\")).map_err(|error| error.to_string())?;
        let task = folder.GetTask(&BSTR::from(name(sid))).map_err(|error| error.to_string())?;
        verify(&task, executable, sid)?;
        task.Run(&VARIANT::default()).map_err(|error| format!("Start protected vault task: {error}"))?;
        Ok(())
    }
}

fn verify(task: &windows::Win32::System::TaskScheduler::IRegisteredTask, executable: &Path, sid: &str) -> Result<(), String> {
    unsafe {
        if task.Enabled().map_err(|error| error.to_string())? != VARIANT_TRUE {
            return Err("Protected vault startup task is disabled".into());
        }
        let definition = task.Definition().map_err(|error| error.to_string())?;
        let principal = definition.Principal().map_err(|error| error.to_string())?;
        let mut user = BSTR::new();
        let mut logon = TASK_LOGON_INTERACTIVE_TOKEN;
        let mut level = TASK_RUNLEVEL_HIGHEST;
        principal.UserId(&mut user).map_err(|error| error.to_string())?;
        principal.LogonType(&mut logon).map_err(|error| error.to_string())?;
        principal.RunLevel(&mut level).map_err(|error| error.to_string())?;
        if account_sid(&user.to_string())? != sid || logon != TASK_LOGON_INTERACTIVE_TOKEN || level != TASK_RUNLEVEL_HIGHEST {
            return Err("Protected vault startup task has an unexpected identity".into());
        }
        let actions = definition.Actions().map_err(|error| error.to_string())?;
        let triggers = definition.Triggers().map_err(|error| error.to_string())?;
        let mut trigger_count = 0;
        triggers.Count(&mut trigger_count).map_err(|error| error.to_string())?;
        if trigger_count != 0 { return Err("Protected vault startup task has unexpected triggers".into()); }
        let mut count = 0;
        actions.Count(&mut count).map_err(|error| error.to_string())?;
        if count != 1 { return Err("Protected vault startup task has unexpected actions".into()); }
        let action = actions.get_Item(1).map_err(|error| error.to_string())?
            .cast::<IExecAction>().map_err(|error| error.to_string())?;
        let mut path = BSTR::new();
        let mut args = BSTR::new();
        let mut directory = BSTR::new();
        action.Path(&mut path).map_err(|error| error.to_string())?;
        action.Arguments(&mut args).map_err(|error| error.to_string())?;
        action.WorkingDirectory(&mut directory).map_err(|error| error.to_string())?;
        if !path.to_string().eq_ignore_ascii_case(&executable.to_string_lossy())
            || args.to_string() != arguments(sid) || !directory.is_empty()
        {
            return Err("Protected vault startup task points to an unexpected command".into());
        }
        let actual = task.GetSecurityDescriptor((OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION) as i32)
            .map_err(|error| error.to_string())?;
        if acl_identity(&actual.to_string())? != acl_identity(&security(sid))? {
            return Err("Protected vault startup task has unsafe permissions".into());
        }
        Ok(())
    }
}

fn account_sid(name: &str) -> Result<String, String> {
    if name.starts_with("S-1-") { return Ok(name.to_owned()); }
    unsafe {
        let account = crate::platform::wide(name);
        let mut sid_size = 0;
        let mut domain_size = 0;
        let mut usage = std::mem::zeroed();
        LookupAccountNameW(
            std::ptr::null(), account.as_ptr(), std::ptr::null_mut(), &mut sid_size,
            std::ptr::null_mut(), &mut domain_size, &mut usage,
        );
        if sid_size == 0 { return Err("Cannot resolve vault task account".into()); }
        let mut sid = vec![0_u8; sid_size as usize];
        let mut domain = vec![0_u16; domain_size as usize];
        if LookupAccountNameW(
            std::ptr::null(), account.as_ptr(), sid.as_mut_ptr().cast(), &mut sid_size,
            domain.as_mut_ptr(), &mut domain_size, &mut usage,
        ) == 0 { return Err("Cannot verify vault task account".into()); }
        crate::platform::sid_text(sid.as_mut_ptr().cast())
    }
}

fn acl_identity(sddl: &str) -> Result<(String, bool, Vec<(u8, u32, String)>), String> {
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            crate::platform::wide(sddl).as_ptr(), 1, &mut descriptor, std::ptr::null_mut(),
        ) == 0 { return Err("Invalid vault task permissions".into()); }
        struct Local(*mut std::ffi::c_void);
        impl Drop for Local { fn drop(&mut self) { unsafe { LocalFree(self.0); } } }
        let _guard = Local(descriptor);
        let mut owner = std::ptr::null_mut();
        let mut owner_defaulted = 0;
        if GetSecurityDescriptorOwner(descriptor, &mut owner, &mut owner_defaulted) == 0 {
            return Err("Cannot inspect vault task owner".into());
        }
        let owner = crate::platform::sid_text(owner)?;
        let mut control = 0;
        let mut revision = 0;
        if GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) == 0 {
            return Err("Cannot inspect vault task inheritance".into());
        }
        let protected = control & SE_DACL_PROTECTED != 0;
        let mut present = 0;
        let mut dacl = std::ptr::null_mut();
        let mut defaulted = 0;
        if GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) == 0 || present == 0 || dacl.is_null() {
            return Err("Vault task needs an explicit permission list".into());
        }
        let mut entries = Vec::new();
        for index in 0..(*dacl).AceCount {
            let mut ace = std::ptr::null_mut();
            if GetAce(dacl, index as u32, &mut ace) == 0 { return Err("Cannot inspect vault task rule".into()); }
            let header = &*(ace as *const ACE_HEADER);
            if header.AceType != 0 { return Err("Unexpected vault task rule".into()); }
            let allowed = &*(ace as *const ACCESS_ALLOWED_ACE);
            let sid = crate::platform::sid_text((&allowed.SidStart as *const u32).cast_mut().cast())?;
            let mut mask = allowed.Mask;
            let mapping = GENERIC_MAPPING {
                GenericRead: FILE_GENERIC_READ,
                GenericWrite: FILE_GENERIC_WRITE,
                GenericExecute: FILE_GENERIC_EXECUTE,
                GenericAll: FILE_ALL_ACCESS,
            };
            MapGenericMask(&mut mask, &mapping);
            entries.push((header.AceType, mask, sid));
        }
        entries.sort();
        Ok((owner, protected, entries))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_task_grants_the_user_only_read_and_run() {
        let sid = "S-1-5-21-111-222-333-1001";
        let (owner, protected, rules) = acl_identity(&security(sid)).unwrap();
        assert_eq!(owner, "S-1-5-32-544");
        assert!(protected);
        assert_eq!(rules.len(), 3);
        assert_ne!(rules[0].2, "S-1-1-0");
        assert!(rules.iter().any(|rule| rule.2 == sid));
        assert_ne!(acl_identity(&security(sid)).unwrap(), acl_identity(&format!("O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;{sid})")).unwrap());
    }

    #[test]
    #[ignore = "requires the installed Windows vault task"]
    fn installed_task_can_start_without_elevation() {
        run(&crate::platform::installed_binary().unwrap(), &crate::platform::user_sid().unwrap()).unwrap();
    }
}
