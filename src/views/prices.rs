use leptos::prelude::*;
use leptos::task::spawn_local;

use super::{Icon, icon};
use crate::app::{Prefill, Ui};
use crate::format::{self, money, num};
use crate::i18n::fill;
use crate::model::FuelKind;
use crate::places::geo::distance_km;
use crate::places::{self, Issue, Query, SearchResult, Station, sort_stations};
use crate::store::Store;
use crate::web;

pub const RADII: [u32; 5] = [2, 5, 10, 15, 25];
const HISTORY_MATCH_KM: f64 = 0.15;

/// Last price search, kept while switching tabs.
#[derive(Clone, Debug, PartialEq)]
pub struct PriceSearch {
    pub origin: (f64, f64),
    pub label: String,
    pub fuel: FuelKind,
    pub radius: u32,
    pub result: SearchResult,
}

/// Flag emoji for an ISO country code.
fn flag(code: &str) -> String {
    code.chars()
        .filter_map(|c| char::from_u32(0x1F1E6 + (c.to_ascii_uppercase() as u32).checked_sub('A' as u32)?))
        .collect()
}

/// Display name of a station.
pub fn station_title(st: &Station, t: &crate::i18n::Texts) -> String {
    let title = st.title();
    if title.is_empty() { t.unnamed_station.to_string() } else { title }
}

/// A euro price in the display currency, plus the euro value when converted.
pub fn local_price(store: Store, eur: f64) -> (String, Option<String>) {
    let l = store.lang();
    let cur = store.currency();
    let dec = |v: f64| if v >= 10.0 { 2 } else { 3 };
    match (store.convert_eur(eur), store.currency_iso()) {
        (Some(v), Some(iso)) if iso != "EUR" => (money(l, &cur, v, dec(v)), Some(money(l, "€", eur, 3))),
        (Some(v), _) => (money(l, &cur, v, dec(v)), None),
        (None, _) => (money(l, "€", eur, 3), None),
    }
}

/// Runs a search around a position and stores the result.
fn run_search(store: Store, ui: Ui, loading: RwSignal<bool>, origin: (f64, f64), label: String) {
    let (fuel, radius, key) = store.settings.with_untracked(|s| (s.search_fuel, s.search_radius, s.tankerkoenig_key.clone()));
    loading.set(true);
    store.refresh_rates();
    spawn_local(async move {
        let result = places::search(Query { lat: origin.0, lng: origin.1, radius_km: radius as f64, fuel, tankerkoenig_key: key }).await;
        ui.price_search.set(Some(PriceSearch { origin, label, fuel, radius, result }));
        loading.set(false);
    });
}

