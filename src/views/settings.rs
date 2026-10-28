use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::app::{Ui, run_sync};
use crate::i18n::{Lang, fill};
use crate::model::{AppData, FUEL_TYPES, MapProvider, Theme};
use crate::places::tankerkoenig::{self as tk, TkError};
use crate::store::Store;
use crate::sync::DEFAULT_REPO;
use crate::{currency, format, web};

const TOKEN_URL: &str = "https://github.com/settings/tokens/new?scopes=repo&description=Fuel%20Log";
const FINE_GRAINED_URL: &str = "https://github.com/settings/personal-access-tokens/new";
const MAX_SYMBOL_LEN: usize = 5;

/// Whether a search query can be used as-is as a custom currency symbol.
fn custom_ok(q: &str) -> bool {
    let n = q.trim().chars().count();
    n > 0 && n <= MAX_SYMBOL_LEN
}

/// The symbol, plus the currency name when the symbol is unambiguous.
fn currency_label(symbol: &str, lang: Lang) -> String {
    let mut found = currency::CURRENCIES.iter().filter(|c| c.symbol == symbol);
    match (found.next(), found.next()) {
        (Some(c), None) => format!("{symbol} · {}", c.name(lang)),
        _ => symbol.to_string(),
    }
}

fn picked_file(ev: &leptos::ev::Event) -> Option<web_sys::File> {
    let input = ev.target()?.dyn_into::<web_sys::HtmlInputElement>().ok()?;
    let file = input.files()?.get(0);
    input.set_value("");
    file
}

fn today() -> String {
    web::now_local().get(..10).unwrap_or_default().to_string()
}

