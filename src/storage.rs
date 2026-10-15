use serde::Serialize;
use serde::de::DeserializeOwned;

pub const DATA_KEY: &str = "fuellog.data";
pub const SETTINGS_KEY: &str = "fuellog.settings";
pub const SYNC_KEY: &str = "fuellog.sync";

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// Loads a value; unreadable data is kept under a backup key instead of being lost.
pub fn load<T: DeserializeOwned>(key: &str) -> Option<T> {
    let s = storage()?;
    let raw = s.get_item(key).ok()??;
    match serde_json::from_str(&raw) {
        Ok(v) => Some(v),
        Err(_) => {
            let _ = s.set_item(&format!("{key}.corrupt-{}", js_sys::Date::now() as u64), &raw);
            None
        }
    }
}

pub fn save<T: Serialize>(key: &str, value: &T) -> Result<(), String> {
    let json = serde_json::to_string(value).map_err(|e| e.to_string())?;
    storage()
        .ok_or("localStorage unavailable")?
        .set_item(key, &json)
        .map_err(|e| format!("{e:?}"))
}

pub fn remove(key: &str) {
    if let Some(s) = storage() {
        let _ = s.remove_item(key);
    }
}
