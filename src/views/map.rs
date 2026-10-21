use leptos::ev;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{MouseEvent, PointerEvent, WheelEvent};

use crate::places::geo::{Bounds, meters_per_px, project, unproject, zoom_for_km};

const TILE: f64 = 256.0;
const TILE_URL: &str = "https://tile.openstreetmap.org";
const MAX_ZOOM: f64 = 18.0;
/// Pointer travel before a press counts as a drag.
const DRAG_PX: f64 = 4.0;

/// A labelled point on the map.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub id: String,
    pub lat: f64,
    pub lng: f64,
    pub label: String,
    pub class: &'static str,
    pub logo: Option<String>,
}

/// Map position, kept outside the component so it survives remounts.
#[derive(Clone, Copy)]
pub struct MapView {
    pub center: RwSignal<(f64, f64)>,
    pub zoom: RwSignal<f64>,
    pub size: RwSignal<(f64, f64)>,
    /// Moved by the user since the last `fit`.
    pub moved: RwSignal<bool>,
    ready: RwSignal<bool>,
}

impl Default for MapView {
    fn default() -> Self {
        Self {
            center: RwSignal::new((51.16, 10.45)),
            zoom: RwSignal::new(5.5),
            size: RwSignal::new((0.0, 0.0)),
            moved: RwSignal::new(false),
            ready: RwSignal::new(false),
        }
    }
}

impl MapView {
    /// Shows a circle of `radius_km` around `center`.
    pub fn fit(&self, center: (f64, f64), radius_km: f64) {
        let (w, h) = self.size.get_untracked();
        let px = if w > 0.0 { w.min(h) * 0.85 } else { 300.0 };
        self.zoom.set(zoom_for_km(2.0 * radius_km, px, center.0).clamp(3.0, MAX_ZOOM));
        self.center.set(center);
        self.moved.set(false);
    }

    /// Pans so that `pos` sits in the lower part of the view, leaving room for a popup above it.
    pub fn focus(&self, pos: (f64, f64)) {
        let (_, h) = self.size.get_untracked();
        let p = project(pos);
        let dy = h * 0.25 / (TILE * 2f64.powf(self.zoom.get_untracked()));
        self.center.set(unproject((p.0, p.1 - dy)));
    }
}

#[derive(Clone, Copy)]
struct Gesture {
    center: (f64, f64),
    zoom: f64,
    mid: (f64, f64),
    dist: f64,
    moved: bool,
}

