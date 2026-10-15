use leptos::prelude::*;

use super::{EmptyState, Icon, icon};
use crate::app::Ui;
use crate::format::{self, money, num};
use crate::i18n::{Lang, Texts, fill};
use crate::model::MapProvider;
use crate::stats::{Entry, SegState};
use crate::store::Store;
use crate::web;

fn matches(e: &Entry, q: &str) -> bool {
    [&e.rec.station, &e.rec.address, &e.rec.fuel, &e.rec.notes, &e.rec.payment, &e.rec.date]
        .iter()
        .any(|s| s.to_lowercase().contains(q))
}

#[component]
pub fn Records() -> impl IntoView {
    let store = expect_context::<Store>();
    let ui = expect_context::<Ui>();
    let entries = Memo::new(move |_| store.entries_for(&store.active_vehicle()));

    let list = move || {
        let t = store.t();
        let l = store.lang();
        let cur = store.currency();
        let provider = store.settings.with(|s| s.map_provider);
        if entries.with(Vec::is_empty) {
            return view! { <EmptyState /> }.into_any();
        }
        let q = ui.search.get().trim().to_lowercase();
        let items: Vec<Entry> = entries
            .get()
            .into_iter()
            .rev()
            .filter(|e| q.is_empty() || matches(e, &q))
            .collect();
        if items.is_empty() {
            return view! { <p class="card muted">{t.no_matches}</p> }.into_any();
        }

        let mut groups: Vec<(String, Vec<Entry>)> = Vec::new();
        for e in items.iter().cloned() {
            let key = e.rec.date.get(..7).unwrap_or_default().to_string();
            match groups.last_mut() {
                Some((k, v)) if *k == key => v.push(e),
                _ => groups.push((key, vec![e])),
            }
        }

        view! {
            <p class="muted count">{fill(t.n_records, &[("n", &items.len())])}</p>
            {groups
                .into_iter()
                .map(|(key, group)| {
                    let cost: f64 = group.iter().map(|e| e.cost).sum();
                    let liters: f64 = group.iter().filter_map(|e| e.rec.liters).sum();
                    view! {
                        <section class="month">
                            <h3 class="month-head">
                                <span>{format::month(l, &key)}</span>
                                <span class="muted">
                                    {money(l, &cur, cost, 2)}" · "{num(l, liters, 1)}" L"
                                </span>
                            </h3>
                            <div class="rec-list">
                                {group
                                    .into_iter()
                                    .map(|e| record_card(e, t, l, &cur, provider))
                                    .collect_view()}
                            </div>
                        </section>
                    }
                })
                .collect_view()}
        }
        .into_any()
    };

    view! {
        <div class="toolbar">
            <input
                type="search"
                class="search"
                placeholder=move || store.t().search_placeholder
                prop:value=move || ui.search.get()
                on:input=move |ev| ui.search.set(event_target_value(&ev))
            />
        </div>
        {list}
    }
}

fn record_card(e: Entry, t: &'static Texts, l: Lang, cur: &str, provider: MapProvider) -> impl IntoView {
    let r = &e.rec;
    let href = format!("#/edit/{}", r.id);
    let (cons_class, cons_text) = match (e.state, &e.seg) {
        (SegState::Ok, Some(s)) => ("cons ok", format!("{} L/100 km", num(l, s.cons, 2))),
        (SegState::Start, _) => ("cons", t.state_start.to_string()),
        (SegState::Partial, _) => ("cons", t.state_partial.to_string()),
        (SegState::NoStart, _) => ("cons", t.state_nostart.to_string()),
        _ => ("cons warn", t.state_invalid.to_string()),
    };
    let price = r.price.or(e.unit_price);
    let station = if r.station.is_empty() { t.unspecified.to_string() } else { r.station.clone() };
    let saved = match (r.total, r.paid) {
        (Some(total), Some(paid)) if total > paid => Some(money(l, cur, total - paid, 2)),
        _ => None,
    };
    let map = provider.url(r);

    view! {
        <article
            class="rec card"
            tabindex="0"
            role="button"
            on:click={
                let href = href.clone();
                move |_| web::navigate(&href)
            }
            on:keydown=move |ev| {
                if ev.key() == "Enter" {
                    web::navigate(&href)
                }
            }
        >
            <div class="rec-row">
                <span class="rec-date">{format::date(l, &r.date, true)}</span>
                <span class="rec-cost">{money(l, cur, e.cost, 2)}</span>
            </div>
            <div class="rec-row">
                <span class="rec-station">
                    {station}
                    {(!r.fuel.is_empty()).then(|| view! { <span class="badge">{r.fuel.clone()}</span> })}
                </span>
                <span class="muted nowrap">
                    {r.liters.map(|v| num(l, v, 2))}" L × "{price.map(|p| money(l, cur, p, 3))}
                </span>
            </div>
            <div class="rec-row">
                <span class="muted">
                    {r.odometer.map(|o| num(l, o, 0))}" km"
                    {e.trip.filter(|t| *t > 0.0).map(|t| format!(" · +{} km", num(l, t, 0)))}
                </span>
                <span class=cons_class>{cons_text}</span>
            </div>
            <div class="rec-tags">
                {(!r.full).then(|| view! { <span class="badge warn">{t.badge_partial}</span> })}
                {r.missed_previous.then(|| view! { <span class="badge warn">{t.badge_missed}</span> })}
                {r.display_cons
                    .map(|d| view! { <span class="badge">{fill(t.badge_display, &[("v", &num(l, d, 1))])}</span> })}
                {saved.map(|s| view! { <span class="badge good">{fill(t.badge_saved, &[("v", &s)])}</span> })}
                {r.demo.then(|| view! { <span class="badge">{t.badge_demo}</span> })}
                {map.map(|url| {
                    view! {
                        <a
                            class="badge link"
                            href=url
                            target="_blank"
                            rel="noopener"
                            on:click=|ev| ev.stop_propagation()
                        >
                            {icon(Icon::Pin)}
                            {t.map}
                        </a>
                    }
                })}
            </div>
        </article>
    }
}
