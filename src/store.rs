use leptos::prelude::*;

use crate::i18n::{Lang, Texts};
use crate::model::{AppData, Record, Settings, SyncConf, Vehicle};
use crate::stats::{Entry, analyze};
use crate::storage::{self, DATA_KEY, SETTINGS_KEY, SYNC_KEY};
use crate::web;

/// Application state shared through context.
#[derive(Clone, Copy)]
pub struct Store {
    pub data: RwSignal<AppData>,
    pub settings: RwSignal<Settings>,
    pub sync: RwSignal<SyncConf>,
    pub sync_busy: RwSignal<bool>,
    /// Bumped on every local edit.
    pub local_change: RwSignal<u64>,
    pub save_error: RwSignal<Option<String>>,
}

impl Store {
    pub fn load() -> Self {
        let settings: Settings = storage::load(SETTINGS_KEY).unwrap_or_else(|| Settings {
            lang: web::browser_lang(),
            ..Default::default()
        });
        let mut data: AppData = storage::load(DATA_KEY).unwrap_or_default();
        data.version = 1;
        ensure_vehicle(&mut data, settings.lang);
        Self {
            data: RwSignal::new(data),
            settings: RwSignal::new(settings),
            sync: RwSignal::new(storage::load(SYNC_KEY).unwrap_or_default()),
            sync_busy: RwSignal::new(false),
            local_change: RwSignal::new(0),
            save_error: RwSignal::new(None),
        }
    }

    pub fn lang(&self) -> Lang {
        self.settings.with(|s| s.lang)
    }

    pub fn t(&self) -> &'static Texts {
        self.lang().t()
    }

    pub fn currency(&self) -> String {
        self.settings.with(|s| s.currency.clone())
    }

    pub fn vehicles(&self) -> Vec<Vehicle> {
        self.data.with(|d| d.vehicles.iter().filter(|v| !v.deleted).cloned().collect())
    }

    pub fn active_vehicle(&self) -> String {
        let wanted = self.settings.with(|s| s.active_vehicle.clone());
        self.data.with(|d| {
            let active = d.vehicles.iter().filter(|v| !v.deleted);
            active
                .clone()
                .find(|v| v.id == wanted)
                .or_else(|| active.clone().next())
                .map(|v| v.id.clone())
                .unwrap_or_default()
        })
    }

    pub fn set_active_vehicle(&self, id: &str) {
        self.update_settings(|s| s.active_vehicle = id.to_string());
    }

    pub fn records_for(&self, vehicle: &str) -> Vec<Record> {
        self.data.with(|d| {
            d.records
                .iter()
                .filter(|r| !r.deleted && r.vehicle_id == vehicle)
                .cloned()
                .collect()
        })
    }

    pub fn entries_for(&self, vehicle: &str) -> Vec<Entry> {
        analyze(&self.records_for(vehicle))
    }

    pub fn record(&self, id: &str) -> Option<Record> {
        self.data
            .with_untracked(|d| d.records.iter().find(|r| r.id == id && !r.deleted).cloned())
    }

    pub fn save_record(&self, mut rec: Record) {
        let now = web::now_ms();
        rec.updated_at = now;
        if rec.created_at == 0.0 {
            rec.created_at = now;
        }
        self.data.update(|d| match d.records.iter_mut().find(|r| r.id == rec.id) {
            Some(r) => *r = rec,
            None => d.records.push(rec),
        });
        self.commit();
    }

    fn set_deleted(&self, ids: &[String], deleted: bool) {
        let now = web::now_ms();
        self.data.update(|d| {
            for r in d.records.iter_mut().filter(|r| ids.contains(&r.id)) {
                r.deleted = deleted;
                r.updated_at = now;
            }
        });
        self.commit();
    }

    pub fn delete_records(&self, ids: &[String]) {
        self.set_deleted(ids, true);
    }

    pub fn restore_records(&self, ids: &[String]) {
        self.set_deleted(ids, false);
    }

    pub fn add_vehicle(&self, name: &str) -> String {
        let id = web::new_id();
        let v = Vehicle { id: id.clone(), name: name.to_string(), updated_at: web::now_ms(), deleted: false };
        self.data.update(|d| d.vehicles.push(v));
        self.commit();
        id
    }

    pub fn rename_vehicle(&self, id: &str, name: &str) {
        let now = web::now_ms();
        self.data.update(|d| {
            if let Some(v) = d.vehicles.iter_mut().find(|v| v.id == id) {
                v.name = name.to_string();
                v.updated_at = now;
            }
        });
        self.commit();
    }

    /// Deletes a vehicle together with its records.
    pub fn delete_vehicle(&self, id: &str) {
        let now = web::now_ms();
        let lang = self.lang();
        self.data.update(|d| {
            for v in d.vehicles.iter_mut().filter(|v| v.id == id) {
                v.deleted = true;
                v.updated_at = now;
            }
            for r in d.records.iter_mut().filter(|r| r.vehicle_id == id && !r.deleted) {
                r.deleted = true;
                r.updated_at = now;
            }
            ensure_vehicle(d, lang);
        });
        self.commit();
    }

    /// Merges records/vehicles from elsewhere; local edits count as changes.
    pub fn import(&self, other: &AppData) -> usize {
        let mut changed = 0;
        let lang = self.lang();
        self.data.update(|d| {
            changed = d.merge(other);
            ensure_vehicle(d, lang);
        });
        if changed > 0 {
            self.commit();
        }
        changed
    }

    /// Merges remote data without triggering another upload.
    pub fn merge_remote(&self, other: &AppData) -> usize {
        let mut changed = 0;
        let lang = self.lang();
        self.data.update(|d| {
            changed = d.merge(other);
            ensure_vehicle(d, lang);
        });
        if changed > 0 {
            self.persist();
        }
        changed
    }

    pub fn snapshot(&self) -> AppData {
        self.data.get_untracked().normalized()
    }

    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) {
        self.settings.update(f);
        let _ = storage::save(SETTINGS_KEY, &self.settings.get_untracked());
    }

    pub fn update_sync(&self, f: impl FnOnce(&mut SyncConf)) {
        self.sync.update(f);
        let _ = storage::save(SYNC_KEY, &self.sync.get_untracked());
    }

    pub fn clear_sync(&self) {
        self.sync.set(SyncConf::default());
        storage::remove(SYNC_KEY);
    }

    fn persist(&self) {
        let res = self.data.with_untracked(|d| storage::save(DATA_KEY, d));
        self.save_error.set(res.err());
    }

    fn commit(&self) {
        self.persist();
        self.local_change.update(|n| *n += 1);
    }
}

/// Makes sure at least one active vehicle exists.
fn ensure_vehicle(d: &mut AppData, lang: Lang) {
    if d.vehicles.iter().any(|v| !v.deleted) {
        return;
    }
    match d.vehicles.iter_mut().find(|v| v.id == "default") {
        Some(v) => {
            v.deleted = false;
            v.updated_at = web::now_ms();
        }
        None => d.vehicles.push(Vehicle {
            id: "default".into(),
            name: lang.t().default_vehicle.into(),
            updated_at: 0.0,
            deleted: false,
        }),
    }
}
