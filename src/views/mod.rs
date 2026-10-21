mod form;
mod map;
mod overview;
mod prices;
mod records;
mod settings;

pub use form::RecordForm;
pub use overview::Overview;
pub use prices::{PriceState, PricesView};
pub use records::Records;
pub use settings::SettingsView;

use leptos::prelude::*;

use crate::store::Store;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Chart,
    List,
    Plus,
    Settings,
    Sync,
    Pin,
    Locate,
    Search,
    Calendar,
    Fuel,
}

impl Icon {
    fn paths(self) -> &'static str {
        match self {
            Self::Chart => r#"<path d="M3 3v18h18"/><path d="M7 15l4-5 3 3 5-7"/>"#,
            Self::List => r#"<path d="M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01"/>"#,
            Self::Plus => r#"<circle cx="12" cy="12" r="9.5"/><path d="M12 8v8M8 12h8"/>"#,
            Self::Settings => {
                r#"<path d="M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6"/>"#
            }
            Self::Sync => r#"<path d="M21 12a9 9 0 0 1-15.4 6.4L3 16M3 12a9 9 0 0 1 15.4-6.4L21 8M21 3v5h-5M3 21v-5h5"/>"#,
            Self::Pin => r#"<path d="M12 21s-7-6.2-7-11.5a7 7 0 0 1 14 0C19 14.8 12 21 12 21z"/><circle cx="12" cy="9.5" r="2.5"/>"#,
            Self::Locate => r#"<circle cx="12" cy="12" r="7"/><circle cx="12" cy="12" r="2"/><path d="M12 1v4M12 19v4M1 12h4M19 12h4"/>"#,
            Self::Search => r#"<circle cx="11" cy="11" r="7"/><path d="M21 21l-5-5"/>"#,
            Self::Calendar => r#"<rect x="3" y="5" width="18" height="16" rx="2"/><path d="M3 10h18M8 3v4M16 3v4"/>"#,
            Self::Fuel => {
                r#"<path d="M4 21V5a2 2 0 0 1 2-2h6a2 2 0 0 1 2 2v16M2 21h14M7 8h4"/><path d="M14 10h2a2 2 0 0 1 2 2v4.5a1.5 1.5 0 0 0 3 0V9l-3-3"/>"#
            }
        }
    }
}

pub fn icon(i: Icon) -> impl IntoView {
    let svg = format!(
        r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{}</svg>"#,
        i.paths()
    );
    view! { <span class="ic" inner_html=svg></span> }
}

#[component]
pub fn EmptyState() -> impl IntoView {
    let store = expect_context::<Store>();
    move || {
        let t = store.t();
        view! {
            <div class="card empty">
                <img src="icons/icon.svg" alt="" width="64" height="64" />
                <h2>{t.empty_title}</h2>
                <p class="muted">{t.empty_text}</p>
                <a class="btn primary" href="#/add">
                    {t.add_first}
                </a>
            </div>
        }
    }
}