fn mid_dist(ps: &[(i32, f64, f64)]) -> ((f64, f64), f64) {
    let n = ps.len().max(1) as f64;
    let mid = (ps.iter().map(|p| p.1).sum::<f64>() / n, ps.iter().map(|p| p.2).sum::<f64>() / n);
    let dist = match ps {
        [a, b, ..] => ((a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt(),
        _ => 0.0,
    };
    (mid, dist)
}

#[component]
pub fn SlippyMap(
    view: MapView,
    bounds: Bounds,
    min_zoom: f64,
    #[prop(into)] markers: Signal<Vec<Marker>>,
    /// Center and radius in km of the searched area.
    #[prop(into)]
    circle: Signal<Option<((f64, f64), f64)>>,
    #[prop(into)] selected: Signal<Option<String>>,
    on_select: Callback<String>,
    /// Position of the popup's anchor; the popup is hidden when `None`.
    #[prop(into)]
    popup_at: Signal<Option<(f64, f64)>>,
    popup: ViewFn,
    /// Tap on an empty part of the map.
    on_background: Callback<()>,
    zoom_in_label: &'static str,
    zoom_out_label: &'static str,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    let el = NodeRef::<leptos::html::Div>::new();
    let pointers = StoredValue::new(Vec::<(i32, f64, f64)>::new());
    let gesture = StoredValue::new(None::<Gesture>);
    let dragged = StoredValue::new(false);
    let popup = StoredValue::new(popup);

    let world = |z: f64| TILE * 2f64.powf(z);
    let half = move || {
        let (w, h) = view.size.get_untracked();
        (w / 2.0, h / 2.0)
    };
    let set_view = move |merc: (f64, f64), zoom: f64| {
        view.center.set(bounds.clamp(unproject(merc)));
        view.zoom.set(zoom);
    };
    // Changes zoom keeping the map point under `at` in place.
    let zoom_at = move |at: (f64, f64), zoom: f64| {
        let zoom = zoom.clamp(min_zoom, MAX_ZOOM);
        let z0 = view.zoom.get_untracked();
        let c = project(view.center.get_untracked());
        let h = half();
        let p = (c.0 + (at.0 - h.0) / world(z0), c.1 + (at.1 - h.1) / world(z0));
        set_view((p.0 - (at.0 - h.0) / world(zoom), p.1 - (at.1 - h.1) / world(zoom)), zoom);
        view.moved.set(true);
    };
    let local = move |x: i32, y: i32| {
        el.get_untracked()
            .map(|e| {
                let r = e.get_bounding_client_rect();
                (x as f64 - r.left(), y as f64 - r.top())
            })
            .unwrap_or_default()
    };
    let measure = move || {
        if let Some(e) = el.get_untracked() {
            let r = e.get_bounding_client_rect();
            view.size.set((r.width(), r.height()));
            if !view.ready.get_untracked() && r.width() > 0.0 {
                view.ready.set(true);
                view.zoom.set(bounds.fit_zoom(r.width(), r.height()).max(min_zoom));
                view.center.set(bounds.center());
            }
        }
    };
    Effect::new(move |_| {
        if el.get().is_some() {
            measure();
        }
    });
    let resize = window_event_listener(ev::resize, move |_| measure());
    on_cleanup(move || resize.remove());

    let restart = move || {
        let ps = pointers.get_value();
        let moved = gesture.get_value().is_some_and(|g| g.moved);
        gesture.set_value((!ps.is_empty()).then(|| {
            let (mid, dist) = mid_dist(&ps);
            Gesture { center: project(view.center.get_untracked()), zoom: view.zoom.get_untracked(), mid, dist, moved }
        }));
    };
    let on_down = move |e: PointerEvent| {
        if e.pointer_type() == "mouse" && e.button() != 0 {
            return;
        }
        let p = local(e.client_x(), e.client_y());
        if pointers.with_value(Vec::is_empty) {
            dragged.set_value(false);
        }
        pointers.update_value(|v| {
            v.retain(|q| q.0 != e.pointer_id());
            v.push((e.pointer_id(), p.0, p.1));
        });
        restart();
    };
    let on_move = move |e: PointerEvent| {
        let id = e.pointer_id();
        if !pointers.with_value(|v| v.iter().any(|q| q.0 == id)) {
            return;
        }
        let p = local(e.client_x(), e.client_y());
        pointers.update_value(|v| {
            if let Some(q) = v.iter_mut().find(|q| q.0 == id) {
                (q.1, q.2) = p;
            }
        });
        let Some(mut g) = gesture.get_value() else { return };
        let ps = pointers.get_value();
        let (mid, dist) = mid_dist(&ps);
        if !g.moved {
            if ps.len() < 2 && (mid.0 - g.mid.0).hypot(mid.1 - g.mid.1) < DRAG_PX {
                return;
            }
            g.moved = true;
            gesture.set_value(Some(g));
            dragged.set_value(true);
            if let Some(el) = el.get_untracked() {
                for q in &ps {
                    let _ = el.set_pointer_capture(q.0);
                }
            }
        }
        let zoom = if ps.len() >= 2 && g.dist > 0.0 { g.zoom + (dist / g.dist).log2() } else { g.zoom };
        let zoom = zoom.clamp(min_zoom, MAX_ZOOM);
        let h = half();
        let anchor = (g.center.0 + (g.mid.0 - h.0) / world(g.zoom), g.center.1 + (g.mid.1 - h.1) / world(g.zoom));
        set_view((anchor.0 - (mid.0 - h.0) / world(zoom), anchor.1 - (mid.1 - h.1) / world(zoom)), zoom);
        view.moved.set(true);
    };
    let on_up = move |e: PointerEvent| {
        pointers.update_value(|v| v.retain(|q| q.0 != e.pointer_id()));
        if pointers.with_value(Vec::is_empty) {
            gesture.set_value(None);
        } else {
            restart();
        }
    };
    let on_wheel = move |e: WheelEvent| {
        e.prevent_default();
        let dy = e.delta_y() * if e.delta_mode() == 1 { 16.0 } else { 1.0 };
        zoom_at(local(e.client_x(), e.client_y()), view.zoom.get_untracked() - dy * 0.002);
    };
    let on_dblclick = move |e: MouseEvent| zoom_at(local(e.client_x(), e.client_y()), view.zoom.get_untracked() + 1.0);
    let on_click = move |e: MouseEvent| {
        let on_control = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|t| t.closest(".map-marker, .map-popup, .map-zoom, .map-overlay, .map-attrib").ok().flatten())
            .is_some();
        if !dragged.get_value() && !on_control {
            on_background.run(());
        }
    };
    let step = move |d: f64| zoom_at(half(), view.zoom.get_untracked().round() + d);

    let screen = move |(lat, lng): (f64, f64)| {
        let c = project(view.center.get());
        let p = project((lat, lng));
        let (w, h) = view.size.get();
        let s = world(view.zoom.get());
        ((p.0 - c.0) * s + w / 2.0, (p.1 - c.1) * s + h / 2.0)
    };

    let tiles = Memo::new(move |_| {
        let (w, h) = view.size.get();
        if w <= 0.0 {
            return Vec::new();
        }
        let zoom = view.zoom.get();
        let z = zoom.floor().clamp(0.0, MAX_ZOOM) as u32;
        let scale = 2f64.powf(zoom - z as f64);
        let n = 1i64 << z;
        let c = project(view.center.get());
        let (cx, cy) = (c.0 * TILE * n as f64, c.1 * TILE * n as f64);
        let (hw, hh) = (w / 2.0 / scale, h / 2.0 / scale);
        let range = |a: f64, b: f64| ((a / TILE).floor() as i64).max(0)..=((b / TILE).floor() as i64).min(n - 1);
        let ys = range(cy - hh, cy + hh);
        range(cx - hw, cx + hw)
            .flat_map(|x| ys.clone().map(move |y| (z, x, y)))
            .collect::<Vec<_>>()
    });
    let tile_style = move |z: u32, x: i64, y: i64| {
        let zoom = view.zoom.get();
        let scale = 2f64.powf(zoom - z as f64);
        let n = (1i64 << z) as f64;
        let c = project(view.center.get());
        let (w, h) = view.size.get();
        let left = (x as f64 * TILE - c.0 * TILE * n) * scale + w / 2.0;
        let top = (y as f64 * TILE - c.1 * TILE * n) * scale + h / 2.0;
        format!("transform:translate({left:.1}px,{top:.1}px);width:{0:.1}px;height:{0:.1}px", TILE * scale + 0.5)
    };

    let circle_view = move || {
        let (center, km) = circle.get()?;
        let (x, y) = screen(center);
        let r = km * 1000.0 / meters_per_px(center.0, view.zoom.get());
        Some(view! {
            <div class="map-circle" style=format!("transform:translate({:.1}px,{:.1}px);width:{2:.1}px;height:{2:.1}px", x - r, y - r, 2.0 * r)></div>
            <div class="map-origin" style=format!("transform:translate({x:.1}px,{y:.1}px)")></div>
        })
    };

    view! {
        <div
            class="map"
            node_ref=el
            on:pointerdown=on_down
            on:pointermove=on_move
            on:pointerup=on_up
            on:pointercancel=on_up
            on:wheel=on_wheel
            on:dblclick=on_dblclick
            on:click=on_click
        >
            <div class="map-tiles">
                <For each=move || tiles.get() key=|t| *t let((z, x, y))>
                    <img
                        src=format!("{TILE_URL}/{z}/{x}/{y}.png")
                        alt=""
                        draggable="false"
                        decoding="async"
                        style=move || tile_style(z, x, y)
                    />
                </For>
            </div>
            {circle_view}
            <For each=move || markers.get() key=|m| (m.id.clone(), m.label.clone(), m.class) let(m)>
                {
                    let id = m.id.clone();
                    let sel_id = m.id.clone();
                    view! {
                        <button
                            type="button"
                            class=move || {
                                let sel = selected.with(|s| s.as_deref() == Some(sel_id.as_str()));
                                format!("map-marker {}{}", m.class, if sel { " selected" } else { "" })
                            }
                            style=move || {
                                let (x, y) = screen((m.lat, m.lng));
                                format!("transform:translate({x:.1}px,{y:.1}px) translate(-50%,-100%)")
                            }
                            on:click=move |_| on_select.run(id.clone())
                        >
                            {m.logo.clone().map(|src| view! { <img src=src alt="" draggable="false" /> })}
                            {m.label.clone()}
                        </button>
                    }
                }
            </For>
            <Show when=move || popup_at.with(Option::is_some)>
                <div
                    class="map-popup"
                    on:dblclick=|e| e.stop_propagation()
                    on:wheel=|e| e.stop_propagation()
                    style=move || {
                        let (x, y) = popup_at.get().map(screen).unwrap_or_default();
                        format!("transform:translate({x:.1}px,{y:.1}px) translate(-50%,calc(-100% - 34px))")
                    }
                >
                    {move || popup.with_value(ViewFn::run)}
                </div>
            </Show>
            <div class="map-zoom" on:dblclick=|e| e.stop_propagation()>
                <button type="button" aria-label=zoom_in_label title=zoom_in_label on:click=move |_| step(1.0)>
                    "+"
                </button>
                <button type="button" aria-label=zoom_out_label title=zoom_out_label on:click=move |_| step(-1.0)>
                    "−"
                </button>
            </div>
            <div class="map-overlay" on:dblclick=|e| e.stop_propagation()>
                {children.map(|c| c())}
            </div>
            <a class="map-attrib" href="https://www.openstreetmap.org/copyright" target="_blank" rel="noopener noreferrer">
                "© OpenStreetMap"
            </a>
        </div>
    }
}
