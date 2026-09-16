use tauri::{plugin::TauriPlugin, Runtime};
use zeroize::Zeroizing;

const SERVICE: &str = "top.inktandwkx.moyu-agent";

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    // Secrets have no JavaScript command surface.
    tauri::plugin::Builder::new("secure-store").build()
}

pub async fn set_secret(name: &str, value: String) -> Result<(), String> {
    let name = name.to_owned();
    let value = Zeroizing::new(value);
    tokio::task::spawn_blocking(move || {
        keyring::Entry::new(SERVICE, &name)
            .and_then(|entry| entry.set_password(&value))
            .map_err(|_| "Cannot write to the operating system credential vault".to_owned())
    })
    .await
    .map_err(|_| "Credential vault task failed".to_owned())?
}

pub async fn get_secret(name: &str) -> Result<Option<String>, String> {
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || {
        match keyring::Entry::new(SERVICE, &name).and_then(|entry| entry.get_password()) {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("Cannot read the operating system credential vault".to_owned()),
        }
    })
    .await
    .map_err(|_| "Credential vault task failed".to_owned())?
}

pub async fn delete_secret(name: &str) -> Result<(), String> {
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || {
        match keyring::Entry::new(SERVICE, &name).and_then(|entry| entry.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("Cannot delete the operating system credential".to_owned()),
        }
    })
    .await
    .map_err(|_| "Credential vault task failed".to_owned())?
}
