use leptos::prelude::*;
use leptos::task::spawn_local;

use super::map::{MapView, Marker, SlippyMap};
use super::{Icon, icon};
use crate::app::{Prefill, Ui};
use crate::format::{DASH, money, num};
use crate::i18n::fill;
use crate::places::brands;
use crate::places::geo::Bounds;
use crate::places::tankerkoenig::{self as tk, TkError, TkFuel, TkStation};
use crate::places::{self, Known, Station};
use crate::store::Store;
use crate::web;

/// Country covered by the price source.
const COUNTRY: &str = "de";
const GERMANY: Bounds = Bounds { south: 47.2, west: 5.8, north: 55.1, east: 15.1 };
const MIN_ZOOM: f64 = 5.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Found(Vec<TkStation>),
    OutOfRange,
    Failed(TkError),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PriceSearch {
    pub origin: (f64, f64),
    pub label: String,
    pub radius: u32,
    pub outcome: Outcome,
}

/// Price page state, kept while switching tabs.
#[derive(Clone, Copy)]
pub struct PriceState {
    pub fuel: RwSignal<Option<TkFuel>>,
    pub radius: RwSignal<u32>,
    pub by_distance: RwSignal<bool>,
    pub only_open: RwSignal<bool>,
    pub place: RwSignal<String>,
    pub search: RwSignal<Option<PriceSearch>>,
    pub loading: RwSignal<bool>,
    pub map: MapView,
    pub selected: RwSignal<Option<String>>,
}

impl Default for PriceState {
    fn default() -> Self {
        Self {
            fuel: RwSignal::new(None),
            radius: RwSignal::new(5),
            by_distance: RwSignal::new(false),
            only_open: RwSignal::new(true),
            place: RwSignal::new(String::new()),
            search: RwSignal::new(None),
            loading: RwSignal::new(false),
            map: MapView::default(),
            selected: RwSignal::new(None),
        }
    }
}

/// Looks up prices around `origin` unless `country` is known to be outside the covered one.
fn run_search(store: Store, ps: PriceState, origin: (f64, f64), label: String, country: Option<String>) {
    let radius = ps.radius.get_untracked();
    let key = store.tk_key.get_untracked();
    ps.loading.set(true);
    spawn_local(async move {
        let outcome = if country.as_deref().is_some_and(|c| c != COUNTRY) {
            Outcome::OutOfRange
        } else {
            match tk::stations(&key, origin, radius).await {
                Ok(list) => Outcome::Found(list),
                Err(e) => Outcome::Failed(e),
            }
        };
        if outcome != Outcome::OutOfRange {
            ps.map.fit(origin, radius as f64);
        }
        ps.selected.set(None);
        ps.search.set(Some(PriceSearch { origin, label, radius, outcome }));
        ps.loading.set(false);
    });
}

/// Searches around a position after checking which country it is in.
fn search_at(store: Store, ps: PriceState, origin: (f64, f64), label: String) {
    ps.loading.set(true);
    spawn_local(async move {
        let country = places::country_at(origin).await.ok().map(Option::unwrap_or_default);
        run_search(store, ps, origin, label, country);
    });
}

fn sorted(mut list: Vec<TkStation>, fuel: TkFuel, by_distance: bool, only_open: bool) -> Vec<TkStation> {
    if only_open {
        list.retain(|s| s.is_open);
    }
    list.sort_by(|a, b| {
        let price = |s: &TkStation| s.price(fuel).unwrap_or(f64::INFINITY);
        if by_distance {
            a.dist_km.total_cmp(&b.dist_km)
        } else {
            price(a).total_cmp(&price(b)).then(a.dist_km.total_cmp(&b.dist_km))
        }
    });
    list
}

fn cheapest(list: &[TkStation], fuel: TkFuel) -> Option<f64> {
    list.iter().filter_map(|st| st.price(fuel)).reduce(f64::min)
}

/// Scrolls an element into view only as far as needed.
fn scroll_to(id: &str) {
    if let Some(el) = document().get_element_by_id(id) {
        let opts = web_sys::ScrollIntoViewOptions::new();
        opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
        el.scroll_into_view_with_scroll_into_view_options(&opts);
    }
}

