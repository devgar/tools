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
        default = ewwkit;
      };
    };
}
