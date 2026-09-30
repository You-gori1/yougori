//! The vault is a separate broker. Desktop-only approval and secret entry are
//! checked against the foreground Yougori process; remote MCP cannot use them.
#[cfg(windows)]
mod broker;
#[cfg(windows)]
pub mod client;
#[cfg(windows)]
mod native;
#[cfg(windows)]
pub mod platform;
pub mod policy;
pub mod protocol;
#[cfg(windows)]
mod storage;
#[cfg(windows)]
mod task;
pub mod transport;

#[cfg(windows)]
pub fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [arg, flag, owner]
            if flag == "--owner" && matches!(arg.as_str(), "--install" | "--serve") =>
        {
            if &platform::user_sid()? != owner {
                return Err("Personal Vault must be elevated as the same Windows account that opened Yougori. Separate administrator accounts and Windows administrator-protection shadow accounts are not supported in this release. No vault was opened.".into());
            }
            if arg == "--install" {
                platform::install()
            } else {
                broker::run()
            }
        }
        _ => Err("Open Personal Vault in Yougori Desktop.".into()),
    }
}