#[component]
pub fn PricesView() -> impl IntoView {
    let store = expect_context::<Store>();
    let ui = expect_context::<Ui>();
    let ps = ui.prices;

    let fuel = Memo::new(move |_| {
        ps.fuel.get().unwrap_or_else(|| TkFuel::for_vehicle(&store.vehicle_fuels(&store.active_vehicle())))
    });
    let visible = Memo::new(move |_| match ps.search.get()?.outcome {
        Outcome::Found(list) => Some(sorted(list, fuel.get(), ps.by_distance.get(), ps.only_open.get())),
        _ => None,
    });

    let near_me = move |_| {
        ps.loading.set(true);
        spawn_local(async move {
            let t = store.t();
            match web::geolocate().await {
                Ok(c) => search_at(store, ps, c, t.your_location.to_string()),
                Err(e) => {
                    ps.loading.set(false);
                    ui.error(format!("{} ({e})", t.locate_failed));
                }
            }
        });
    };

    let search_place = move || {
        let q = ps.place.get_untracked().trim().to_string();
        if q.is_empty() {
            return;
        }
        ps.loading.set(true);
        spawn_local(async move {
            let t = store.t();
            match places::find_place(&q).await {
                Ok(Some(p)) => run_search(store, ps, (p.lat, p.lng), p.name, Some(p.country)),
                Ok(None) => {
                    ps.loading.set(false);
                    ui.error(t.place_not_found);
                }
                Err(e) => {
                    ps.loading.set(false);
                    ui.error(format!("{} ({e})", t.place_not_found));
                }
            }
        });
    };

    let rerun = move || {
        if let Some(s) = ps.search.get_untracked()
            && s.outcome != Outcome::OutOfRange
        {
            run_search(store, ps, s.origin, s.label, None);
        }
    };

    let controls = move || {
        let t = store.t();
        let current = fuel.get();
        let radius = ps.radius.get();
        let by_distance = ps.by_distance.get();
        view! {
            <section class="card search-card">
                <div class="chips wrap">
                    {TkFuel::ALL
                        .into_iter()
                        .map(|f| {
                            view! {
                                <button
                                    type="button"
                                    class=if f == current { "chip active" } else { "chip" }
                                    on:click=move |_| ps.fuel.set(Some(f))
                                >
                                    {f.name()}
                                </button>
                            }
                        })
                        .collect_view()}
                </div>
                <div class="grid2">
                    <label class="field">
                        <span>{t.radius}</span>
                        <select on:change=move |ev| {
                            ps.radius.set(event_target_value(&ev).parse().unwrap_or(5));
                            rerun();
                        }>
                            {tk::RADII_KM
                                .into_iter()
                                .map(|r| view! { <option value=r.to_string() selected=r == radius>{format!("{r} km")}</option> })
                                .collect_view()}
                        </select>
                    </label>
                    <div class="field">
                        <span>{t.sort_by}</span>
                        <div class="chips">
                            <button
                                type="button"
                                class=if by_distance { "chip" } else { "chip active" }
                                on:click=move |_| ps.by_distance.set(false)
                            >
                                {t.sort_cheapest}
                            </button>
                            <button
                                type="button"
                                class=if by_distance { "chip active" } else { "chip" }
                                on:click=move |_| ps.by_distance.set(true)
                            >
                                {t.sort_nearest}
                            </button>
                        </div>
                    </div>
                </div>
                <label class="switch">
                    <input
                        type="checkbox"
                        prop:checked=move || ps.only_open.get()
                        on:change=move |ev| ps.only_open.set(event_target_checked(&ev))
                    />
                    <span>{t.only_open}</span>
                </label>
                <button class="btn primary full" disabled=move || ps.loading.get() on:click=near_me>
                    {icon(Icon::Locate)}
                    {move || if ps.loading.get() { store.t().searching } else { store.t().near_me }}
                </button>
                <div class="input-row">
                    <input
                        type="search"
                        autocomplete="off"
                        placeholder=t.place_placeholder
                        prop:value=move || ps.place.get()
                        on:input=move |ev| ps.place.set(event_target_value(&ev))
                        on:keydown=move |ev| {
                            if ev.key() == "Enter" {
                                ev.prevent_default();
                                search_place();
                            }
                        }
                    />
                    <button type="button" class="btn" disabled=move || ps.loading.get() on:click=move |_| search_place()>
                        {icon(Icon::Search)}
                        {t.search_here}
                    </button>
                </div>
            </section>
        }
    };

    let markers = Signal::derive(move || {
        let Some(list) = visible.get() else { return Vec::new() };
        let l = store.lang();
        let f = fuel.get();
        let best = cheapest(&list, f);
        list.iter()
            .rev()
            .map(|st| {
                let price = st.price(f);
                Marker {
                    id: st.id.clone(),
                    lat: st.lat,
                    lng: st.lng,
                    label: price.map(|p| num(l, p, 3)).unwrap_or_else(|| DASH.to_string()),
                    class: match (price.is_some() && price == best, price.is_some()) {
                        (true, _) => "best",
                        (false, true) => "",
                        (false, false) => "dim",
                    },
                    logo: brands::logo(&st.brand, &st.name),
                }
            })
            .collect()
    });
    let circle = Signal::derive(move || {
        ps.search.with(|s| s.as_ref().filter(|s| s.outcome != Outcome::OutOfRange).map(|s| (s.origin, s.radius as f64)))
    });
    let selected_station = Memo::new(move |_| {
        let id = ps.selected.get()?;
        visible.with(|v| v.as_ref()?.iter().find(|s| s.id == id).cloned())
    });
    let on_select = Callback::new(move |id: String| {
        if let Some(st) = visible.with_untracked(|v| v.as_ref().and_then(|v| v.iter().find(|s| s.id == id).cloned())) {
            ps.map.focus((st.lat, st.lng));
        }
        ps.selected.set(Some(id));
    });
    let popup_at = Signal::derive(move || selected_station.with(|s| s.as_ref().map(|s| (s.lat, s.lng))));
    let popup = move || {
        let st = selected_station.get()?;
        let f = fuel.get();
        let best = visible.with(|v| v.as_deref().and_then(|v| cheapest(v, f)));
        let station = named_stations(store, std::slice::from_ref(&st)).pop()?;
        Some(station_popup(store, ui, st, station, f, best))
    };
    let on_background = Callback::new(move |()| ps.selected.set(None));

    let map = move || {
        let t = store.t();
        view! {
            <div id="price-map" class="map-wrap">
                <SlippyMap
                    view=ps.map
                    bounds=GERMANY
                    min_zoom=MIN_ZOOM
                    markers=markers
                    circle=circle
                    selected=ps.selected
                    on_select=on_select
                    popup_at=popup_at
                    popup=ViewFn::from(popup)
                    on_background=on_background
                    zoom_in_label=t.zoom_in
                    zoom_out_label=t.zoom_out
                >
                    {move || {
                        (ps.map.moved.get() && !ps.loading.get())
                            .then(|| {
                                view! {
                                    <button
                                        type="button"
                                        class="btn small primary"
                                        on:click=move |_| {
                                            let c = ps.map.center.get_untracked();
                                            search_at(store, ps, c, store.t().this_area.to_string());
                                        }
                                    >
                                        {icon(Icon::Search)}
                                        {store.t().search_this_area}
                                    </button>
                                }
                            })
                    }}
                </SlippyMap>
            </div>
        }
    };

    let results = move || {
        let s = ps.search.get()?;
        let t = store.t();
        let l = store.lang();
        let total = match &s.outcome {
            Outcome::OutOfRange => {
                return Some(view! { <p class="card note warn">{t.tk_out_of_range}</p> }.into_any());
            }
            Outcome::Failed(TkError::InvalidKey) => {
                return Some(
                    view! {
                        <p class="card note warn">
                            {t.tk_key_rejected}" "<a href="#/settings">{t.nav_settings}" →"</a>
                        </p>
                    }
                    .into_any(),
                );
            }
            Outcome::Failed(TkError::Api(e) | TkError::Network(e)) => {
                return Some(view! { <p class="card note warn">{t.tk_failed}" ("{e.clone()}")"</p> }.into_any());
            }
            Outcome::Found(list) if list.is_empty() => {
                return Some(view! { <p class="card note">{t.tk_none_found}</p> }.into_any());
            }
            Outcome::Found(list) => list.len(),
        };
        let fuel = fuel.get();
        let list = visible.get().unwrap_or_default();
        let best = cheapest(&list, fuel);
        let stations = named_stations(store, &list);
        let cards = list
            .into_iter()
            .zip(stations)
            .map(|(st, station)| station_card(store, ui, st, station, fuel, best))
            .collect_view();
        Some(
            view! {
                <p class="muted count">
                    {fill(t.stations_found, &[("n", &total), ("km", &s.radius), ("place", &s.label)])}
                    {best.map(|b| format!(" · {} {}", t.sort_cheapest, money(l, "€", b, 3)))}
                </p>
                <div class="rec-list">{cards}</div>
            }
            .into_any(),
        )
    };

    move || {
        let t = store.t();
        if store.tk_key.with(String::is_empty) {
            return view! {
                <section class="card">
                    <h2>{t.tk_title}</h2>
                    <p class="muted">{t.tk_intro}</p>
                    <p>{t.tk_need_key}</p>
                    <a class="btn primary" href="#/settings">
                        {icon(Icon::Settings)}
                        {t.nav_settings}
                    </a>
                </section>
            }
            .into_any();
        }
        view! {
            <div class="prices-main">
                {controls}
                <div class="prices-results">
                    {results}
                    <p class="hint attribution">{t.tk_attribution}</p>
                </div>
            </div>
            {map}
        }
        .into_any()
    }
}

