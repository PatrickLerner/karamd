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
          hash = "sha256-6Cl1+MD5F2TN67QqcRe5V2Vh4a/8lxgNsvInSVgwvrc=";
        };
        "aarch64-linux" = {
          target = "aarch64-unknown-linux-musl";
          hash = "sha256-GxplGN8vs6uq0X/GZVPIkKWcoWmDD0AKK1f8q76nRt0=";
        };
        "x86_64-darwin" = {
          target = "x86_64-apple-darwin";
          hash = "sha256-7o4/MorE8KjYE1l6pSoQytqbyNDOf6yKjKOy3eJP2W4=";
        };
        "aarch64-darwin" = {
          target = "aarch64-apple-darwin";
          hash = "sha256-aLE3e9SWfBEMvlBRv/1FtJTus5gQ7AHCCF8Pgn36stU=";
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

        # The prebuilt SPA bundle: `dist/` built by bun in the release workflow
        # (deterministic, `--production`) and attached to the GitHub Release, so
        # Nix just fetches and unpacks it. No `bun install` in the sandbox (which
        # would need a fragile, platform-varying fixed-output hash) and no bun in
        # the eval closure. Fill `webBundleHash` after a release, the same way as
        # the prebuilt-binary hashes above.
        karamd-web-bundle =
          final.runCommand "karamd-web-bundle-${version}"
            {
              src = final.fetchurl {
                url = "https://github.com/PatrickLerner/karamd/releases/download/v${version}/karamd-web-dist.tar.gz";
                hash = "sha256-PEvhYaSjm2c+80wwcRL+8GRkWFoG6QEaQW75FxBlFzE=";
              };
              nativeBuildInputs = [ final.gnutar final.gzip ];
            }
            ''
              mkdir -p $out
              tar -xzf $src -C $out
            '';

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
