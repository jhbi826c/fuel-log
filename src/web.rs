use js_sys::{Array, Date, Function, Object, Promise, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, BlobPropertyBag, File, HtmlAnchorElement, PositionOptions, Url};

use crate::i18n::Lang;

fn window() -> web_sys::Window {
    web_sys::window().expect("no window")
}

pub fn now_ms() -> f64 {
    Date::now()
}

/// Current local time as "YYYY-MM-DDTHH:MM".
pub fn now_local() -> String {
    local_string(&Date::new_0())
}

/// Local time of a Unix timestamp (ms) as "YYYY-MM-DDTHH:MM".
pub fn local_from_ms(ms: f64) -> String {
    local_string(&Date::new(&JsValue::from_f64(ms)))
}

fn local_string(d: &Date) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}",
        d.get_full_year(),
        d.get_month() + 1,
        d.get_date(),
        d.get_hours(),
        d.get_minutes()
    )
}

pub fn browser_lang() -> Lang {
    let l = window().navigator().language().unwrap_or_default().to_lowercase();
    if l.starts_with("zh") {
        Lang::Zh
    } else if l.starts_with("de") {
        Lang::De
    } else {
        Lang::En
    }
}

pub fn hash() -> String {
    window().location().hash().unwrap_or_default()
}

pub fn navigate(hash: &str) {
    let _ = window().location().set_hash(hash);
}

pub fn scroll_top() {
    window().scroll_to_with_x_and_y(0.0, 0.0);
}

pub fn set_document_lang(code: &str, title: &str) {
    if let Some(doc) = window().document() {
        if let Some(el) = doc.document_element() {
            let _ = el.set_attribute("lang", code);
        }
        doc.set_title(title);
    }
}

pub fn confirm(msg: &str) -> bool {
    window().confirm_with_message(msg).unwrap_or(false)
}

pub fn prompt(msg: &str, default: &str) -> Option<String> {
    window().prompt_with_message_and_default(msg, default).ok().flatten()
}

/// Saves `content` as a file download.
pub fn download(name: &str, content: &str, mime: &str) {
    let parts = Array::of1(&JsValue::from_str(content));
    let bag = BlobPropertyBag::new();
    bag.set_type(mime);
    let Ok(blob) = Blob::new_with_str_sequence_and_options(&parts, &bag) else { return };
    let Ok(url) = Url::create_object_url_with_blob(&blob) else { return };
    let doc = window().document().expect("no document");
    if let Ok(a) = doc.create_element("a").map(|e| e.unchecked_into::<HtmlAnchorElement>()) {
        a.set_href(&url);
        a.set_download(name);
        if let Some(body) = doc.body() {
            let _ = body.append_child(&a);
            a.click();
            a.remove();
        }
    }
    let revoke = Closure::once_into_js(move || {
        let _ = Url::revoke_object_url(&url);
    });
    let _ = window().set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 10_000);
}

pub async fn read_file(file: File) -> Result<String, String> {
    JsFuture::from(file.text())
        .await
        .map_err(|e| format!("{e:?}"))?
        .as_string()
        .ok_or_else(|| "not text".into())
}

fn get_f64(obj: &JsValue, key: &str) -> Option<f64> {
    Reflect::get(obj, &JsValue::from_str(key)).ok()?.as_f64()
}

/// Current position as (latitude, longitude).
pub async fn geolocate() -> Result<(f64, f64), String> {
    let geo = window().navigator().geolocation().map_err(|_| "unavailable".to_string())?;
    let promise = Promise::new(&mut |resolve: Function, reject: Function| {
        let ok = Closure::once_into_js(move |pos: JsValue| {
            let _ = resolve.call1(&JsValue::NULL, &pos);
        });
        let err = Closure::once_into_js(move |e: JsValue| {
            let _ = reject.call1(&JsValue::NULL, &e);
        });
        let opts = Object::new();
        let _ = Reflect::set(&opts, &"enableHighAccuracy".into(), &JsValue::TRUE);
        let _ = Reflect::set(&opts, &"timeout".into(), &JsValue::from(15_000));
        let _ = Reflect::set(&opts, &"maximumAge".into(), &JsValue::from(60_000));
        let _ = geo.get_current_position_with_error_callback_and_options(
            ok.unchecked_ref(),
            Some(err.unchecked_ref()),
            opts.unchecked_ref::<PositionOptions>(),
        );
    });
    let pos = JsFuture::from(promise).await.map_err(|e| {
        Reflect::get(&e, &"message".into())
            .ok()
            .and_then(|m| m.as_string())
            .unwrap_or_else(|| "error".into())
    })?;
    let coords = Reflect::get(&pos, &"coords".into()).map_err(|_| "error".to_string())?;
    match (get_f64(&coords, "latitude"), get_f64(&coords, "longitude")) {
        (Some(lat), Some(lng)) => Ok((lat, lng)),
        _ => Err("error".into()),
    }
}

/// Distance between two coordinates in meters.
pub fn haversine((lat1, lng1): (f64, f64), (lat2, lng2): (f64, f64)) -> f64 {
    let r = 6_371_000.0;
    let d_lat = (lat2 - lat1).to_radians();
    let d_lng = (lng2 - lng1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2) + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lng / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().asin()
}

pub fn new_id() -> String {
    let rand = (js_sys::Math::random() * 2f64.powi(40)) as u64;
    format!("{}{}", radix36(now_ms() as u64), radix36(rand))
}

fn radix36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}
