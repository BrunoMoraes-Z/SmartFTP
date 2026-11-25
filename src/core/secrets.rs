//! Saved passwords live in the operating system's credential store: Windows Credential
//! Manager (protected with DPAPI under the user's logon), the macOS Keychain or the Linux
//! Secret Service. `config.json` never contains them.
//!
//! One entry per site: service "SmartFTP", account = the site id. A cache of what is
//! already in the store avoids touching it on every config save.

use std::collections::HashMap;
use std::sync::LazyLock;

use parking_lot::Mutex;

/// What the store holds, as far as this process knows: site id -> password (None = no entry).
static KNOWN: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(Default::default);

fn service() -> String {
    // a separate SMARTFTP_HOME (tests, portable setups) gets its own entries
    match std::env::var_os("SMARTFTP_HOME") {
        Some(home) => format!("SmartFTP ({})", home.to_string_lossy()),
        None => "SmartFTP".into(),
    }
}

/// Read a site's password from the store.
pub fn load(site_id: &str) -> anyhow::Result<Option<String>> {
    let value = backend::get(&service(), site_id)?;
    KNOWN.lock().insert(site_id.to_string(), value.clone());
    Ok(value)
}

/// Put (or with `None`, remove) a site's password in the store. No-op when unchanged.
pub fn store(site_id: &str, password: Option<&str>) -> anyhow::Result<()> {
    let password = password.filter(|p| !p.is_empty());
    if let Some(known) = KNOWN.lock().get(site_id)
        && known.as_deref() == password
    {
        return Ok(());
    }
    match password {
        Some(pw) => backend::set(&service(), site_id, pw)?,
        None => backend::delete(&service(), site_id)?,
    }
    KNOWN.lock().insert(site_id.to_string(), password.map(str::to_string));
    Ok(())
}

/// Remove the entries of sites that no longer exist.
pub fn forget_except(site_ids: &[&str]) {
    let stale: Vec<String> =
        KNOWN.lock().iter().filter(|(id, pw)| pw.is_some() && !site_ids.contains(&id.as_str())).map(|(id, _)| id.clone()).collect();
    for id in stale {
        if let Err(e) = store(&id, None) {
            tracing::warn!("removing the saved password of a deleted site: {e:#}");
        }
    }
}

#[cfg(not(test))]
mod backend {
    use keyring::{Entry, Error};

    pub fn get(service: &str, account: &str) -> anyhow::Result<Option<String>> {
        match Entry::new(service, account)?.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn set(service: &str, account: &str, password: &str) -> anyhow::Result<()> {
        Ok(Entry::new(service, account)?.set_password(password)?)
    }

    pub fn delete(service: &str, account: &str) -> anyhow::Result<()> {
        match Entry::new(service, account)?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// Unit tests use an in-memory store instead of the real one.
#[cfg(test)]
pub(crate) mod backend {
    use std::collections::HashMap;
    use std::sync::LazyLock;

    use parking_lot::Mutex;

    pub static STORE: LazyLock<Mutex<HashMap<(String, String), String>>> = LazyLock::new(Default::default);

    pub fn get(service: &str, account: &str) -> anyhow::Result<Option<String>> {
        Ok(STORE.lock().get(&(service.into(), account.into())).cloned())
    }

    pub fn set(service: &str, account: &str, password: &str) -> anyhow::Result<()> {
        STORE.lock().insert((service.into(), account.into()), password.into());
        Ok(())
    }

    pub fn delete(service: &str, account: &str) -> anyhow::Result<()> {
        STORE.lock().remove(&(service.into(), account.into()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_load_and_forget() {
        store("site-a", Some("pw-a")).unwrap();
        store("site-b", Some("pw-b")).unwrap();
        KNOWN.lock().clear();
        assert_eq!(load("site-a").unwrap().as_deref(), Some("pw-a"));
        assert_eq!(load("site-b").unwrap().as_deref(), Some("pw-b"));
        store("site-a", Some("")).unwrap(); // empty = remove
        assert_eq!(load("site-a").unwrap(), None);
        forget_except(&[]);
        assert_eq!(backend::get(&service(), "site-b").unwrap(), None);
    }
}