#[component]
pub fn PricesView() -> impl IntoView {
    let store = expect_context::<Store>();
    let ui = expect_context::<Ui>();
    let loading = RwSignal::new(false);
    let place = RwSignal::new(String::new());

    let rerun = move || {
        if let Some(ps) = ui.price_search.get_untracked() {
            run_search(store, ui, loading, ps.origin, ps.label);
        }
    };
    let near_me = move |_| {
        loading.set(true);
        spawn_local(async move {
            match web::geolocate().await {
                Ok(c) => run_search(store, ui, loading, c, store.t().current_location.to_string()),
                Err(e) => {
                    loading.set(false);
                    ui.error(format!("{} ({e})", store.t().locate_failed));
                }
            }
        });
    };
    let search_place = move || {
        let q = place.get_untracked();
        if q.trim().is_empty() {
            return;
        }
        loading.set(true);
        spawn_local(async move {
            match places::geocode(&q).await {
                Ok(Some((lat, lng, name))) => {
                    let short = name.split(',').take(2).collect::<Vec<_>>().join(",");
                    run_search(store, ui, loading, (lat, lng), short);
                }
                Ok(None) => {
                    loading.set(false);
                    ui.error(store.t().place_not_found);
                }
                Err(e) => {
                    loading.set(false);
                    ui.error(format!("{} ({e})", store.t().place_not_found));
                }
            }
        });
    };

    let controls = move || {
        let t = store.t();
        let s = store.settings.get();
        view! {
            <section class="card search-card">
                <div class="pair">
                    <label class="field">
                        <span>{t.fuel}</span>
                        <select on:change=move |ev| {
                            let f = FuelKind::from_key(&event_target_value(&ev));
                            store.update_settings(|s| s.search_fuel = f);
                            rerun();
                        }>
                            {FuelKind::ALL
                                .into_iter()
                                .map(|f| view! { <option value=f.key() selected=f == s.search_fuel>{f.name()}</option> })
                                .collect_view()}
                        </select>
                    </label>
                    <label class="field">
                        <span>{t.radius}</span>
                        <select on:change=move |ev| {
                            let r = event_target_value(&ev).parse().unwrap_or(10);
                            store.update_settings(|s| s.search_radius = r);
                            rerun();
                        }>
                            {RADII
                                .into_iter()
                                .map(|r| view! { <option value=r.to_string() selected=r == s.search_radius>{format!("{r} km")}</option> })
                                .collect_view()}
                        </select>
                    </label>
                </div>
                <div class="row-between">
                    <div class="seg" role="group">
                        <button
                            class=if s.sort_by_price { "on" } else { "" }
                            on:click=move |_| store.update_settings(|s| s.sort_by_price = true)
                        >
                            {t.sort_price}
                        </button>
                        <button
                            class=if s.sort_by_price { "" } else { "on" }
                            on:click=move |_| store.update_settings(|s| s.sort_by_price = false)
                        >
                            {t.sort_distance}
                        </button>
                    </div>
                    <label class="switch">
                        <input
                            type="checkbox"
                            prop:checked=s.only_open
                            on:change=move |ev| {
                                let on = event_target_checked(&ev);
                                store.update_settings(|s| s.only_open = on);
                            }
                        />
                        <span>{t.only_open}</span>
                    </label>
                </div>
                <button class="btn primary full" disabled=move || loading.get() on:click=near_me>
                    {icon(Icon::Locate)}
                    {move || if loading.get() { store.t().searching } else { store.t().search_near_me }}
                </button>
                <div class="input-row">
                    <input
                        type="search"
                        placeholder=t.place_placeholder
                        prop:value=move || place.get()
                        on:input=move |ev| place.set(event_target_value(&ev))
                        on:keydown=move |ev| {
                            if ev.key() == "Enter" {
                                search_place();
                            }
                        }
                    />
                    <button class="btn" disabled=move || loading.get() on:click=move |_| search_place()>
                        {t.search}
                    </button>
                </div>
            </section>
        }
    };

    let results = move || {
        let ps = ui.price_search.get()?;
        let t = store.t();
        let l = store.lang();
        let s = store.settings.get();
        let fuel = ps.fuel;
        let mut list = ps.result.stations.clone();
        if s.only_open {
            list.retain(|st| st.is_open != Some(false));
        }
        sort_stations(&mut list, fuel, s.sort_by_price);

        let history: Vec<(f64, f64, String, f64)> = store.data.with(|d| {
            d.records
                .iter()
                .filter(|r| !r.deleted)
                .filter_map(|r| {
                    let (lat, lng) = r.coords()?;
                    Some((lat, lng, r.date.clone(), r.cost() / r.liters.filter(|v| *v > 0.0)?))
                })
                .collect()
        });

        let live: Vec<f64> = list.iter().filter_map(|st| st.price(fuel)).collect();
        let cheapest = live.iter().copied().reduce(f64::min);
        let average = (!live.is_empty()).then(|| live.iter().sum::<f64>() / live.len() as f64);

        let issues = ps.result.issues.iter().map(|i| match i {
            Issue::MissingKey => view! {
                <p class="note warn">{t.need_tk_key}" "<a href="#/settings">{t.nav_settings}" →"</a></p>
            }
            .into_any(),
            Issue::Failed(src, e) => view! {
                <p class="note warn">{fill(t.source_failed, &[("source", &src.name()), ("error", e)])}</p>
            }
            .into_any(),
        });
        let averages = ps.result.averages.iter().map(|a| {
            let price = a.price(fuel).map(|p| {
                let (main, _) = local_price(store, p);
                let native = a
                    .native_price(fuel)
                    .filter(|_| !a.currency.is_empty())
                    .map(|n| format!(" ({} {}/L)", num(l, n, 2), a.currency))
                    .unwrap_or_default();
                format!("{main}/L{native}")
            });
            view! {
                <p class="note">
                    {flag(a.country)}" "
                    {fill(
                        t.country_average,
                        &[
                            ("country", &a.country),
                            ("date", &format::short_date(l, &a.date)),
                            ("price", &price.unwrap_or_else(|| format::DASH.to_string())),
                        ],
                    )}
                </p>
            }
        });
        let fx_note = store.currency_iso().filter(|c| c != "EUR").map(|_| {
            let rates = store.rates.get();
            let text = match &rates {
                Some(r) => fill(t.fx_note, &[("date", &format::short_date(l, &r.date)), ("markup", &num(l, s.card_markup, 1))]),
                None => t.fx_missing.to_string(),
            };
            view! { <p class="note muted">{text}</p> }
        });

        let cards = list
            .into_iter()
            .map(|st| {
                let last = history
                    .iter()
                    .filter(|(lat, lng, _, _)| distance_km((*lat, *lng), (st.lat, st.lng)) < HISTORY_MATCH_KM)
                    .max_by(|a, b| a.2.cmp(&b.2))
                    .map(|(_, _, date, p)| {
                        fill(t.last_paid, &[("date", &format::short_date(l, date)), ("price", &money(l, &s.currency, *p, 3))])
                    });
                station_card(store, ui, st, fuel, last)
            })
            .collect_view();

        Some(view! {
            {issues.collect_view()}
            {averages.collect_view()}
            {fx_note}
            <p class="muted count">
                {fill(t.stations_found, &[("n", &ps.result.stations.len()), ("km", &ps.radius), ("place", &ps.label)])}
                {cheapest.map(|c| format!(" · {} {}", t.cheapest, local_price(store, c).0))}
                {average.map(|a| format!(" · ∅ {}", local_price(store, a).0))}
            </p>
            <div class="rec-list">{cards}</div>
            <p class="hint attribution">{t.price_attribution}</p>
        })
    };

    view! {
        {controls}
        {results}
    }
}

