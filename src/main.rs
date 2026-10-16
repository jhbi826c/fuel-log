mod app;
mod charts;
mod csv;
mod demo;
mod format;
mod i18n;
mod model;
mod places;
mod stats;
mod storage;
mod store;
mod sync;
mod views;
mod web;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
