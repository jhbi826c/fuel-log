use chrono::NaiveDate;
use leptos::prelude::*;

use super::{EmptyState, Icon, icon};
use crate::app::{Period, Ui};
use crate::charts::{Bar, BarChart, LineChart};
use crate::format::{self, money, money_opt, num, opt};
use crate::i18n::{Lang, fill};
use crate::stats::{Stats, compute};
use crate::store::Store;
use crate::web;

#[component]
pub fn Overview() -> impl IntoView {
    let store = expect_context::<Store>();
    let ui = expect_context::<Ui>();
    let entries = Memo::new(move |_| store.entries_for(&store.active_vehicle()));
    let years = Memo::new(move |_| {
        let mut ys: Vec<i32> = entries.with(|e| e.iter().filter_map(|e| e.rec.date.get(..4)?.parse().ok()).collect());
        ys.sort_unstable();
        ys.dedup();
        ys.reverse();
        ys
    });
    let today = web::now_local();
    let stats = Memo::new(move |_| {
        let range = ui.period.get().range(&today);
        entries.with(|e| compute(e, &range))
    });

    move || {
        let t = store.t();
        if entries.with(Vec::is_empty) {
            return view! { <EmptyState /> }.into_any();
        }
        let mut periods = vec![(Period::All, t.period_all.to_string()), (Period::Months(3), t.period_3m.to_string()), (Period::Months(12), t.period_12m.to_string())];
        periods.extend(years.get().into_iter().map(|y| (Period::Year(y), y.to_string())));
        let is_custom = move || matches!(ui.period.get(), Period::Custom(..));
        view! {
            <div class="period-bar">
                <div class="chips" role="tablist">
                    {periods
                        .into_iter()
                        .map(|(p, label)| {
                            view! {
                                <button
                                    class=move || if ui.period.get() == p { "chip active" } else { "chip" }
                                    on:click=move |_| ui.period.set(p)
                                >
                                    {label}
                                </button>
                            }
                        })
                        .collect_view()}
                </div>
                <button
                    class=move || if is_custom() { "chip custom-chip active" } else { "chip custom-chip" }
                    aria-pressed=move || is_custom().to_string()
                    on:click=move |_| {
                        if !is_custom() {
                            ui.period.set(Period::Custom(None, None));
                        }
                    }
                >
                    {icon(Icon::Calendar)}
                    <span>{t.period_custom}</span>
                </button>
            </div>
            {move || is_custom().then(|| view! { <CustomRange /> })}
            <StatsView stats=stats.get() />
        }
        .into_any()
    }
}

/// From/to date pickers for `Period::Custom`.
#[component]
fn CustomRange() -> impl IntoView {
    let store = expect_context::<Store>();
    let ui = expect_context::<Ui>();
    let t = store.t();
    let dates = move || match ui.period.get() {
        Period::Custom(f, to) => (f, to),
        _ => (None, None),
    };
    let fmt = |d: Option<NaiveDate>| d.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
    let parse = |s: String| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok();
    view! {
        <div class="card custom-range">
            <label class="field">
                <span>{t.range_from}</span>
                <input
                    type="date"
                    prop:value=move || fmt(dates().0)
                    max=move || fmt(dates().1)
                    on:input=move |ev| ui.period.set(Period::Custom(parse(event_target_value(&ev)), dates().1))
                />
            </label>
            <label class="field">
                <span>{t.range_to}</span>
                <input
                    type="date"
                    prop:value=move || fmt(dates().1)
                    min=move || fmt(dates().0)
                    on:input=move |ev| ui.period.set(Period::Custom(dates().0, parse(event_target_value(&ev))))
                />
            </label>
        </div>
    }
}

fn kpi(label: &'static str, value: String, sub: Option<String>) -> impl IntoView {
    view! {
        <div class="kpi">
            <div class="label">{label}</div>
            <div class="value">{value}</div>
            {sub.map(|s| view! { <div class="sub">{s}</div> })}
        </div>
    }
}

fn group_name(name: &str, fallback: &'static str) -> String {
    if name.is_empty() { fallback.to_string() } else { name.to_string() }
}