/// Stations with the names you used for them in your records.
fn named_stations(store: Store, list: &[TkStation]) -> Vec<Station> {
    store.data.with(|d| {
        let known: Vec<Known> = d
            .records
            .iter()
            .filter(|r| !r.deleted)
            .filter_map(|r| {
                let (lat, lng) = r.coords()?;
                Some(Known { lat, lng, name: &r.station, address: &r.address, date: &r.date })
            })
            .collect();
        let mut stations: Vec<Station> = list.iter().map(TkStation::to_station).collect();
        places::apply_own_names(&mut stations, &known);
        stations
    })
}

fn display_title(store: Store, station: &Station) -> String {
    let title = station.title();
    if title.is_empty() { store.t().unnamed_station.to_string() } else { title }
}

/// Brand logo, or the brand's first letter when there is none.
fn brand_mark(st: &TkStation) -> impl IntoView + use<> {
    match brands::logo(&st.brand, &st.name) {
        Some(src) => view! { <img class="brand-logo" src=src alt=st.brand.clone() /> }.into_any(),
        None => view! { <span class="brand-logo letter">{brands::initial(&st.brand, &st.name)}</span> }.into_any(),
    }
}

fn route_url(store: Store, st: &TkStation, title: &str) -> String {
    store.settings.with_untracked(|s| s.map_provider).route_url(st.lat, st.lng, title)
}

