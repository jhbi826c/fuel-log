use leptos::prelude::*;
use leptos::html::Div;

use crate::format;
use crate::stats::Point;
use crate::store::Store;

/// Rounded axis bounds and tick values.
pub fn nice_scale(min: f64, max: f64, n: usize) -> (f64, f64, Vec<f64>, usize) {
    let (mut min, mut max) = (min, max);
    if (max - min).abs() < f64::EPSILON {
        let d = if min.abs() > 0.0 { min.abs() * 0.1 } else { 1.0 };
        min -= d;
        max += d;
    }
    let raw = (max - min) / n.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = mag
        * if norm <= 1.0 {
            1.0
        } else if norm <= 2.0 {
            2.0
        } else if norm <= 2.5 {
            2.5
        } else if norm <= 5.0 {
            5.0
        } else {
            10.0
        };
    let lo = (min / step).floor() * step;
    let hi = (max / step).ceil() * step;
    let mut ticks = Vec::new();
    let mut v = lo;
    while v <= hi + step / 2.0 {
        ticks.push((v / step).round() * step);
        v += step;
    }
    let decimals = if step >= 1.0 { 0 } else { (-step.log10().floor()) as usize + usize::from(norm > 2.0 && norm <= 2.5) };
    (lo, hi, ticks, decimals.min(4))
}

/// Horizontal pointer position within the plot, 0..1.
fn pct_from_pointer(plot: NodeRef<Div>, ev: &leptos::ev::PointerEvent) -> Option<f64> {
    let rect = plot.get()?.get_bounding_client_rect();
    (rect.width() > 0.0).then(|| ((ev.client_x() as f64 - rect.left()) / rect.width()).clamp(0.0, 1.0))
}

fn edge_class(base: &str, pct: f64) -> String {
    if pct < 12.0 {
        format!("{base} start")
    } else if pct > 88.0 {
        format!("{base} end")
    } else {
        base.to_string()
    }
}

fn y_axis(ticks: &[f64], lo: f64, hi: f64, decimals: usize, lang: crate::i18n::Lang) -> impl IntoView {
    ticks
        .iter()
        .map(|v| {
            let top = 100.0 - (v - lo) / (hi - lo) * 100.0;
            view! {
                <span class="ylab" style=format!("top:{top:.2}%")>
                    {format::num(lang, *v, decimals)}
                </span>
            }
        })
        .collect_view()
}

fn grid_path(ticks: &[f64], lo: f64, hi: f64) -> String {
    ticks
        .iter()
        .map(|v| format!("M0,{:.1}H1000", 1000.0 - (v - lo) / (hi - lo) * 1000.0))
        .collect()
}

