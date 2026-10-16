use std::collections::HashMap;

use leptos::prelude::*;
use leptos::task::spawn_local;

use super::prices::{local_price, station_title};
use super::{Icon, icon};
use crate::app::Ui;
use crate::format::{self, input_num, money, num, parse_num};
use crate::i18n::fill;
use crate::model::{FuelKind, Record};
use crate::places::geo::distance_km;
use crate::places::{self, Query, Station, sort_stations};
use crate::stats::{SegState, analyze, parse_dt, sort_records};
use crate::store::Store;
use crate::web;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trio {
    Liters,
    Price,
    Total,
}

const KNOWN_STATION_KM: f64 = 0.15;
const NEARBY_KM: f64 = 2.0;

/// Distinct non-empty values, most frequent first.
fn by_frequency<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for v in values.map(str::trim).filter(|v| !v.is_empty()) {
        *counts.entry(v).or_default() += 1;
    }
    let mut out: Vec<(&str, usize)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    out.into_iter().map(|(v, _)| v.to_string()).collect()
}

fn datalist(id: &'static str, values: Vec<String>) -> impl IntoView {
    view! {
        <datalist id=id>
            {values.into_iter().map(|v| view! { <option value=v></option> }).collect_view()}
        </datalist>
    }
}

#[component]
pub fn RecordForm(#[prop(optional, into)] id: Option<String>) -> impl IntoView {
    let store = expect_context::<Store>();
    let ui = expect_context::<Ui>();
    let l = store.lang();

    let existing = id.as_deref().and_then(|id| store.record(id));
    if id.is_some() && existing.is_none() {
        return (move || {
            let t = store.t();
            view! {
                <div class="card">
                    <p>{t.record_not_found}</p>
                    <a class="btn" href="#/records">{t.nav_records}</a>
                </div>
            }
        })
        .into_any();
    }
    let editing = existing.is_some();
    let all = store.data.with_untracked(|d| d.records.iter().filter(|r| !r.deleted).cloned().collect::<Vec<_>>());
    let active = store.active_vehicle();
    let last_of_vehicle = {
        let mut rs: Vec<Record> = all.iter().filter(|r| r.vehicle_id == active).cloned().collect();
        sort_records(&mut rs);
        rs.pop()
    };
    let base = existing.unwrap_or_else(|| Record {
        id: web::new_id(),
        vehicle_id: active.clone(),
        date: web::now_local(),
        fuel: last_of_vehicle.as_ref().map(|r| r.fuel.clone()).unwrap_or_default(),
        payment: last_of_vehicle.as_ref().map(|r| r.payment.clone()).unwrap_or_default(),
        ..Default::default()
    });
    let rec_id = base.id.clone();
    let created_at = base.created_at;
    let demo = base.demo;
    let s = |v: Option<f64>, d: usize| v.map(|v| input_num(l, v, d)).unwrap_or_default();

    let vehicle = RwSignal::new(base.vehicle_id.clone());
    let date = RwSignal::new(base.date.clone());
    let odo = RwSignal::new(s(base.odometer, 1));
    let trip = RwSignal::new(String::new());
    let liters = RwSignal::new(s(base.liters, 2));
    let price = RwSignal::new(s(base.price, 3));
    let total = RwSignal::new(s(base.total, 2));
    let paid = RwSignal::new(s(base.paid, 2));
    let full = RwSignal::new(base.full);
    let missed = RwSignal::new(base.missed_previous);
    let fuel = RwSignal::new(base.fuel.clone());
    let station = RwSignal::new(base.station.clone());
    let address = RwSignal::new(base.address.clone());
    let coords = RwSignal::new(base.coords());
    let display = RwSignal::new(s(base.display_cons, 1));
    let payment = RwSignal::new(base.payment.clone());
    let notes = RwSignal::new(base.notes.clone());
    let order = RwSignal::new(if editing { vec![Trio::Total, Trio::Liters, Trio::Price] } else { Vec::new() });
    let computed = RwSignal::new(None::<Trio>);
    let locating = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    let others = {
        let rec_id = rec_id.clone();
        Memo::new(move |_| {
            let v = vehicle.get();
            store
                .records_for(&v)
                .into_iter()
                .filter(|r| r.id != rec_id)
                .collect::<Vec<_>>()
        })
    };
    let neighbors = Memo::new(move |_| {
        let d = date.get();
        let mut rs = others.get();
        sort_records(&mut rs);
        let prev = rs.iter().rev().find(|r| r.date < d).cloned();
        let next = rs.iter().find(|r| r.date > d).cloned();
        (prev, next)
    });
    let prev_odo = move || neighbors.with_untracked(|(p, _)| p.as_ref().and_then(|p| p.odometer));
    let sync_trip = move || {
        trip.set(match (prev_odo(), parse_num(&odo.get_untracked())) {
            (Some(p), Some(o)) => input_num(l, o - p, 1),
            _ => String::new(),
        });
    };
    sync_trip();

    let on_trio = move |f: Trio, v: String| {
        match f {
            Trio::Liters => liters.set(v),
            Trio::Price => price.set(v),
            Trio::Total => total.set(v),
        }
        order.update(|o| {
            o.retain(|x| *x != f);
            o.push(f);
        });
        let o = order.get_untracked();
        let target = (o.len() >= 2)
            .then(|| [Trio::Liters, Trio::Price, Trio::Total].into_iter().find(|x| !o[o.len() - 2..].contains(x)))
            .flatten();
        let (lv, pv, tv) = (
            parse_num(&liters.get_untracked()),
            parse_num(&price.get_untracked()),
            parse_num(&total.get_untracked()),
        );
        let result = match target {
            Some(Trio::Liters) => tv.zip(pv.filter(|p| *p > 0.0)).map(|(t, p)| (Trio::Liters, input_num(l, t / p, 2))),
            Some(Trio::Price) => tv.zip(lv.filter(|v| *v > 0.0)).map(|(t, v)| (Trio::Price, input_num(l, t / v, 3))),
            Some(Trio::Total) => lv.zip(pv).map(|(v, p)| (Trio::Total, input_num(l, v * p, 2))),
            None => None,
        };
        computed.set(result.as_ref().map(|(f, _)| *f));
        match result {
            Some((Trio::Liters, v)) => liters.set(v),
            Some((Trio::Price, v)) => price.set(v),
            Some((Trio::Total, v)) => total.set(v),
            None => {}
        }
    };

    let preview = {
        let rec_id = rec_id.clone();
        Memo::new(move |_| {
            let cand = Record {
                id: rec_id.clone(),
                vehicle_id: vehicle.get(),
                date: date.get(),
                odometer: Some(parse_num(&odo.get())?),
                liters: Some(parse_num(&liters.get())?),
                price: parse_num(&price.get()),
                total: parse_num(&total.get()),
                paid: parse_num(&paid.get()),
                full: full.get(),
                missed_previous: missed.get(),
                ..Default::default()
            };
            let mut rs = others.get();
            rs.push(cand);
            analyze(&rs).into_iter().find(|e| e.rec.id == rec_id)
        })
    };

    let suggestions = |f: fn(&Record) -> &str| by_frequency(all.iter().map(f));
    let stations = suggestions(|r| r.station.as_str());
    let with_defaults = |mut used: Vec<String>, defaults: &[&str]| {
        for d in defaults {
            if !used.iter().any(|u| u == d) {
                used.push(d.to_string());
            }
        }
        used
    };
    let fuels = with_defaults(suggestions(|r| r.fuel.as_str()), store.t().fuel_suggestions);
    let payments = with_defaults(suggestions(|r| r.payment.as_str()), store.t().payment_suggestions);

    let on_station_change = {
        let all = all.clone();
        move |name: String| {
            let latest = all
                .iter()
                .filter(|r| r.station.trim() == name.trim() && !name.trim().is_empty())
                .max_by(|a, b| a.date.cmp(&b.date));
            if let Some(r) = latest {
                if address.get_untracked().is_empty() {
                    address.set(r.address.clone());
                }
                if coords.get_untracked().is_none() {
                    coords.set(r.coords());
                }
            }
        }
    };

    let nearby = RwSignal::new(None::<Vec<Station>>);
    let search_kind = move || {
        FuelKind::from_name(&fuel.get_untracked()).unwrap_or_else(|| store.settings.with_untracked(|s| s.search_fuel))
    };

    let apply_station = move |st: &Station, kind: FuelKind| {
        let label = st.label();
        station.set(if label.is_empty() { store.t().unnamed_station.to_string() } else { label });
        if !st.address.is_empty() {
            address.set(st.address.clone());
        }
        coords.set(Some((st.lat, st.lng)));
        if fuel.get_untracked().trim().is_empty() {
            fuel.set(kind.name().to_string());
        }
        if price.get_untracked().trim().is_empty()
            && let Some(p) = st.price(kind).and_then(|p| store.convert_eur(p))
        {
            on_trio(Trio::Price, input_num(l, p, 3));
        }
        nearby.set(None);
    };

    if !editing && let Some(p) = ui.prefill.get_untracked() {
        ui.prefill.set(None);
        fuel.set(p.fuel.name().to_string());
        apply_station(&p.station, p.fuel);
    }

    let locate = {
        let all = all.clone();
        move |_| {
            locating.set(true);
            let all = all.clone();
            spawn_local(async move {
                let t = store.t();
                let c = match web::geolocate().await {
                    Ok(c) => c,
                    Err(e) => {
                        ui.error(format!("{} ({e})", t.locate_failed));
                        locating.set(false);
                        return;
                    }
                };
                coords.set(Some(c));
                store.refresh_rates();
                let query = Query {
                    lat: c.0,
                    lng: c.1,
                    radius_km: NEARBY_KM,
                    fuel: search_kind(),
                    tankerkoenig_key: store.settings.with_untracked(|s| s.tankerkoenig_key.clone()),
                };
                let (addr, found) = futures::join!(places::reverse_geocode(c), places::search(query));
                if address.get_untracked().is_empty()
                    && let Ok(Some(a)) = addr
                {
                    address.set(a);
                }
                let known = all
                    .iter()
                    .filter(|r| !r.station.is_empty())
                    .filter_map(|r| Some((distance_km(c, r.coords()?), r)))
                    .filter(|(d, _)| *d < KNOWN_STATION_KM)
                    .min_by(|a, b| a.0.total_cmp(&b.0));
                if let Some((_, r)) = known
                    && station.get_untracked().is_empty()
                {
                    station.set(r.station.clone());
                    if !r.address.is_empty() {
                        address.set(r.address.clone());
                    }
                    ui.toast(fill(t.station_recognized, &[("name", &r.station)]));
                }
                let mut list = found.stations;
                sort_stations(&mut list, FuelKind::E10, false);
                list.truncate(8);
                nearby.set(Some(list));
                locating.set(false);
            });
        }
    };

    let picker = move || {
        let list = nearby.get()?;
        let t = store.t();
        if list.is_empty() {
            return Some(view! { <p class="hint">{t.no_nearby}</p> }.into_any());
        }
        let kind = search_kind();
        Some(
            view! {
                <div class="picker">
                    <p class="hint">{t.pick_station}</p>
                    {list
                        .into_iter()
                        .map(|st| {
                            let price = st.price(kind).map(|p| local_price(store, p).0);
                            let dist = format!("{} km", num(l, st.dist_km, 1));
                            let pick = st.clone();
                            view! {
                                <button type="button" class="pick" on:click=move |_| apply_station(&pick, kind)>
                                    <span class="pick-main">
                                        <b>{station_title(&st, t)}</b>
                                        <small class="muted">{st.address.clone()}</small>
                                    </span>
                                    <span class="pick-side">
                                        {price.map(|p| view! { <b>{p}</b> })}
                                        <small class="muted">{dist}</small>
                                    </span>
                                </button>
                            }
                        })
                        .collect_view()}
                    <button type="button" class="link-btn" on:click=move |_| nearby.set(None)>
                        {t.close}
                    </button>
                </div>
            }
            .into_any(),
        )
    };

    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let t = store.t();
        let fail = |msg: &str| error.set(Some(msg.to_string()));
        let d = date.get_untracked();
        if parse_dt(&d).is_none() {
            return fail(t.err_date);
        }
        let Some(o) = parse_num(&odo.get_untracked()).filter(|v| *v >= 0.0) else {
            return fail(t.err_odometer);
        };
        let Some(lt) = parse_num(&liters.get_untracked()).filter(|v| *v > 0.0) else {
            return fail(t.err_liters);
        };
        let (pr, tot, pd) = (
            parse_num(&price.get_untracked()),
            parse_num(&total.get_untracked()),
            parse_num(&paid.get_untracked()),
        );
        if pr.is_none() && tot.is_none() && pd.is_none() {
            return fail(t.err_amount);
        }
        let (prev, next) = neighbors.get_untracked();
        if let Some(p) = prev.and_then(|p| p.odometer)
            && o <= p
            && !web::confirm(&fill(t.confirm_odo_lower, &[("km", &num(l, p, 0))]))
        {
            return;
        }
        if let Some(n) = next.and_then(|n| n.odometer)
            && o >= n
            && !web::confirm(&fill(t.confirm_odo_higher, &[("km", &num(l, n, 0))]))
        {
            return;
        }
        let text = |s: RwSignal<String>| s.get_untracked().trim().to_string();
        let (lat, lng) = coords.get_untracked().unzip();
        let v = vehicle.get_untracked();
        store.save_record(Record {
            id: rec_id.clone(),
            vehicle_id: v.clone(),
            date: d,
            odometer: Some(o),
            liters: Some(lt),
            price: pr,
            total: tot,
            paid: pd.filter(|p| Some(*p) != tot),
            full: full.get_untracked(),
            missed_previous: missed.get_untracked(),
            fuel: text(fuel),
            station: text(station),
            address: text(address),
            lat,
            lng,
            display_cons: parse_num(&display.get_untracked()),
            payment: text(payment),
            notes: notes.get_untracked().trim().to_string(),
            created_at,
            demo,
            ..Default::default()
        });
        store.set_active_vehicle(&v);
        ui.toast(t.saved);
        web::navigate("#/records");
    };

    let delete = {
        let id = base.id.clone();
        move |_| {
            let t = store.t();
            if web::confirm(t.confirm_delete) {
                store.delete_records(std::slice::from_ref(&id));
                ui.toast_undo(t.deleted, vec![id.clone()]);
                web::navigate("#/records");
            }
        }
    };

    let trio_input = move |f: Trio, sig: RwSignal<String>, label: String| {
        view! {
            <label class="field">
                <span>{label}</span>
                <input
                    inputmode="decimal"
                    autocomplete="off"
                    class=move || if computed.get() == Some(f) { "computed" } else { "" }
                    prop:value=move || sig.get()
                    on:input=move |ev| on_trio(f, event_target_value(&ev))
                />
            </label>
        }
    };
    let preview_view = move || {
        let t = store.t();
        let cur = store.currency();
        let e = preview.get()?;
        let (prev, _) = neighbors.get();
        let trip_line = e.trip.zip(prev).map(|(tr, p)| {
            fill(t.preview_trip, &[("km", &num(l, tr, 0)), ("date", &format::short_date(l, &p.date))])
        });
        let state_line = match (e.state, &e.seg) {
            (SegState::Ok, Some(sg)) => fill(
                t.preview_cons,
                &[("v", &num(l, sg.cons, 2)), ("km", &num(l, sg.dist, 0)), ("c", &money(l, &cur, sg.cost / sg.dist * 100.0, 2))],
            ),
            (SegState::Start, _) => t.state_start.to_string(),
            (SegState::Partial, _) => t.state_partial.to_string(),
            (SegState::NoStart, _) => t.state_nostart.to_string(),
            _ => t.state_invalid.to_string(),
        };
        let unit = e.unit_price.map(|p| fill(t.preview_unit_price, &[("v", &money(l, &cur, p, 3))]));
        Some(view! {
            <div class=if e.state == SegState::Invalid { "preview warn" } else { "preview" }>
                {trip_line.map(|s| view! { <div>{s}</div> })}
                <div>
                    <b>{state_line}</b>
                </div>
                {unit.map(|s| view! { <div class="muted">{s}</div> })}
            </div>
        })
    };

    let vehicles = store.vehicles();

    (move || {
        let t = store.t();
        let vehicle_field = (vehicles.len() > 1).then(|| {
            let current = vehicle.get_untracked();
            view! {
                <label class="field">
                    <span>{t.vehicle}</span>
                    <select on:change=move |ev| {
                        vehicle.set(event_target_value(&ev));
                        sync_trip();
                    }>
                        {vehicles
                            .iter()
                            .map(|v| {
                                view! {
                                    <option value=v.id.clone() selected=v.id == current>
                                        {v.name.clone()}
                                    </option>
                                }
                            })
                            .collect_view()}
                    </select>
                </label>
            }
        });
        let cur = store.currency();
        let on_station_change = on_station_change.clone();
        view! {
            <form class="card form" on:submit=submit.clone() novalidate>
                <h2>{if editing { t.edit_fillup } else { t.new_fillup }}</h2>
                {vehicle_field}
                <div class="grid2">
                    <label class="field">
                        <span>{t.date}</span>
                        <input
                            type="datetime-local"
                            required
                            prop:value=move || date.get()
                            on:input=move |ev| {
                                date.set(event_target_value(&ev));
                                sync_trip();
                            }
                        />
                    </label>
                    <label class="field">
                        <span>{t.odometer}" (km)"</span>
                        <input
                            inputmode="decimal"
                            autocomplete="off"
                            required
                            placeholder=move || {
                                neighbors.with(|(p, _)| p.as_ref().and_then(|p| p.odometer)).map(|o| format!("> {}", num(l, o, 0)))
                            }
                            prop:value=move || odo.get()
                            on:input=move |ev| {
                                odo.set(event_target_value(&ev));
                                sync_trip();
                            }
                        />
                    </label>
                </div>
                <label class="field">
                    <span>{t.trip}" (km)"</span>
                    <input
                        inputmode="decimal"
                        autocomplete="off"
                        placeholder=t.trip_hint
                        prop:value=move || trip.get()
                        on:input=move |ev| {
                            let v = event_target_value(&ev);
                            if let (Some(p), Some(tv)) = (prev_odo(), parse_num(&v)) {
                                odo.set(input_num(l, p + tv, 1));
                            }
                            trip.set(v);
                        }
                    />
                </label>
                <div class="grid3">
                    {trio_input(Trio::Liters, liters, format!("{} (L)", t.liters))}
                    {trio_input(Trio::Price, price, format!("{} ({cur}/L)", t.price))}
                    {trio_input(Trio::Total, total, format!("{} ({cur})", t.total))}
                </div>
                <p class="hint">{t.trio_hint}</p>
                <label class="field">
                    <span>{t.paid}" ("{cur}") "<em>{t.optional}</em></span>
                    <input
                        inputmode="decimal"
                        autocomplete="off"
                        placeholder=move || total.get()
                        prop:value=move || paid.get()
                        on:input=move |ev| paid.set(event_target_value(&ev))
                    />
                </label>
                <div class="toggles">
                    <label class="switch">
                        <input
                            type="checkbox"
                            prop:checked=move || full.get()
                            on:change=move |ev| full.set(event_target_checked(&ev))
                        />
                        <span>{t.full_tank}</span>
                    </label>
                    <label class="switch">
                        <input
                            type="checkbox"
                            prop:checked=move || missed.get()
                            on:change=move |ev| missed.set(event_target_checked(&ev))
                        />
                        <span>{t.missed_previous}</span>
                    </label>
                </div>
                <p class="hint">{t.full_hint}</p>
                {preview_view}
                <div class="grid2">
                    <label class="field">
                        <span>{t.fuel}</span>
                        <input
                            list="dl-fuel"
                            autocomplete="off"
                            prop:value=move || fuel.get()
                            on:input=move |ev| fuel.set(event_target_value(&ev))
                        />
                    </label>
                    <label class="field">
                        <span>{t.display_cons}" (L/100 km)"</span>
                        <input
                            inputmode="decimal"
                            autocomplete="off"
                            placeholder=t.optional
                            prop:value=move || display.get()
                            on:input=move |ev| display.set(event_target_value(&ev))
                        />
                    </label>
                </div>
                <label class="field">
                    <span>{t.station}</span>
                    <div class="input-row">
                        <input
                            list="dl-station"
                            autocomplete="off"
                            prop:value=move || station.get()
                            on:input=move |ev| station.set(event_target_value(&ev))
                            on:change=move |ev| on_station_change(event_target_value(&ev))
                        />
                        <button
                            type="button"
                            class="btn"
                            disabled=move || locating.get()
                            on:click=locate.clone()
                        >
                            {icon(Icon::Locate)}
                            {move || if locating.get() { store.t().locating } else { store.t().nearby }}
                        </button>
                    </div>
                </label>
                {picker}
                <label class="field">
                    <span>{t.address}</span>
                    <input
                        autocomplete="off"
                        prop:value=move || address.get()
                        on:input=move |ev| address.set(event_target_value(&ev))
                    />
                </label>
                {move || {
                    coords
                        .get()
                        .map(|(lat, lng)| {
                            view! {
                                <p class="hint coords">
                                    {icon(Icon::Pin)}
                                    {format!("{lat:.5}, {lng:.5}")}
                                    <button type="button" class="link-btn" on:click=move |_| coords.set(None)>
                                        {store.t().remove}
                                    </button>
                                </p>
                            }
                        })
                }}
                <label class="field">
                    <span>{t.payment}</span>
                    <input
                        list="dl-payment"
                        autocomplete="off"
                        prop:value=move || payment.get()
                        on:input=move |ev| payment.set(event_target_value(&ev))
                    />
                </label>
                <label class="field">
                    <span>{t.notes}</span>
                    <textarea
                        rows="2"
                        prop:value=move || notes.get()
                        on:input=move |ev| notes.set(event_target_value(&ev))
                    ></textarea>
                </label>
                {move || error.get().map(|e| view! { <p class="form-error" role="alert">{e}</p> })}
                <div class="btn-row">
                    <button type="submit" class="btn primary grow">{t.save}</button>
                    <a class="btn" href="#/records">{t.cancel}</a>
                </div>
                {editing.then(|| view! {
                    <button type="button" class="btn danger full" on:click=delete.clone()>{t.delete}</button>
                })}
            </form>
            {datalist("dl-station", stations.clone())}
            {datalist("dl-fuel", fuels.clone())}
            {datalist("dl-payment", payments.clone())}
        }
    })
    .into_any()
}