fn station_card(store: Store, ui: Ui, st: Station, fuel: FuelKind, last: Option<String>) -> impl IntoView {
    let t = store.t();
    let l = store.lang();
    let provider = store.settings.with(|s| s.map_provider);
    let route = provider.route_url(st.lat, st.lng, &st.title());
    let (price_main, price_sub, estimated) = match (st.price(fuel), st.avg) {
        (Some(p), _) => {
            let (m, s) = local_price(store, p);
            (m, s, false)
        }
        (None, Some(a)) => (format!("~{}", local_price(store, a).0), Some(t.weekly_avg.to_string()), true),
        (None, None) => (format::DASH.to_string(), Some(t.no_live_price.to_string()), true),
    };
    let open = st.is_open.map(|o| if o { (t.open, "badge good") } else { (t.closed, "badge warn") });
    let st2 = st.clone();
    view! {
        <article class="card station">
            <div class="st-head">
                <div class=if estimated { "st-price est" } else { "st-price" }>
                    <b>{price_main}</b>
                    {price_sub.map(|s| view! { <small>{s}</small> })}
                </div>
                <div class="st-main">
                    <b>{station_title(&st, t)}</b>
                    <span class="muted">{st.address.clone()}</span>
                </div>
            </div>
            <div class="rec-tags">
                <span class="badge">{format!("{} km", num(l, st.dist_km, 1))}</span>
                {st.country.map(|c| view! { <span class="badge">{flag(c)}" "{c}</span> })}
                {open.map(|(text, class)| view! { <span class=class>{text}</span> })}
                {last.map(|s| view! { <span class="badge good">{s}</span> })}
                <span class="badge">{st.source.name()}</span>
            </div>
            <div class="btn-row">
                <a class="btn small" href=route target="_blank" rel="noopener">
                    {icon(Icon::Pin)}
                    {t.navigate}
                </a>
                <button
                    class="btn small primary"
                    on:click=move |_| {
                        ui.prefill.set(Some(Prefill { station: st2.clone(), fuel }));
                        web::navigate("#/add");
                    }
                >
                    {t.fill_here}
                </button>
            </div>
        </article>
    }
}

#[cfg(test)]
mod tests {
    use super::flag;

    #[test]
    fn flags() {
        assert_eq!(flag("CZ"), "🇨🇿");
        assert_eq!(flag("de"), "🇩🇪");
    }
}