#[component]
fn StatsView(stats: Stats) -> impl IntoView {
    let store = expect_context::<Store>();
    let t = store.t();
    let l: Lang = store.lang();
    let cur = store.currency();
    let s = stats;
    if s.count == 0 {
        return view! { <div class="card empty-period muted">{t.no_records_in_period}</div> }.into_any();
    }
    let m = |v: Option<f64>, d: usize| money_opt(l, &cur, v, d);

    let cons_sub = if s.avg_cons.is_some() {
        fill(t.based_on_segments, &[("n", &s.seg_count), ("km", &num(l, s.seg_dist, 0))])
    } else {
        t.need_two_full.to_string()
    };

    let mut kpis = vec![
        kpi(t.total_spent, money(l, &cur, s.total_cost, 2), Some(fill(t.n_fillups, &[("n", &s.count)]))),
        kpi(
            t.total_fuel,
            format!("{} L", num(l, s.total_liters, 1)),
            s.avg_liters_per_fill.map(|v| fill(t.per_fillup_liters, &[("v", &num(l, v, 1))])),
        ),
        kpi(
            t.distance,
            format!("{} km", num(l, s.distance, 0)),
            s.km_per_month.map(|v| fill(t.per_month_km, &[("v", &num(l, v, 0))])),
        ),
        kpi(
            t.monthly_cost,
            m(s.cost_per_month, 2),
            s.cost_per_month.map(|v| fill(t.per_year_money, &[("v", &money(l, &cur, v * 12.0, 0))])),
        ),
        kpi(
            t.fill_interval,
            s.avg_interval_days.map_or(format::DASH.into(), |d| fill(t.n_days, &[("n", &num(l, d, 1))])),
            s.avg_interval_km.map(|v| format!("∅ {} km", num(l, v, 0))),
        ),
        kpi(t.per_fillup, m(s.avg_cost_per_fill, 2), None),
        kpi(
            t.last_consumption,
            s.last_cons.map_or(format::DASH.into(), |v| format!("{} L/100 km", num(l, v, 2))),
            s.best_cons.zip(s.worst_cons).map(|(b, w)| {
                fill(t.best_worst, &[("best", &num(l, b, 1)), ("worst", &num(l, w, 1))])
            }),
        ),
        kpi(
            t.price_range,
            match (&s.min_price, &s.max_price) {
                (Some(a), Some(b)) => format!("{}–{}", num(l, a.value, 3), m(Some(b.value), 3)),
                _ => format::DASH.into(),
            },
            s.min_price.as_ref().map(|p| {
                let station = if p.station.is_empty() { String::new() } else { format!(" · {}", p.station) };
                format!("{} {}{}", t.cheapest, format::short_date(l, &p.date), station)
            }),
        ),
    ];
    if s.savings > 0.0 {
        kpis.push(kpi(t.savings, money(l, &cur, s.savings, 2), Some(t.savings_sub.to_string())));
    }

    let bars: Vec<Bar> = s
        .monthly
        .iter()
        .skip(s.monthly.len().saturating_sub(24))
        .map(|mo| Bar {
            label: format::month_axis(l, &mo.key),
            value: mo.cost,
            tip_title: format::month(l, &mo.key),
            tip_lines: vec![
                money(l, &cur, mo.cost, 2),
                format!("{} L · {}", num(l, mo.liters, 1), fill(t.n_fillups, &[("n", &mo.count)])),
            ],
        })
        .collect();

    let yearly = (s.yearly.len() > 1).then(|| {
        view! {
            <section class="card half">
                <h3>{t.yearly}</h3>
                <div class="table-wrap">
                    <table>
                        <thead>
                            <tr>
                                <th>{t.col_year}</th>
                                <th class="r">{t.col_fillups}</th>
                                <th class="r">{t.col_spent}</th>
                                <th class="r">"km"</th>
                                <th class="r">"L/100 km"</th>
                                <th class="r">{t.col_per_100km}</th>
                            </tr>
                        </thead>
                        <tbody>
                            {s
                                .yearly
                                .iter()
                                .map(|y| {
                                    view! {
                                        <tr>
                                            <td>{y.year}</td>
                                            <td class="r">{y.count}</td>
                                            <td class="r">{money(l, &cur, y.cost, 0)}</td>
                                            <td class="r">{num(l, y.dist, 0)}</td>
                                            <td class="r">{opt(l, y.cons, 2)}</td>
                                            <td class="r">{m(y.cost_per_100km, 2)}</td>
                                        </tr>
                                    }
                                })
                                .collect_view()}
                        </tbody>
                    </table>
                </div>
            </section>
        }
    });

    let group_table = |title: &'static str, first_col: &'static str, rows: &[crate::stats::GroupAgg], fallback: &'static str| {
        view! {
            <section class="card half">
                <h3>{title}</h3>
                <div class="table-wrap">
                    <table>
                        <thead>
                            <tr>
                                <th>{first_col}</th>
                                <th class="r">{t.col_fillups}</th>
                                <th class="r">{t.col_avg_price}</th>
                                <th class="r">{t.col_spent}</th>
                                <th class="r">"L/100 km"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {rows
                                .iter()
                                .map(|g| {
                                    view! {
                                        <tr>
                                            <td class="name">{group_name(&g.name, fallback)}</td>
                                            <td class="r">{g.count}</td>
                                            <td class="r">{m(g.avg_price, 3)}</td>
                                            <td class="r">{money(l, &cur, g.cost, 0)}</td>
                                            <td class="r">{opt(l, g.cons, 2)}</td>
                                        </tr>
                                    }
                                })
                                .collect_view()}
                        </tbody>
                    </table>
                </div>
            </section>
        }
    };
    let stations = group_table(t.stations, t.col_station, &s.stations, t.unspecified);
    let fuels = (s.fuels.len() > 1 || s.fuels.first().is_some_and(|f| !f.name.is_empty()))
        .then(|| group_table(t.fuel_types, t.col_fuel, &s.fuels, t.unspecified));

    view! {
        <section class="hero card">
            <div class="hero-main">
                <div class="label">{t.avg_consumption}</div>
                <div class="big">
                    {opt(l, s.avg_cons, 2)} <small>" L/100 km"</small>
                </div>
                <div class="sub">{cons_sub}</div>
            </div>
            <div class="hero-side">
                <div>
                    <div class="label">{t.avg_price}</div>
                    <div class="mid">{m(s.avg_price, 3)} <small>" /L"</small></div>
                </div>
                <div>
                    <div class="label">{t.cost_per_100km}</div>
                    <div class="mid">{m(s.cost_per_100km, 2)} <small>" /100 km"</small></div>
                </div>
            </div>
        </section>
        <section class="kpis">{kpis.into_iter().collect_view()}</section>
        <section class="card half">
            <h3>{t.chart_consumption}</h3>
            <LineChart points=s.cons_series.clone() decimals=2 unit="L/100 km".to_string() avg=s.avg_cons />
        </section>
        <section class="card half">
            <h3>{t.chart_price}</h3>
            <LineChart points=s.price_series.clone() decimals=3 unit=format!("{cur}/L") avg=s.avg_price />
        </section>
        <section class="card half">
            <h3>{t.chart_monthly}</h3>
            <BarChart bars=bars />
        </section>
        {yearly}
        {stations}
        {fuels}
    }
    .into_any()
}
