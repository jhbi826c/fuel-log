{
  description = "Fuel Log - personal fuel consumption tracker (Rust + Leptos, static PWA)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      rust-overlay,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            }
          )
        );

      mkProject =
        pkgs:
        let
          inherit (pkgs) lib;

          toolchain = pkgs.rust-bin.stable.latest.default.override {
            targets = [ "wasm32-unknown-unknown" ];
          };
          craneLib = (crane.mkLib pkgs).overrideToolchain (_: toolchain);

          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./Trunk.toml
              ./src
              ./index.html
              ./style.css
              ./sw.js
              ./manifest.webmanifest
              ./icons
            ];
          };

          lockedWasmBindgen =
            (lib.findFirst (p: p.name == "wasm-bindgen") null
              (builtins.fromTOML (builtins.readFile ./Cargo.lock)).package
            ).version;
          wasm-bindgen-cli =
            assert lib.assertMsg (lockedWasmBindgen == pkgs.wasm-bindgen-cli.version) ''
              wasm-bindgen version mismatch: Cargo.lock has ${lockedWasmBindgen},
              nixpkgs provides wasm-bindgen-cli ${pkgs.wasm-bindgen-cli.version}.
              Update the `wasm-bindgen = "=..."` pin in Cargo.toml and run `cargo update -p wasm-bindgen`.
            '';
            pkgs.wasm-bindgen-cli;

          commonArgs = {
            inherit src;
            pname = "fuel-log";
            strictDeps = true;
          };

          wasmArgs = commonArgs // {
            CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
          };
          wasmArtifacts = craneLib.buildDepsOnly (wasmArgs // { doCheck = false; });

          site = craneLib.buildTrunkPackage (
            wasmArgs
            // {
              cargoArtifacts = wasmArtifacts;
              inherit wasm-bindgen-cli;
              nativeBuildInputs = [ pkgs.binaryen ];
            }
          );

          nativeArtifacts = craneLib.buildDepsOnly commonArgs;
        in
        {
          inherit
            craneLib
            site
            wasm-bindgen-cli
            ;
          tests = craneLib.cargoTest (commonArgs // { cargoArtifacts = nativeArtifacts; });
          clippy = craneLib.cargoClippy (
            wasmArgs
            // {
              cargoArtifacts = wasmArtifacts;
              cargoClippyExtraArgs = "-- --deny warnings";
            }
          );
        };
    in
    {
      packages = forAllSystems (
        pkgs:
        let
          p = mkProject pkgs;
        in
        {
          default = p.site;
          site = p.site;
        }
      );

      checks = forAllSystems (
        pkgs:
        let
          p = mkProject pkgs;
        in
        {
          inherit (p) site tests clippy;
        }
      );

      apps = forAllSystems (
        pkgs:
        let
          p = mkProject pkgs;
          serve = pkgs.writeShellScript "serve-fuel-log" ''
            echo "Serving ${p.site} on http://localhost:''${PORT:-8080}"
            exec ${pkgs.python3}/bin/python3 -m http.server "''${PORT:-8080}" --directory ${p.site}
          '';
        in
        {
          serve = {
            type = "app";
            program = "${serve}";
          };
          default = self.apps.${pkgs.stdenv.hostPlatform.system}.serve;
        }
      );

      devShells = forAllSystems (
        pkgs:
        let
          p = mkProject pkgs;
        in
        {
          default = p.craneLib.devShell {
            packages = [
              pkgs.trunk
              p.wasm-bindgen-cli
              pkgs.binaryen
            ];
          };
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
