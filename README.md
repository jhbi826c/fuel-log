# Fuel Log

A personal fuel consumption tracker that runs entirely in the browser. Written in Rust ([Leptos](https://leptos.dev), compiled to WebAssembly), built with Nix and hosted for free on GitHub Pages.

UI languages: English, 中文, Deutsch.

## Features

- **Fill-ups**: date and time, odometer (or trip distance), liters, price per liter, total (enter any two, the third is calculated), amount actually paid (discounts), full or partial tank, missed fill-ups, fuel type, station, address with GPS location, the consumption shown by the car's display, payment method and notes.
- **Consumption**: calculated full-to-full. Partial fill-ups are added to the next full tank, and a missed fill-up starts a new measurement.
- **Statistics** (all time, 3 or 12 months, or per year):
  - Average consumption, average price, cost per km and per 100 km.
  - Total spent, liters and distance; monthly and yearly cost.
  - Fill-up interval; best, worst and last consumption; price range.
  - Car display compared with the calculated value, and discounts saved.
- **Charts**: consumption trend, price trend and monthly spending, with touch-friendly tooltips.
- **Tables**: per year, per station and per fuel type.
- **Station picker**: "Nearby" in the fill-up form uses your location (and fills in the address via Nominatim); "Search here" uses the address or place typed into the address field. Both list the nearest fuel stations from OpenStreetMap (within 2 km, widening to 5 and 10 km when fewer than 3 are found), with the fuel types each station is tagged with; picking one fills in name, address and location.
- **Fuel types**: picked from a fixed list (Super E10, Super E5, Super Plus, Super 100, Diesel, Premium Diesel, LPG, CNG, E85) or entered freely. Each vehicle can be limited to the fuels it uses; "Nearby" then hides stations tagged with other fuels only, and the form offers only those fuels. Types the chosen station is not tagged with are dimmed.
- **Map links**: Google Maps, Apple Maps, OpenStreetMap or OsmAnd.
- **Several vehicles.**
- **Data**:
  - Stored locally in the browser.
  - Optional sync across devices through a private GitHub Gist.
  - Backup export and import as a JSON file.
- **PWA**: can be added to the home screen and works offline.

## Development

Requires Nix with flakes enabled.

```sh
nix develop            # shell with Rust (wasm32), trunk, wasm-bindgen, binaryen
trunk serve            # dev server with live reload on http://127.0.0.1:8080
cargo test             # unit tests (statistics, CSV, formatting, merge, …)

nix build .#site       # release build of the static site into ./result
nix run .#serve        # build and serve the release site on http://localhost:8080
nix flake check        # tests + clippy + site build
```

The `wasm-bindgen` crate is pinned to the exact version of `wasm-bindgen-cli` in nixpkgs. After `nix flake update`, the build will say if they no longer match. In that case, update the pin in `Cargo.toml` and run `cargo update -p wasm-bindgen`.

## Deployment (GitHub Pages)

1. Create a repository on GitHub (for example `fuel-log`) and push this project to the `main` branch.
2. In the repository, open **Settings → Pages** and set **Source** to **GitHub Actions**.
3. Every push to `main` runs `.github/workflows/deploy.yml`. It runs the tests, builds the site with `nix build .#site` and publishes it to `https://<user>.github.io/<repo>/`.

The build uses relative URLs, so it works under any repository name.

## Sync across devices

By default, data lives only in the browser where it was entered. To use the same data on several devices:

1. Create a GitHub token with the `gist` scope ([direct link](https://github.com/settings/tokens/new?scopes=gist&description=Fuel%20Log)).
2. In the app, open **Settings → Sync**, paste the token and connect.

The data is then stored in a secret gist named `fuel-log.json`. Other devices that use a token from the same account find that gist automatically. Changes are merged per record (the newest edit wins, and deletions sync too).

The token is stored only in the browser's local storage, never in the repository or in exports.

## Privacy

The repository and the website contain only the app, never your data. Fill-ups stay in your browser and, if sync is enabled, in your own secret gist.