fn fill_here(ui: Ui, station: &Station, fuel: TkFuel, price: Option<f64>) {
    ui.prefill.set(Some(Prefill { station: station.clone(), fuel: fuel.name(), price }));
    web::navigate("#/add");
}

fn station_popup(store: Store, ui: Ui, st: TkStation, station: Station, fuel: TkFuel, best: Option<f64>) -> impl IntoView {
    let t = store.t();
    let l = store.lang();
    let title = display_title(store, &station);
    let route = route_url(store, &st, &title);
    let price = st.price(fuel);
    let prices = TkFuel::ALL
        .into_iter()
        .map(|f| {
            let p = st.price(f);
            let class = match (f == fuel, p.is_some() && p == best && f == fuel) {
                (true, true) => "pop-fuel on best",
                (true, false) => "pop-fuel on",
                _ => "pop-fuel",
            };
            view! {
                <span class=class>
                    <small>{f.name()}</small>
                    <b>{p.map(|p| money(l, "€", p, 3)).unwrap_or_else(|| DASH.to_string())}</b>
                </span>
            }
        })
        .collect_view();
    view! {
        <div class="pop-head">
            {brand_mark(&st)}
            <div class="pop-title">
                <b>{title}</b>
                <small class="muted">{st.address.clone()}</small>
            </div>
            <button
                type="button"
                class="pop-close"
                aria-label=t.close
                title=t.close
                on:click=move |_| ui.prices.selected.set(None)
            >
                "×"
            </button>
        </div>
        <div class="pop-prices">{prices}</div>
        <div class="rec-tags">
            <span class="badge">{format!("{} km", num(l, st.dist_km, 1))}</span>
            {if st.is_open {
                view! { <span class="badge good">{t.open_now}</span> }.into_any()
            } else {
                view! { <span class="badge warn">{t.closed_now}</span> }.into_any()
            }}
        </div>
        <div class="btn-row">
            <a class="btn small primary" href=route target="_blank" rel="noopener noreferrer">
                {icon(Icon::Pin)}
                {t.navigate}
            </a>
            <button type="button" class="btn small" on:click=move |_| fill_here(ui, &station, fuel, price)>
                {t.fill_here}
            </button>
        </div>
    }
}

