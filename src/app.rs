use std::time::Duration;

use chrono::Months;
use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::i18n::fill;
use crate::stats::{Range, parse_dt};
use crate::store::Store;
use crate::sync::{SyncError, sync_once};
use crate::views::{Icon, Overview, RecordForm, Records, SettingsView, icon};
use crate::web;

#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Overview,
    Records,
    Add,
    Edit(String),
    Settings,
}

impl Route {
    pub fn parse(hash: &str) -> Self {
        let h = hash.trim_start_matches('#').trim_start_matches('/');
        match h.split_once('/') {
            Some(("edit", id)) if !id.is_empty() => Self::Edit(id.to_string()),
            _ => match h {
                "records" => Self::Records,
                "add" => Self::Add,
                "settings" => Self::Settings,
                _ => Self::Overview,
            },
        }
    }
}

/// Time span for the overview statistics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Period {
    All,
    Months(u32),
    Year(i32),
}

impl Period {
    pub fn range(self, today: &str) -> Range {
        match self {
            Self::All => Range::default(),
            Self::Months(n) => Range {
                from: parse_dt(today)
                    .and_then(|d| d.date().checked_sub_months(Months::new(n)))
                    .map(|d| d.format("%Y-%m-%d").to_string()),
                to: None,
            },
            Self::Year(y) => Range { from: Some(format!("{y:04}-01-01")), to: Some(format!("{:04}-01-01", y + 1)) },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub msg: String,
    pub undo: Vec<String>,
    pub error: bool,
}

/// UI state shared through context.
#[derive(Clone, Copy)]
pub struct Ui {
    pub route: RwSignal<Route>,
    pub toast: RwSignal<Option<Toast>>,
    toast_seq: RwSignal<u64>,
    pub period: RwSignal<Period>,
    pub search: RwSignal<String>,
    sync_pending: RwSignal<bool>,
}

impl Ui {
    fn show(&self, msg: String, undo: Vec<String>, error: bool) {
        let id = self.toast_seq.get_untracked() + 1;
        self.toast_seq.set(id);
        let ms = if undo.is_empty() && !error { 3000 } else { 6000 };
        self.toast.set(Some(Toast { id, msg, undo, error }));
        let toast = self.toast;
        set_timeout(
            move || {
                if toast.with_untracked(|t| t.as_ref().map(|t| t.id)) == Some(id) {
                    toast.set(None);
                }
            },
            Duration::from_millis(ms),
        );
    }

    pub fn toast(&self, msg: impl Into<String>) {
        self.show(msg.into(), Vec::new(), false);
    }

    pub fn error(&self, msg: impl Into<String>) {
        self.show(msg.into(), Vec::new(), true);
    }

    pub fn toast_undo(&self, msg: impl Into<String>, ids: Vec<String>) {
        self.show(msg.into(), ids, false);
    }
}

fn auto_sync_on(store: Store) -> bool {
    store.sync.with_untracked(|c| c.auto && !c.token.trim().is_empty())
}

/// Runs a sync in the background; repeats once if requested while running.
pub fn run_sync(store: Store, ui: Ui, silent: bool) {
    if store.sync_busy.get_untracked() {
        ui.sync_pending.set(true);
        return;
    }
    store.sync_busy.set(true);
    spawn_local(async move {
        let mut silent = silent;
        loop {
            ui.sync_pending.set(false);
            let t = store.t();
            match sync_once(store).await {
                Ok(r) if r.pulled > 0 => ui.toast(fill(t.sync_done_pulled, &[("n", &r.pulled)])),
                Ok(_) if !silent => ui.toast(t.sync_done),
                Ok(_) => {}
                Err(SyncError::Network(_)) if silent => {}
                Err(e) => ui.error(sync_error_text(store, &e)),
            }
            if !ui.sync_pending.get_untracked() {
                break;
            }
            silent = true;
        }
        store.sync_busy.set(false);
    });
}

pub fn sync_error_text(store: Store, e: &SyncError) -> String {
    let t = store.t();
    match e {
        SyncError::NoToken => t.sync_err_no_token.into(),
        SyncError::Unauthorized => t.sync_err_unauthorized.into(),
        SyncError::Http(code, msg) => format!("{} ({code} {msg})", t.sync_err_http),
        SyncError::Network(msg) => format!("{} ({msg})", t.sync_err_network),
        SyncError::BadRemote(msg) => format!("{} ({msg})", t.sync_err_bad_remote),
    }
}

#[component]
pub fn App() -> impl IntoView {
    let store = Store::load();
    provide_context(store);
    let ui = Ui {
        route: RwSignal::new(Route::parse(&web::hash())),
        toast: RwSignal::new(None),
        toast_seq: RwSignal::new(0),
        period: RwSignal::new(Period::All),
        search: RwSignal::new(String::new()),
        sync_pending: RwSignal::new(false),
    };
    provide_context(ui);

    let _ = window_event_listener(ev::hashchange, move |_| {
        ui.route.set(Route::parse(&web::hash()));
        web::scroll_top();
    });

    Effect::new(move |_| {
        let lang = store.lang();
        web::set_document_lang(lang.code(), lang.t().app_name);
    });

    Effect::new(move |prev: Option<u64>| {
        let n = store.local_change.get();
        if prev.is_some() && auto_sync_on(store) {
            set_timeout(
                move || {
                    if store.local_change.get_untracked() == n {
                        run_sync(store, ui, true);
                    }
                },
                Duration::from_millis(1500),
            );
        }
        n
    });

    if auto_sync_on(store) {
        run_sync(store, ui, true);
    }
    let _ = window_event_listener(ev::focus, move |_| {
        let stale = web::now_ms() - store.sync.with_untracked(|c| c.last_sync) > 5.0 * 60_000.0;
        if stale && auto_sync_on(store) {
            run_sync(store, ui, true);
        }
    });

    view! {
        <header class="topbar">
            <a class="brand" href="#/overview">
                <img src="icons/icon.svg" alt="" width="28" height="28" />
                <span>{move || store.t().app_name}</span>
            </a>
            <div class="topbar-right">
                <VehicleSelect />
                {move || {
                    store
                        .sync
                        .with(|c| !c.token.is_empty())
                        .then(|| {
                            view! {
                                <button
                                    class=move || if store.sync_busy.get() { "icon-btn spin" } else { "icon-btn" }
                                    title=move || store.t().sync_now
                                    aria-label=move || store.t().sync_now
                                    on:click=move |_| run_sync(store, ui, false)
                                >
                                    {icon(Icon::Sync)}
                                </button>
                            }
                        })
                }}
            </div>
        </header>
        {move || {
            store
                .save_error
                .get()
                .map(|e| view! { <div class="banner">{store.t().save_failed}" ("{e}")"</div> })
        }}
        <main class="view">
            {move || match ui.route.get() {
                Route::Overview => view! { <Overview /> }.into_any(),
                Route::Records => view! { <Records /> }.into_any(),
                Route::Add => view! { <RecordForm /> }.into_any(),
                Route::Edit(id) => view! { <RecordForm id=id /> }.into_any(),
                Route::Settings => view! { <SettingsView /> }.into_any(),
            }}
        </main>
        <nav class="tabbar">
            <Tab href="#/overview" icon=Icon::Chart label=|t| t.nav_overview active=|r| *r == Route::Overview />
            <Tab
                href="#/records"
                icon=Icon::List
                label=|t| t.nav_records
                active=|r| matches!(r, Route::Records | Route::Add | Route::Edit(_))
            />
            <Tab href="#/settings" icon=Icon::Settings label=|t| t.nav_settings active=|r| *r == Route::Settings />
        </nav>
        {move || {
            matches!(ui.route.get(), Route::Overview | Route::Records)
                .then(|| {
                    view! {
                        <a class="fab" href="#/add" title=move || store.t().nav_add aria-label=move || store.t().nav_add>
                            {icon(Icon::Plus)}
                            <span>{move || store.t().nav_add}</span>
                        </a>
                    }
                })
        }}
        <ToastView />
    }
}

#[component]
fn Tab(
    href: &'static str,
    icon: Icon,
    label: fn(&'static crate::i18n::Texts) -> &'static str,
    active: fn(&Route) -> bool,
) -> impl IntoView {
    let store = expect_context::<Store>();
    let ui = expect_context::<Ui>();
    view! {
        <a
            href=href
            class=move || if ui.route.with(active) { "tab active" } else { "tab" }
            aria-current=move || ui.route.with(active).then_some("page")
        >
            {crate::views::icon(icon)}
            <span>{move || label(store.t())}</span>
        </a>
    }
}

#[component]
fn VehicleSelect() -> impl IntoView {
    let store = expect_context::<Store>();
    move || {
        let vehicles = store.vehicles();
        let active = store.active_vehicle();
        (vehicles.len() > 1).then(|| {
            view! {
                <select
                    class="vehicle-select"
                    aria-label=store.t().vehicle
                    on:change=move |ev| store.set_active_vehicle(&event_target_value(&ev))
                >
                    {vehicles
                        .into_iter()
                        .map(|v| {
                            let selected = v.id == active;
                            view! {
                                <option value=v.id selected=selected>
                                    {v.name}
                                </option>
                            }
                        })
                        .collect_view()}
                </select>
            }
        })
    }
}

#[component]
fn ToastView() -> impl IntoView {
    let store = expect_context::<Store>();
    let ui = expect_context::<Ui>();
    move || {
        ui.toast.get().map(|toast| {
            let ids = toast.undo.clone();
            let undo = (!ids.is_empty()).then(|| {
                view! {
                    <button
                        class="toast-action"
                        on:click=move |_| {
                            store.restore_records(&ids);
                            ui.toast(store.t().restored);
                        }
                    >
                        {store.t().undo}
                    </button>
                }
            });
            view! {
                <div class=if toast.error { "toast error" } else { "toast" } role="status">
                    <span>{toast.msg}</span>
                    {undo}
                </div>
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes() {
        assert_eq!(Route::parse(""), Route::Overview);
        assert_eq!(Route::parse("#/records"), Route::Records);
        assert_eq!(Route::parse("#/edit/abc"), Route::Edit("abc".into()));
        assert_eq!(Route::parse("#/edit/"), Route::Overview);
    }

    #[test]
    fn periods() {
        let r = Period::Months(3).range("2026-05-31T10:00");
        assert_eq!(r.from.as_deref(), Some("2026-02-28"));
        let y = Period::Year(2025).range("2026-05-31T10:00");
        assert!(y.contains("2025-12-31T23:59") && !y.contains("2026-01-01T00:00"));
    }
}