/// Time series line chart.
#[component]
pub fn LineChart(points: Vec<Point>, decimals: usize, unit: String, avg: Option<f64>) -> impl IntoView {
    let store = expect_context::<Store>();
    let lang = store.lang();
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return view! { <div class="chart-empty">{store.t().not_enough_data}</div> }.into_any();
    };
    if points.len() < 2 {
        return view! { <div class="chart-empty">{store.t().not_enough_data}</div> }.into_any();
    }

    let (mut x0, mut x1) = (first.x, last.x);
    if x1 - x0 < 1.0 {
        x0 -= 1.0;
        x1 += 1.0;
    }
    let ys = points.iter().map(|p| p.y).chain(avg);
    let (ymin, ymax) = ys.fold((f64::MAX, f64::MIN), |(a, b), y| (a.min(y), b.max(y)));
    let (lo, hi, ticks, tick_dec) = nice_scale(ymin, ymax, 4);
    let px = move |x: f64| (x - x0) / (x1 - x0) * 1000.0;
    let py = move |y: f64| 1000.0 - (y - lo) / (hi - lo) * 1000.0;

    let line: String = points
        .iter()
        .enumerate()
        .map(|(i, p)| format!("{}{:.1},{:.1}", if i == 0 { 'M' } else { 'L' }, px(p.x), py(p.y)))
        .collect();
    let area = format!("{line}L{:.1},1000L{:.1},1000Z", px(last.x), px(first.x));
    let dots: String = if points.len() <= 80 {
        points.iter().map(|p| format!("M{:.1},{:.1}h0", px(p.x), py(p.y))).collect()
    } else {
        String::new()
    };
    let avg_path = avg
        .map(|a| format!(r#"<path class="avg" d="M0,{:.1}H1000"/>"#, py(a)))
        .unwrap_or_default();
    let svg = format!(
        r#"<svg viewBox="0 0 1000 1000" preserveAspectRatio="none" aria-hidden="true"><path class="grid" d="{}"/><path class="area" d="{area}"/>{avg_path}<path class="line" d="{line}"/><path class="dot" d="{dots}"/></svg>"#,
        grid_path(&ticks, lo, hi)
    );

    let n = points.len();
    let mut label_idx: Vec<usize> = if n <= 4 { (0..n).collect() } else { vec![0, n / 3, 2 * n / 3, n - 1] };
    label_idx.dedup();
    let x_labels = label_idx
        .into_iter()
        .map(|i| {
            let left = px(points[i].x) / 10.0;
            view! {
                <span class=edge_class("xlab", left) style=format!("left:{left:.2}%")>
                    {format::axis_date(lang, &points[i].date)}
                </span>
            }
        })
        .collect_view();

    let xs: Vec<f64> = points.iter().map(|p| px(p.x)).collect();
    let hover = RwSignal::new(None::<usize>);
    let plot = NodeRef::<Div>::new();
    let on_move = move |ev: leptos::ev::PointerEvent| {
        if let Some(f) = pct_from_pointer(plot, &ev) {
            let target = f * 1000.0;
            let i = xs
                .iter()
                .enumerate()
                .min_by(|a, b| (a.1 - target).abs().total_cmp(&(b.1 - target).abs()))
                .map(|(i, _)| i);
            hover.set(i);
        }
    };

    let tip = move || {
        hover.get().map(|i| {
            let p = &points[i];
            let left = px(p.x) / 10.0;
            let top = py(p.y) / 10.0;
            view! {
                <div class="hdot" style=format!("left:{left:.2}%;top:{top:.2}%")></div>
                <div class=edge_class("tip", left) style=format!("left:{left:.2}%;top:{top:.2}%")>
                    <b>{format::num(lang, p.y, decimals)}" "{unit.clone()}</b>
                    <span>{format::short_date(lang, &p.date)}</span>
                </div>
            }
        })
    };

    view! {
        <div class="chart">
            <div class="yaxis">{y_axis(&ticks, lo, hi, tick_dec, lang)}</div>
            <div
                class="plot"
                node_ref=plot
                on:pointermove=on_move.clone()
                on:pointerdown=on_move
                on:pointerleave=move |_| hover.set(None)
            >
                <div class="svgwrap" inner_html=svg></div>
                {tip}
            </div>
            <div class="xaxis">{x_labels}</div>
        </div>
    }
    .into_any()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub label: String,
    pub value: f64,
    pub tip_title: String,
    pub tip_lines: Vec<String>,
}

/// Bar chart with evenly spaced bars.
#[component]
pub fn BarChart(bars: Vec<Bar>) -> impl IntoView {
    let store = expect_context::<Store>();
    let lang = store.lang();
    if bars.is_empty() {
        return view! { <div class="chart-empty">{store.t().not_enough_data}</div> }.into_any();
    }
    let n = bars.len();
    let max = bars.iter().map(|b| b.value).fold(0.0, f64::max);
    let (lo, hi, ticks, tick_dec) = nice_scale(0.0, max.max(1.0), 4);
    let svg = format!(
        r#"<svg viewBox="0 0 1000 1000" preserveAspectRatio="none" aria-hidden="true"><path class="grid" d="{}"/></svg>"#,
        grid_path(&ticks, lo, hi)
    );
    let every = n.div_ceil(8);
    let hover = RwSignal::new(None::<usize>);
    let plot = NodeRef::<Div>::new();

    let columns = bars
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let h = (b.value - lo) / (hi - lo) * 100.0;
            view! {
                <div class="col">
                    <div
                        class=move || if hover.get() == Some(i) { "bar on" } else { "bar" }
                        style=format!("height:{h:.2}%")
                    ></div>
                </div>
            }
        })
        .collect_view();
    let x_labels = bars
        .iter()
        .enumerate()
        .filter(|(i, _)| (n - 1 - i).is_multiple_of(every))
        .map(|(i, b)| {
            let left = (i as f64 + 0.5) / n as f64 * 100.0;
            view! {
                <span class=edge_class("xlab", left) style=format!("left:{left:.2}%")>
                    {b.label.clone()}
                </span>
            }
        })
        .collect_view();

    let on_move = move |ev: leptos::ev::PointerEvent| {
        if let Some(f) = pct_from_pointer(plot, &ev) {
            hover.set(Some(((f * n as f64) as usize).min(n - 1)));
        }
    };
    let tip = move || {
        hover.get().map(|i| {
            let b = &bars[i];
            let left = (i as f64 + 0.5) / n as f64 * 100.0;
            let top = 100.0 - (b.value - lo) / (hi - lo) * 100.0;
            view! {
                <div class=edge_class("tip", left) style=format!("left:{left:.2}%;top:{top:.2}%")>
                    <b>{b.tip_title.clone()}</b>
                    {b.tip_lines.iter().map(|l| view! { <span>{l.clone()}</span> }).collect_view()}
                </div>
            }
        })
    };

    view! {
        <div class="chart">
            <div class="yaxis">{y_axis(&ticks, lo, hi, tick_dec, lang)}</div>
            <div
                class="plot"
                node_ref=plot
                on:pointermove=on_move
                on:pointerdown=on_move
                on:pointerleave=move |_| hover.set(None)
            >
                <div class="svgwrap" inner_html=svg></div>
                <div class="cols">{columns}</div>
                {tip}
            </div>
            <div class="xaxis">{x_labels}</div>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_is_nice() {
        let (lo, hi, ticks, dec) = nice_scale(5.3, 7.9, 4);
        assert_eq!((lo, hi), (5.0, 8.0));
        assert_eq!(ticks, [5.0, 6.0, 7.0, 8.0]);
        assert_eq!(dec, 0);

        let (lo, hi, ticks, dec) = nice_scale(1.62, 1.91, 4);
        assert!((lo - 1.6).abs() < 1e-9 && (hi - 2.0).abs() < 1e-9);
        assert_eq!(ticks.len(), 5);
        assert_eq!(dec, 1);

        let (lo, hi, _, _) = nice_scale(0.0, 0.0, 4);
        assert!(lo < 0.0 && hi > 0.0);

        let (_, _, ticks, dec) = nice_scale(0.0, 230.0, 4);
        assert_eq!(ticks, [0.0, 100.0, 200.0, 300.0]);
        assert_eq!(dec, 0);
    }
}