fn station_card(store: Store, ui: Ui, st: TkStation, station: Station, fuel: TkFuel, best: Option<f64>) -> impl IntoView {
    let t = store.t();
    let l = store.lang();
    let ps = ui.prices;
    let price = st.price(fuel);
    let is_best = price.is_some() && price == best;
    let title = display_title(store, &station);
    let route = route_url(store, &st, &title);
    let others: Vec<String> = TkFuel::ALL
        .into_iter()
        .filter(|f| *f != fuel)
        .filter_map(|f| Some(format!("{} {}", f.name(), money(l, "€", st.price(f)?, 3))))
        .collect();
    let base = match (is_best, price.is_some()) {
        (true, _) => "card station best",
        (false, true) => "card station",
        (false, false) => "card station dim",
    };
    let (id, sel_id) = (st.id.clone(), st.id.clone());
    let pos = (st.lat, st.lng);
    view! {
        <article
            id=format!("st-{}", st.id)
            class=move || {
                let sel = ps.selected.with(|s| s.as_deref() == Some(sel_id.as_str()));
                if sel { format!("{base} selected") } else { base.to_string() }
            }
        >
            <div
                class="st-head"
                on:click=move |_| {
                    ps.selected.set(Some(id.clone()));
                    ps.map.focus(pos);
                    scroll_to("price-map");
                }
            >
                <div class="st-price">
                    <b>{price.map(|p| money(l, "€", p, 3)).unwrap_or_else(|| DASH.to_string())}</b>
                    <small>{fuel.name()}</small>
                </div>
                <div class="st-main">
                    <div class="st-title">
                        {brand_mark(&st)}
                        <b>{title}</b>
                    </div>
                    <span class="muted">{st.address.clone()}</span>
                    {(!others.is_empty()).then(|| view! { <small class="muted">{others.join(" · ")}</small> })}
                </div>
            </div>
            <div class="rec-tags">
                <span class="badge">{format!("{} km", num(l, st.dist_km, 1))}</span>
                {if st.is_open {
                    view! { <span class="badge good">{t.open_now}</span> }.into_any()
                } else {
                    view! { <span class="badge warn">{t.closed_now}</span> }.into_any()
                }}
            </div>
            <div class="btn-row">
                <a class="btn small" href=route target="_blank" rel="noopener noreferrer">
                    {icon(Icon::Pin)}
                    {t.navigate}
                </a>
                <button type="button" class="btn small primary" on:click=move |_| fill_here(ui, &station, fuel, price)>
                    {t.fill_here}
                </button>
            </div>
        </article>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(id: &str, dist: f64, e10: Option<f64>, open: bool) -> TkStation {
        TkStation { id: id.into(), dist_km: dist, e10, is_open: open, ..Default::default() }
    }

    #[test]
    fn sorting() {
        let list = vec![st("a", 1.0, Some(1.80), true), st("b", 2.0, Some(1.70), true), st("c", 0.5, None, true), st("d", 0.1, Some(1.6), false)];
        let ids = |v: Vec<TkStation>| v.into_iter().map(|s| s.id).collect::<Vec<_>>();
        assert_eq!(ids(sorted(list.clone(), TkFuel::E10, false, true)), ["b", "a", "c"]);
        assert_eq!(ids(sorted(list.clone(), TkFuel::E10, true, true)), ["c", "a", "b"]);
        assert_eq!(ids(sorted(list.clone(), TkFuel::E10, false, false)), ["d", "b", "a", "c"]);
        assert_eq!(cheapest(&list, TkFuel::E10), Some(1.6));
    }
}
