use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use super::load_demo;
use crate::app::{Ui, run_sync};
use crate::i18n::{Lang, fill};
use crate::model::{AppData, MapProvider};
use crate::store::Store;
use crate::{csv, format, web};

const TOKEN_URL: &str = "https://github.com/settings/tokens/new?scopes=gist&description=Fuel%20Log";
const CURRENCIES: [&str; 9] = ["€", "Kč", "zł", "CHF", "£", "$", "¥", "Ft", "kr"];
const TANKERKOENIG_URL: &str = "https://creativecommons.tankerkoenig.de/";

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

    let export_csv = move |_| {
        let entries: Vec<(String, crate::stats::Entry)> = store
            .vehicles()
            .into_iter()
            .flat_map(|v| store.entries_for(&v.id).into_iter().map(move |e| (v.name.clone(), e)))
            .collect();
        let refs: Vec<(String, &crate::stats::Entry)> = entries.iter().map(|(n, e)| (n.clone(), e)).collect();
        web::download(&format!("fuel-log-{}.csv", today()), &csv::export(&refs), "text/csv;charset=utf-8");
    };

    let import_csv = move |ev: leptos::ev::Event| {
        let Some(file) = picked_file(&ev) else { return };
        spawn_local(async move {
            let t = store.t();
            let text = match web::read_file(file).await {
                Ok(text) => text,
                Err(e) => return ui.error(format!("{} ({e})", t.import_failed)),
            };
            let (rows, skipped) = csv::to_records(&csv::parse(&text), &mut web::new_id);
            let now = web::now_ms();
            let records = rows
                .into_iter()
                .map(|imp| {
                    let mut r = imp.rec;
                    r.vehicle_id = store.vehicle_by_name(&imp.vehicle);
                    r.created_at = now;
                    r.updated_at = now;
                    r
                })
                .collect::<Vec<_>>();
            let n = store.import(&AppData { version: 1, vehicles: Vec::new(), records });
            ui.toast(fill(t.csv_imported, &[("n", &n), ("skipped", &skipped)]));
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
        store.update_sync(|c| {
            c.token = token;
            c.auto = true;
            c.gist_id.clear();
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

    let general = move || {
        let t = store.t();
        let l = store.lang();
        let settings = store.settings.get();
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
                <label class="field">
                    <span>{t.currency}</span>
                    <input
                        list="dl-currency"
                        maxlength="5"
                        autocomplete="off"
                        prop:value=settings.currency.clone()
                        on:change=move |ev| {
                            let v = event_target_value(&ev).trim().to_string();
                            if !v.is_empty() {
                                store.update_settings(|s| s.currency = v);
                            }
                        }
                    />
                    <datalist id="dl-currency">
                        {CURRENCIES.map(|c| view! { <option value=c></option> }).collect_view()}
                    </datalist>
                </label>
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
                            view! {
                                <li>
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
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
                <button class="btn" on:click=add_vehicle>
                    {t.add_vehicle}
                </button>
            </section>
        }
    };

    let prices = move || {
        let t = store.t();
        let l = store.lang();
        let s = store.settings.get();
        let rates = store.rates.get();
        let iso = store.currency_iso();
        let rate_line = match (&iso, &rates) {
            (Some(code), _) if code == "EUR" => None,
            (Some(code), Some(r)) => r.rate(code).map(|rate| {
                fill(
                    t.rate_line,
                    &[("rate", &format::num(l, rate, 4)), ("code", code), ("date", &format::short_date(l, &r.date))],
                )
            }),
            (Some(_), None) => Some(t.fx_missing.to_string()),
            (None, _) => Some(t.currency_unknown.to_string()),
        };
        view! {
            <section class="card">
                <h2>{t.prices_title}</h2>
                <p class="muted">{t.prices_intro}</p>
                <label class="field">
                    <span>{t.tk_key}</span>
                    <input
                        autocomplete="off"
                        spellcheck="false"
                        placeholder="00000000-0000-0000-0000-000000000000"
                        prop:value=s.tankerkoenig_key.clone()
                        on:change=move |ev| {
                            let v = event_target_value(&ev).trim().to_string();
                            store.update_settings(|s| s.tankerkoenig_key = v);
                        }
                    />
                </label>
                <p class="hint">
                    {t.tk_key_hint}" "
                    <a href=TANKERKOENIG_URL target="_blank" rel="noopener">
                        "creativecommons.tankerkoenig.de ↗"
                    </a>
                </p>
                <label class="field">
                    <span>{t.card_markup}" (%)"</span>
                    <input
                        inputmode="decimal"
                        autocomplete="off"
                        prop:value=format::input_num(l, s.card_markup, 2)
                        on:change=move |ev| {
                            let v = format::parse_num(&event_target_value(&ev)).unwrap_or(0.0).clamp(0.0, 20.0);
                            store.update_settings(|s| s.card_markup = v);
                        }
                    />
                </label>
                <p class="hint">{t.card_markup_hint}</p>
                {rate_line.map(|r| view! { <p class="hint">{r}</p> })}
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
                <button class="btn primary" on:click=connect>
                    {t.connect}
                </button>
            }
            .into_any()
        } else {
            let gist = (!conf.gist_id.is_empty()).then(|| {
                view! {
                    <a href=format!("https://gist.github.com/{}", conf.gist_id) target="_blank" rel="noopener">
                        "Gist ↗"
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
                    <span class="badge good">{t.sync_connected}</span>" "{gist}
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
                    <button class="btn" on:click=export_csv>
                        {t.export_csv}
                    </button>
                    <label class="btn">
                        {t.import_csv}
                        <input type="file" accept=".csv,text/csv,text/plain" hidden on:change=import_csv />
                    </label>
                    {if has_demo {
                        view! {
                            <button class="btn" on:click=remove_demo>
                                {t.remove_demo}
                            </button>
                        }
                            .into_any()
                    } else {
                        view! {
                            <button class="btn" on:click=move |_| load_demo(store, ui)>
                                {t.load_demo}
                            </button>
                        }
                            .into_any()
                    }}
                    <button class="btn danger" on:click=delete_all disabled=total == 0>
                        {t.delete_all}
                    </button>
                </div>
                <p class="hint">{t.csv_hint}</p>
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
