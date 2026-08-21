{
  description = "devgar/tools - Nix packages";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      manifest = builtins.fromTOML (builtins.readFile ./apps/ewwkit/Cargo.toml);
    in {
      packages.${system} = rec {
        ewwkit = pkgs.pkgsStatic.rustPlatform.buildRustPackage {
          pname = "ewwkit";
          version = manifest.package.version;
          src = ./apps/ewwkit;
          cargoLock.lockFile = ./apps/ewwkit/Cargo.lock;
        };

        # Python rather than Rust, against the house preference: the earbud
        # protocol needs raw AF_BLUETOOTH sockets, which CPython has in its
        # stdlib with no dependencies at all. See apps/budskit/README.md.
        budskit = pkgs.stdenvNoCC.mkDerivation {
          pname = "budskit";
          version = "0.1.0";
          src = ./apps/budskit;
          dontBuild = true;
          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin
            # cmfbuds.py sits beside the scripts because they resolve their own
            # symlink and import it from that directory.
            install -Dm644 bin/cmfbuds.py $out/bin/cmfbuds.py
            for t in cmf-buds cmf-budsd sdp-services bt-reconnect; do
              install -Dm755 bin/$t $out/bin/$t
            done
            substituteInPlace $out/bin/cmf-buds $out/bin/cmf-budsd $out/bin/sdp-services \
              --replace-fail '#!/usr/bin/python3' '#!${pkgs.python3}/bin/python3'
            runHook postInstall
          '';
          meta = {
            description = "Per-component battery and reconnect tooling for CMF/Nothing earbuds";
            mainProgram = "cmf-buds";
            platforms = pkgs.lib.platforms.linux;
          };
        };

        default = ewwkit;
      };
    };
}
