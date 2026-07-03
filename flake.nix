{
  description = "karamd - recurring-task generator for a taskmd markdown vault";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f system);
      # nixpkgs with our overlay applied, per system.
      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ self.overlays.default ];
        };
      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

      # Prebuilt release binaries (built by .github/workflows/release.yml and
      # attached to the `v${version}` GitHub Release). Linux targets are static
      # musl, so no glibc patching is needed under Nix. Fill each hash AFTER the
      # first release: run `nix build .#karamd-bin`, and Nix prints the real hash
      # to paste in place of `fakeHash` (which forces a mismatch every build).
      prebuilt = {
        "x86_64-linux" = {
          target = "x86_64-unknown-linux-musl";
          hash = "sha256-l9tsaKxtRdbsKomm/EMWD9iewpndPxOfZ5Ii7j/77zA=";
        };
        "aarch64-linux" = {
          target = "aarch64-unknown-linux-musl";
          hash = "sha256-uq+/1cFf9cGM6wqQVTKm1lY8UoQFUF2POD+1xmrjCYE=";
        };
        "x86_64-darwin" = {
          target = "x86_64-apple-darwin";
          hash = "sha256-rKKMlVVPjkd2HaXAJkyZ3474wekOFPHtdkv8x9IBYr8=";
        };
        "aarch64-darwin" = {
          target = "aarch64-apple-darwin";
          hash = "sha256-T0CvosamlmrY3G7gi/Y7mual3ynaYZLcfUe/UuoM0Vk=";
        };
      };

      mkBin =
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          p = prebuilt.${system};
        in
        pkgs.stdenvNoCC.mkDerivation {
          pname = "karamd";
          inherit version;
          src = pkgs.fetchurl {
            url = "https://github.com/PatrickLerner/karamd/releases/download/v${version}/karamd-${p.target}.tar.gz";
            inherit (p) hash;
          };
          sourceRoot = ".";
          dontConfigure = true;
          dontBuild = true;
          installPhase = ''
            runHook preInstall
            install -Dm755 karamd $out/bin/karamd
            runHook postInstall
          '';
          meta = {
            description = "Recurring-task generator for a taskmd markdown vault";
            license = pkgs.lib.licenses.mit;
            mainProgram = "karamd";
          };
        };
    in
    {
      # Add `karamd` to a downstream config: apply this overlay, use pkgs.karamd.
      # This builds from source. For a no-compile install, use `packages.karamd-bin`.
      overlays.default = final: _prev: {
        karamd = final.rustPlatform.buildRustPackage {
          pname = "karamd";
          inherit version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          meta = {
            description = "Recurring-task generator for a taskmd markdown vault";
            license = final.lib.licenses.mit;
            mainProgram = "karamd";
          };
        };

        # The SPA's node_modules, as a fixed-output derivation (the only step
        # allowed to touch the network). Fill `outputHash` after the first
        # `nix build .#karamd-web`: Nix prints the real hash on mismatch, exactly
        # like the prebuilt-binary hashes above.
        karamd-web-deps = final.stdenvNoCC.mkDerivation {
          pname = "karamd-web-deps";
          inherit version;
          src = ./web;
          nativeBuildInputs = [ final.bun ];
          dontConfigure = true;
          buildPhase = ''
            export HOME=$TMPDIR
            bun install --frozen-lockfile --no-progress
          '';
          installPhase = ''
            mkdir -p $out
            cp -R node_modules $out/node_modules
          '';
          dontFixup = true;
          outputHashMode = "recursive";
          outputHashAlgo = "sha256";
          outputHash = final.lib.fakeHash;
        };

        # The built SPA bundle (offline: deps come from karamd-web-deps).
        karamd-web-bundle = final.stdenvNoCC.mkDerivation {
          pname = "karamd-web-bundle";
          inherit version;
          src = ./web;
          nativeBuildInputs = [ final.bun ];
          dontConfigure = true;
          buildPhase = ''
            export HOME=$TMPDIR
            cp -R ${final.karamd-web-deps}/node_modules ./node_modules
            # --production (not just NODE_ENV + --minify) is what makes bun emit
            # the React *production* JSX runtime; the dev runtime bundles a
            # jsxDEV that resolves to undefined here and renders a blank page.
            bun build src/main.tsx --outdir dist --production
            cp index.html src/styles.css dist/
            cp -R public/. dist/
          '';
          installPhase = ''cp -R dist $out'';
        };

        # karamd + the pinned bundle in one closure: the wrapper defaults
        # KARAMD_WEB_DIR to the store path, so `karamd web` needs no --web-dir.
        karamd-web = final.runCommand "karamd-web-${version}" {
          nativeBuildInputs = [ final.makeWrapper ];
          meta = {
            description = "karamd with the bundled web UI";
            license = final.lib.licenses.mit;
            mainProgram = "karamd";
          };
        } ''
          mkdir -p $out/bin
          makeWrapper ${final.karamd}/bin/karamd $out/bin/karamd \
            --set-default KARAMD_WEB_DIR ${final.karamd-web-bundle}
        '';
      };

      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          # Built from source (always available, even before a release exists).
          karamd = pkgs.karamd;
          default = pkgs.karamd;
          # Prebuilt release binary (no local compilation).
          karamd-bin = mkBin system;
          # karamd bundled with the web UI (frontend + binary in one closure).
          inherit (pkgs) karamd-web;
        }
      );

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/karamd";
        };
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.rustc
              pkgs.clippy
              pkgs.rustfmt
              pkgs.bun
            ];
          };
        }
      );
    };
}
