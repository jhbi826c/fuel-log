use std::collections::HashMap;

use leptos::prelude::*;
use leptos::task::spawn_local;

use super::{Icon, icon};
use crate::app::Ui;
use crate::format::{self, input_num, money, num, parse_num};
use crate::i18n::fill;
use crate::model::{FUEL_TYPES, Record, is_known_fuel, usable_fuels};
use crate::places::geo::distance_km;
use crate::places::{self, Station};
use crate::stats::{SegState, analyze, parse_dt, sort_records};
use crate::store::Store;
use crate::web;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trio {
    Liters,
    Price,
    Total,
}

const NEARBY_LIMIT: usize = 10;

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

/// Applies the station names from your records to search results.
fn with_own_names(mut list: Vec<Station>, all: &[Record]) -> Vec<Station> {
    let known: Vec<places::Known> = all
        .iter()
        .filter_map(|r| {
            let (lat, lng) = r.coords()?;
            Some(places::Known { lat, lng, name: &r.station, address: &r.address, date: &r.date })
        })
        .collect();
    places::apply_own_names(&mut list, &known);
    list
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
    let history = StoredValue::new(all.clone());
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
        fuel: last_of_vehicle
            .as_ref()
            .map(|r| r.fuel.clone())
            .filter(|f| !f.is_empty())
            .or_else(|| store.vehicle_fuels(&active).into_iter().next())
            .unwrap_or_default(),
        ..Default::default()
    });
    let rec_id = base.id.clone();
    let created_at = base.created_at;
    let demo = base.demo;
    let s = |v: Option<f64>, d: usize| v.map(|v| input_num(l, v, d)).unwrap_or_default();

    let vehicle = RwSignal::new(base.vehicle_id.clone());
    let date = RwSignal::new(base.date.clone());
    let odo = RwSignal::new(s(base.odometer, 1));
    let trip = RwSignal::new(s(base.trip, 1));
    let trip_explicit = RwSignal::new(base.trip.is_some());
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
    let notes = RwSignal::new(base.notes.clone());
    let order = RwSignal::new(if editing { vec![Trio::Total, Trio::Liters, Trio::Price] } else { Vec::new() });
    let computed = RwSignal::new(None::<Trio>);
    let locating = RwSignal::new(false);
    let searching = RwSignal::new(false);
    let nearby_place = RwSignal::new(None::<String>);
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
        if trip_explicit.get_untracked() {
            if let (Some(p), Some(t)) = (prev_odo(), parse_num(&trip.get_untracked())) {
                odo.set(input_num(l, p + t, 1));
            }
            return;
        }
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
                odometer: parse_num(&odo.get()),
                trip: if trip_explicit.get() { parse_num(&trip.get()) } else { None },
                liters: Some(parse_num(&liters.get())?),
                price: parse_num(&price.get()),
                total: parse_num(&total.get()),
                paid: parse_num(&paid.get()),
                full: full.get(),
                missed_previous: missed.get(),
                ..Default::default()
            };
            if cand.odometer.is_none() && cand.trip.is_none() {
                return None;
            }
            let mut rs = others.get();
            rs.push(cand);
            analyze(&rs).into_iter().find(|e| e.rec.id == rec_id)
        })
    };

    let suggestions = |f: fn(&Record) -> &str| by_frequency(all.iter().map(f));
    let stations = suggestions(|r| r.station.as_str());
    let history_fuels: Vec<String> = suggestions(|r| r.fuel.as_str()).into_iter().filter(|f| !is_known_fuel(f)).take(4).collect();
    let base_fuel = base.fuel.clone();
    let fuel_options = Memo::new(move |_| {
        let mine = store.data.with(|d| {
            d.vehicles.iter().find(|v| v.id == vehicle.get()).map(|v| v.fuels.clone()).unwrap_or_default()
        });
        let usable = usable_fuels(&mine);
        let mut opts: Vec<String> = FUEL_TYPES
            .iter()
            .map(|(name, _)| name.to_string())
            .filter(|n| mine.is_empty() || usable.contains(&n.as_str()) || *n == base_fuel)
            .collect();
        opts.extend(history_fuels.iter().filter(|f| !opts.contains(f)).cloned().collect::<Vec<_>>());
        opts
    });
    let custom_fuel = RwSignal::new(!base.fuel.is_empty() && !fuel_options.get_untracked().contains(&base.fuel));
    let station_fuels = RwSignal::new(Vec::<&'static str>::new());

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

    let nearby = RwSignal::new(None::<(f64, Vec<Station>)>);

    let apply_station = move |st: &Station| {
        let label = st.label();
        station.set(if label.is_empty() { store.t().unnamed_station.to_string() } else { label });
        if !st.address.is_empty() {
            address.set(st.address.clone());
        }
        coords.set(Some((st.lat, st.lng)));
        station_fuels.set(st.fuels.clone());
        if !st.fuels.is_empty() && !fuel.with_untracked(|f| st.fuels.contains(&f.as_str())) {
            let usable = usable_fuels(&store.vehicle_fuels(&vehicle.get_untracked()));
            if let Some(f) = usable.into_iter().find(|f| st.fuels.contains(f)) {
                fuel.set(f.to_string());
                custom_fuel.set(false);
            }
        }
        nearby.set(None);
    };

    if !editing && let Some(p) = ui.prefill.get_untracked() {
        ui.prefill.set(None);
        apply_station(&p.station);
        fuel.set(p.fuel.to_string());
        custom_fuel.set(!fuel_options.get_untracked().iter().any(|f| f == p.fuel));
        if let Some(v) = p.price.filter(|_| store.settings.with_untracked(|s| s.currency == "€")) {
            on_trio(Trio::Price, input_num(l, v, 3));
        }
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
                let wanted = store.vehicle_fuels(&vehicle.get_untracked());
                let (addr, found) = futures::join!(places::reverse_geocode(c), places::nearby(c, NEARBY_LIMIT, &wanted));
                if address.get_untracked().is_empty()
                    && let Ok(Some(a)) = addr
                {
                    address.set(a);
                }
                let known = all
                    .iter()
                    .filter(|r| !r.station.is_empty())
                    .filter_map(|r| Some((distance_km(c, r.coords()?), r)))
                    .filter(|(d, _)| *d < places::SAME_STATION_KM)
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
                match found {
                    Ok(f) => {
                        nearby_place.set(None);
                        nearby.set(Some((f.0, history.with_value(|all| with_own_names(f.1, all)))));
                    }
                    Err(e) => ui.error(format!("{} ({e})", t.nearby_failed)),
                }
                locating.set(false);
            });
        }
    };

    let search_here = move || {
        let q = address.get_untracked().trim().to_string();
        if q.is_empty() {
            ui.error(store.t().enter_address);
            return;
        }
        searching.set(true);
        spawn_local(async move {
            let t = store.t();
            let wanted = store.vehicle_fuels(&vehicle.get_untracked());
            match places::geocode(&q).await {
                Ok(Some(point)) => match places::nearby(point, NEARBY_LIMIT, &wanted).await {
                    Ok(found) => {
                        nearby_place.set(Some(q.clone()));
                        nearby.set(Some((found.0, history.with_value(|all| with_own_names(found.1, all)))));
                    }
                    Err(e) => ui.error(format!("{} ({e})", t.nearby_failed)),
                },
                Ok(None) => ui.error(t.place_not_found),
                Err(e) => ui.error(format!("{} ({e})", t.place_not_found)),
            }
            searching.set(false);
        });
    };

    let picker = move || {
        let (km, list) = nearby.get()?;
        let t = store.t();
        if list.is_empty() {
            let text = match nearby_place.get() {
                Some(place) => fill(t.no_nearby_near, &[("km", &km), ("place", &place)]),
                None => fill(t.no_nearby, &[("km", &km)]),
            };
            return Some(view! { <p class="hint">{text}</p> }.into_any());
        }
        Some(
            view! {
                <div class="picker">
                    <p class="hint">
                        {match nearby_place.get() {
                            Some(place) => fill(t.pick_station_near, &[("km", &km), ("place", &place)]),
                            None => fill(t.pick_station, &[("km", &km)]),
                        }}
                    </p>
                    {list
                        .into_iter()
                        .map(|st| {
                            let title = st.title();
                            let title = if title.is_empty() { t.unnamed_station.to_string() } else { title };
                            let dist = format!("{} km", num(l, st.dist_km, 1));
                            let pick = st.clone();
                            view! {
                                <button type="button" class="pick" on:click=move |_| apply_station(&pick)>
                                    <span class="pick-main">
                                        <b>
                                            {title}
                                            {match (&st.own_name, st.renamed_from) {
                                                (Some(_), _) => Some(view! { <small class="badge good own">{t.own_name_badge}</small> }.into_any()),
                                                (None, Some(o)) => Some(view! { <small class="muted renamed">" ("{o}")"</small> }.into_any()),
                                                (None, None) => None,
                                            }}
                                        </b>
                                        <small class="muted">{st.address.clone()}</small>
                                        {if st.fuels.is_empty() {
                                            (!st.fuel_tagged).then(|| view! { <small class="muted">{t.fuels_unknown}</small> }.into_any())
                                        } else {
                                            Some(view! { <small class="fuels">{st.fuels.join(" · ")}</small> }.into_any())
                                        }}
                                    </span>
                                    <span class="pick-side">
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
        let o = parse_num(&odo.get_untracked()).filter(|v| *v >= 0.0);
        let tr = if trip_explicit.get_untracked() { parse_num(&trip.get_untracked()).filter(|v| *v > 0.0) } else { None };
        if o.is_none() && tr.is_none() {
            return fail(t.err_odometer);
        }
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
        if let (Some(o), Some(p)) = (o, prev.and_then(|p| p.odometer))
            && o <= p
            && !web::confirm(&fill(t.confirm_odo_lower, &[("km", &num(l, p, 0))]))
        {
            return;
        }
        if let (Some(o), Some(n)) = (o, next.and_then(|n| n.odometer))
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
            odometer: o,
            trip: tr,
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
                            placeholder=move || {
                                neighbors.with(|(p, _)| p.as_ref().and_then(|p| p.odometer)).map(|o| format!("> {}", num(l, o, 0)))
                            }
                            prop:value=move || odo.get()
                            on:input=move |ev| {
                                odo.set(event_target_value(&ev));
                                trip_explicit.set(false);
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
                            trip_explicit.set(!v.trim().is_empty());
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
                <label class="field">
                    <span>{t.station}</span>
                    <div class="input-row">
                        <input
                            list="dl-station"
                            autocomplete="off"
                            prop:value=move || station.get()
                            on:input=move |ev| {
                                station.set(event_target_value(&ev));
                                station_fuels.set(Vec::new());
                            }
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
                    <div class="input-row">
                        <input
                            autocomplete="off"
                            prop:value=move || address.get()
                            on:input=move |ev| address.set(event_target_value(&ev))
                            on:keydown=move |ev| {
                                if ev.key() == "Enter" {
                                    ev.prevent_default();
                                    search_here();
                                }
                            }
                        />
                        <button
                            type="button"
                            class="btn"
                            disabled=move || searching.get()
                            on:click=move |_| search_here()
                        >
                            {icon(Icon::Search)}
                            {move || if searching.get() { store.t().searching } else { store.t().search_here }}
                        </button>
                    </div>
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
                <div class="field">
                    <span>{t.fuel}</span>
                    <div class="chips wrap">
                        {move || fuel_options.get()
                            .iter()
                            .map(|f| {
                                let (f_class, f_click, label) = (f.clone(), f.clone(), f.clone());
                                view! {
                                    <button
                                        type="button"
                                        class=move || {
                                            let known = station_fuels.with(|s| s.is_empty() || s.contains(&f_class.as_str()));
                                            match (fuel.with(|v| *v == f_class) && !custom_fuel.get(), known) {
                                                (true, _) => "chip active",
                                                (false, false) => "chip dim",
                                                (false, true) => "chip",
                                            }
                                        }
                                        on:click=move |_| {
                                            fuel.set(f_click.clone());
                                            custom_fuel.set(false);
                                        }
                                    >
                                        {label}
                                    </button>
                                }
                            })
                            .collect_view()}
                        <button
                            type="button"
                            class=move || if custom_fuel.get() { "chip active" } else { "chip" }
                            on:click=move |_| {
                                custom_fuel.set(true);
                                fuel.set(String::new());
                            }
                        >
                            {t.other_fuel}
                        </button>
                    </div>
                    {move || {
                        custom_fuel
                            .get()
                            .then(|| {
                                view! {
                                    <input
                                        autocomplete="off"
                                        placeholder=store.t().other_fuel
                                        prop:value=move || fuel.get()
                                        on:input=move |ev| fuel.set(event_target_value(&ev))
                                    />
                                }
                            })
                    }}
                    {move || {
                        let f = fuel.get();
                        let listed = station_fuels.with(|s| s.is_empty() || s.contains(&f.as_str()));
                        (!listed && is_known_fuel(&f))
                            .then(|| view! { <p class="hint warn">{fill(store.t().fuel_not_listed, &[("fuel", &f)])}</p> })
                    }}
                </div>
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
        }
    })
    .into_any()
}