#[component]
pub fn SettingsView() -> impl IntoView {
    let store = expect_context::<Store>();
    let ui = expect_context::<Ui>();
    let token_input = RwSignal::new(String::new());
    let repo_input = RwSignal::new(store.sync.with_untracked(|c| c.repo.clone()));
    let tk_input = RwSignal::new(String::new());
    let tk_checking = RwSignal::new(false);
    let cur_open = RwSignal::new(false);
    let cur_query = RwSignal::new(String::new());

    let export_json = move |_| {
        let mut data = store.snapshot();
        data.records.retain(|r| !r.deleted);
        data.vehicles.retain(|v| !v.deleted);
        let json = serde_json::to_string_pretty(&data).unwrap_or_default();
        web::download(&format!("fuel-log-{}.json", today()), &json, "application/json");
    };

    let import_json = move |ev: leptos::ev::Event| {
        let Some(file) = picked_file(&ev) else { return };
        spawn_local(async move {
            let t = store.t();
            let parsed = web::read_file(file)
                .await
                .and_then(|text| serde_json::from_str::<AppData>(&text).map_err(|e| e.to_string()));
            match parsed {
                Ok(data) => {
                    let n = store.import(&data);
                    ui.toast(fill(t.imported_n, &[("n", &n)]));
                }
                Err(e) => ui.error(format!("{} ({e})", t.import_failed)),
            }
        });
    };

    let remove_demo = move |_| {
        let ids: Vec<String> = store.data.with_untracked(|d| {
            d.records.iter().filter(|r| r.demo && !r.deleted).map(|r| r.id.clone()).collect()
        });
        store.delete_records(&ids);
        ui.toast_undo(store.t().demo_removed, ids);
    };

    let delete_all = move |_| {
        let t = store.t();
        if !web::confirm(t.confirm_delete_all) {
            return;
        }
        let ids: Vec<String> = store.data.with_untracked(|d| {
            d.records.iter().filter(|r| !r.deleted).map(|r| r.id.clone()).collect()
        });
        store.delete_records(&ids);
        ui.toast_undo(t.all_deleted, ids);
    };

    let connect = move |_| {
        let token = token_input.get_untracked().trim().to_string();
        if token.is_empty() {
            return;
        }
        let repo = repo_input.get_untracked().trim().trim_matches('/').to_string();
        store.update_sync(|c| {
            c.token = token;
            c.repo = repo;
            c.auto = true;
        });
        token_input.set(String::new());
        run_sync(store, ui, false);
    };

    let disconnect = move |_| {
        if web::confirm(store.t().confirm_disconnect) {
            store.clear_sync();
        }
    };

    let add_vehicle = move |_| {
        let t = store.t();
        if let Some(name) = web::prompt(t.vehicle_name_prompt, "").map(|s| s.trim().to_string())
            && !name.is_empty()
        {
            let id = store.add_vehicle(&name);
            store.set_active_vehicle(&id);
        }
    };

    let close_currency = move || {
        cur_open.set(false);
        cur_query.set(String::new());
        if let Some(el) = document().active_element().and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
            let _ = el.blur();
        }
    };

    let pick_currency = move |symbol: String| {
        let symbol = symbol.trim().to_string();
        store.update_settings(|s| s.currency = symbol);
        close_currency();
    };

    let currency_list = move || {
        if !cur_open.get() {
            return None;
        }
        let t = store.t();
        let l = store.lang();
        let current = store.currency();
        let q = cur_query.get();
        let hits = currency::search(&q);
        let custom = (custom_ok(&q) && !hits.iter().any(|c| c.symbol == q.trim())).then(|| {
            let v = q.trim().to_string();
            view! {
                <button type="button" class="pick" on:click=move |_| pick_currency(v.clone())>
                    <span class="pick-main">{fill(t.use_custom_currency, &[("v", &q.trim())])}</span>
                </button>
            }
        });
        Some(view! {
            // Keep focus in the search box so a tap on an option isn't swallowed by blur.
            <div class="picker currency-list" on:mousedown=|ev| ev.prevent_default()>
                {hits
                    .into_iter()
                    .map(|c| {
                        view! {
                            <button
                                type="button"
                                class="pick"
                                class:active=c.symbol == current
                                on:click=move |_| pick_currency(c.symbol.to_string())
                            >
                                <span class="pick-main">
                                    <b>{c.name(l)}</b>
                                    <small class="muted">{c.code}</small>
                                </span>
                                <span class="pick-side">
                                    <b>{c.symbol}</b>
                                </span>
                            </button>
                        }
                    })
                    .collect_view()}
                {custom}
            </div>
        })
    };

    let general = move || {
        let t = store.t();
        let l = store.lang();
        let settings = store.settings.get();
        let current_label = currency_label(&settings.currency, l);
        view! {
            <section class="card">
                <h2>{t.settings_general}</h2>
                <label class="field">
                    <span>{t.language}</span>
                    <select on:change=move |ev| {
                        let lang = Lang::from_code(&event_target_value(&ev));
                        store.update_settings(|s| s.lang = lang);
                    }>
                        {Lang::ALL
                            .into_iter()
                            .map(|x| view! { <option value=x.code() selected=x == l>{x.native_name()}</option> })
                            .collect_view()}
                    </select>
                </label>
                <div class="field">
                    <span>{t.currency}</span>
                    <input
                        type="search"
                        autocomplete="off"
                        spellcheck="false"
                        placeholder=t.search_currency
                        prop:value=move || if cur_open.get() { cur_query.get() } else { current_label.clone() }
                        on:focus=move |_| {
                            cur_query.set(String::new());
                            cur_open.set(true);
                        }
                        on:blur=move |_| cur_open.set(false)
                        on:input=move |ev| {
                            cur_query.set(event_target_value(&ev));
                            cur_open.set(true);
                        }
                        on:keydown=move |ev| match ev.key().as_str() {
                            "Enter" => {
                                ev.prevent_default();
                                let q = cur_query.get_untracked();
                                match currency::search(&q).first() {
                                    Some(c) => pick_currency(c.symbol.to_string()),
                                    None if custom_ok(&q) => pick_currency(q),
                                    None => {}
                                }
                            }
                            "Escape" => close_currency(),
                            _ => {}
                        }
                    />
                    {currency_list}
                </div>
                <label class="field">
                    <span>{t.map_provider}</span>
                    <select on:change=move |ev| {
                        let p = MapProvider::from_key(&event_target_value(&ev));
                        store.update_settings(|s| s.map_provider = p);
                    }>
                        {MapProvider::ALL
                            .into_iter()
                            .map(|p| {
                                view! {
                                    <option value=p.key() selected=p == settings.map_provider>
                                        {p.name()}
                                    </option>
                                }
                            })
                            .collect_view()}
                    </select>
                </label>
                <label class="field">
                    <span>{t.theme}</span>
                    <select on:change=move |ev| {
                        let th = Theme::from_key(&event_target_value(&ev));
                        store.update_settings(|s| s.theme = th);
                    }>
                        {Theme::ALL
                            .into_iter()
                            .map(|th| {
                                let name = match th {
                                    Theme::Auto => t.theme_auto,
                                    Theme::Light => t.theme_light,
                                    Theme::Dark => t.theme_dark,
                                };
                                view! {
                                    <option value=th.key() selected=th == settings.theme>
                                        {name}
                                    </option>
                                }
                            })
                            .collect_view()}
                    </select>
                </label>
            </section>
        }
    };

    let vehicles = move || {
        let t = store.t();
        let vs = store.vehicles();
        let many = vs.len() > 1;
        let counts = store.data.with(|d| {
            vs.iter()
                .map(|v| d.records.iter().filter(|r| !r.deleted && r.vehicle_id == v.id).count())
                .collect::<Vec<_>>()
        });
        view! {
            <section class="card">
                <h2>{t.vehicles}</h2>
                <ul class="list">
                    {vs
                        .into_iter()
                        .zip(counts)
                        .map(|(v, n)| {
                            let (id, id2, name) = (v.id.clone(), v.id.clone(), v.name.clone());
                            let fuel_chips = FUEL_TYPES
                                .iter()
                                .map(|(fuel, _)| {
                                    let (vid, fuel) = (v.id.clone(), *fuel);
                                    let on = v.fuels.iter().any(|f| f == fuel);
                                    view! {
                                        <button
                                            type="button"
                                            class=if on { "chip active" } else { "chip" }
                                            on:click=move |_| store.toggle_vehicle_fuel(&vid, fuel)
                                        >
                                            {fuel}
                                        </button>
                                    }
                                })
                                .collect_view();
                            view! {
                                <li class="vehicle">
                                <div class="vehicle-head">
                                    <span>
                                        {v.name.clone()}
                                        <small class="muted">" · "{fill(t.n_records, &[("n", &n)])}</small>
                                    </span>
                                    <span class="row-actions">
                                        <button
                                            class="btn small"
                                            on:click=move |_| {
                                                if let Some(new) = web::prompt(store.t().vehicle_name_prompt, &name)
                                                    .map(|s| s.trim().to_string())
                                                    && !new.is_empty()
                                                {
                                                    store.rename_vehicle(&id, &new);
                                                }
                                            }
                                        >
                                            {t.rename}
                                        </button>
                                        {many.then(|| {
                                            let id = id2.clone();
                                            view! {
                                                <button
                                                    class="btn small danger"
                                                    on:click=move |_| {
                                                        let t = store.t();
                                                        if web::confirm(&fill(t.confirm_delete_vehicle, &[("n", &n)])) {
                                                            store.delete_vehicle(&id);
                                                        }
                                                    }
                                                >
                                                    {t.delete}
                                                </button>
                                            }
                                        })}
                                    </span>
                                </div>
                                <div class="chips wrap">{fuel_chips}</div>
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
                <p class="hint">{t.vehicle_fuels_hint}</p>
                <button class="btn" on:click=add_vehicle>
                    {t.add_vehicle}
                </button>
            </section>
        }
    };

    let save_tk_key = move || {
        let key = tk_input.get_untracked().trim().to_string();
        if !tk::is_valid_key(&key) {
            ui.error(store.t().tk_key_bad_format);
            return;
        }
        tk_checking.set(true);
        spawn_local(async move {
            let t = store.t();
            match tk::check_key(&key).await {
                Err(TkError::InvalidKey) => ui.error(t.tk_key_rejected),
                checked => match store.set_tk_key(&key) {
                    Ok(()) => {
                        tk_input.set(String::new());
                        ui.toast(if checked.is_ok() { t.tk_key_ok } else { t.tk_key_unchecked });
                    }
                    Err(e) => ui.error(format!("{} ({e})", t.save_failed)),
                },
            }
            tk_checking.set(false);
        });
    };

    let prices = move || {
        let t = store.t();
        let key = store.tk_key.get();
        let body = if key.is_empty() {
            view! {
                <p class="muted">
                    {t.tk_intro}" "
                    <a href=tk::SIGNUP_URL target="_blank" rel="noopener noreferrer">
                        {t.tk_get_key}" ↗"
                    </a>
                </p>
                <label class="field">
                    <span>"API key"</span>
                    <input
                        type="password"
                        autocomplete="off"
                        spellcheck="false"
                        placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
                        prop:value=move || tk_input.get()
                        on:input=move |ev| tk_input.set(event_target_value(&ev))
                        on:keydown=move |ev| {
                            if ev.key() == "Enter" {
                                ev.prevent_default();
                                save_tk_key();
                            }
                        }
                    />
                </label>
                <button class="btn primary" disabled=move || tk_checking.get() on:click=move |_| save_tk_key()>
                    {move || if tk_checking.get() { store.t().tk_checking } else { store.t().save }}
                </button>
            }
            .into_any()
        } else {
            view! {
                <p class="muted">{t.tk_intro}</p>
                <p>
                    <span class="badge good">{fill(t.tk_key_saved, &[("key", &tk::key_hint(&key))])}</span>
                </p>
                <div class="btn-row">
                    <a class="btn" href="#/prices">
                        {t.nav_prices}
                    </a>
                    <button
                        class="btn danger"
                        on:click=move |_| {
                            if web::confirm(store.t().tk_confirm_remove) {
                                store.clear_tk_key();
                            }
                        }
                    >
                        {t.remove}
                    </button>
                </div>
            }
            .into_any()
        };
        view! {
            <section class="card">
                <h2>{t.tk_title}</h2>
                {body}
                <p class="hint">{t.tk_key_privacy}</p>
            </section>
        }
    };

    let sync = move || {
        let t = store.t();
        let l = store.lang();
        let conf = store.sync.get();
        let body = if conf.token.is_empty() {
            view! {
                <ol class="steps">
                    <li>
                        {t.sync_step1}" "
                        <a href=TOKEN_URL target="_blank" rel="noopener">
                            {t.sync_create_token}
                        </a>
                    </li>
                    <li>{t.sync_step2}</li>
                    <li>{t.sync_step3}</li>
                </ol>
                <label class="field">
                    <span>"GitHub token"</span>
                    <input
                        type="password"
                        autocomplete="off"
                        spellcheck="false"
                        placeholder="ghp_… / github_pat_…"
                        prop:value=move || token_input.get()
                        on:input=move |ev| token_input.set(event_target_value(&ev))
                    />
                </label>
                <label class="field">
                    <span>{t.sync_repo}</span>
                    <input
                        type="text"
                        autocomplete="off"
                        autocapitalize="off"
                        spellcheck="false"
                        placeholder=DEFAULT_REPO
                        prop:value=move || repo_input.get()
                        on:input=move |ev| repo_input.set(event_target_value(&ev))
                    />
                </label>
                <p class="hint">
                    {t.sync_repo_hint}" "
                    <a href=FINE_GRAINED_URL target="_blank" rel="noopener">
                        {t.sync_create_fine_grained}
                    </a>
                </p>
                <button class="btn primary" on:click=connect>
                    {t.connect}
                </button>
            }
            .into_any()
        } else {
            let repo = conf.repo.contains('/').then(|| {
                view! {
                    <a href=format!("https://github.com/{}", conf.repo) target="_blank" rel="noopener">
                        {t.open_repo}
                    </a>
                }
            });
            let last = if conf.last_sync > 0.0 {
                format::date(l, &web::local_from_ms(conf.last_sync), true)
            } else {
                t.never.to_string()
            };
            view! {
                <p>
                    <span class="badge good">{t.sync_connected}</span>" "{repo}
                </p>
                <p class="muted">{t.last_sync}": "{last}</p>
                <label class="switch">
                    <input
                        type="checkbox"
                        prop:checked=conf.auto
                        on:change=move |ev| {
                            let on = event_target_checked(&ev);
                            store.update_sync(|c| c.auto = on);
                        }
                    />
                    <span>{t.auto_sync}</span>
                </label>
                <div class="btn-row">
                    <button
                        class="btn primary"
                        disabled=move || store.sync_busy.get()
                        on:click=move |_| run_sync(store, ui, false)
                    >
                        {move || if store.sync_busy.get() { store.t().syncing } else { store.t().sync_now }}
                    </button>
                    <button class="btn" on:click=disconnect>
                        {t.disconnect}
                    </button>
                </div>
            }
            .into_any()
        };
        view! {
            <section class="card">
                <h2>{t.sync_title}</h2>
                <p class="muted">{t.sync_intro}</p>
                {body}
            </section>
        }
    };

    let data = move || {
        let t = store.t();
        let (total, has_demo) = store.data.with(|d| {
            let active = d.records.iter().filter(|r| !r.deleted);
            (active.clone().count(), active.clone().any(|r| r.demo))
        });
        view! {
            <section class="card">
                <h2>{t.data_title}</h2>
                <div class="btn-grid">
                    <button class="btn" on:click=export_json>
                        {t.export_json}
                    </button>
                    <label class="btn">
                        {t.import_json}
                        <input type="file" accept=".json,application/json" hidden on:change=import_json />
                    </label>
                    {has_demo.then(|| view! {
                        <button class="btn" on:click=remove_demo>
                            {t.remove_demo}
                        </button>
                    })}
                    <button class="btn danger" on:click=delete_all disabled=total == 0>
                        {t.delete_all}
                    </button>
                </div>
                <p class="hint">{t.backup_hint}</p>
                <p class="hint">{t.storage_note}</p>
            </section>
            <section class="card about">
                <h2>{t.about}</h2>
                <p class="muted">
                    "Fuel Log v"{env!("CARGO_PKG_VERSION")}" · "{fill(t.n_records, &[("n", &total)])}
                </p>
            </section>
        }
    };

    view! {
        {general}
        {vehicles}
        {prices}
        {sync}
        {data}
    }
}
