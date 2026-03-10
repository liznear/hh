{
  description = "hh: terminal UI coding assistant";

  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.1"; # unstable Nixpkgs
    crane.url = "github:ipetkov/crane";
    fenix = {
      url = "https://flakehub.com/f/nix-community/fenix/0.1";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { self, ... }@inputs:

    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forEachSupportedSystem = f: inputs.nixpkgs.lib.genAttrs supportedSystems (system: f system);

      mkPkgs =
        system:
        import inputs.nixpkgs {
          inherit system;
          overlays = [
            inputs.self.overlays.default
          ];
        };
    in
    {
      overlays.default = final: prev: {
        rustToolchain =
          with inputs.fenix.packages.${prev.stdenv.hostPlatform.system};
          combine (
            with stable;
            [
              clippy
              rustc
              cargo
              rustfmt
              rust-src
            ]
          );
      };

      packages = forEachSupportedSystem (
        system:
        let
          pkgs = mkPkgs system;
          craneLib = (inputs.crane.mkLib pkgs).overrideToolchain (_: pkgs.rustToolchain);
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              (craneLib.filterCargoSources path type)
              || pkgs.lib.hasInfix "/src/core/prompts/" (toString path);
          };

          commonArgs = {
            inherit src;
            strictDeps = true;
            nativeBuildInputs = with pkgs; [
              pkg-config
              makeWrapper
              ripgrep
            ];
            buildInputs = with pkgs; [
              openssl
            ];
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          hh = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              postInstall = ''
                wrapProgram "$out/bin/hh" --prefix PATH : "${pkgs.lib.makeBinPath [ pkgs.ripgrep ]}"
              '';
            }
          );
        in
        {
          default = hh;
          hh = hh;
        }
      );

      apps = forEachSupportedSystem (
        system:
        let
          pkgs = mkPkgs system;
        in
        {
          default = {
            type = "app";
            program = "${pkgs.lib.getExe inputs.self.packages.${system}.default}";
          };

          hh = {
            type = "app";
            program = "${pkgs.lib.getExe inputs.self.packages.${system}.hh}";
          };
        }
      );

      devShells = forEachSupportedSystem (
        system:
        let
          pkgs = mkPkgs system;
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              rustToolchain
              openssl
              pkg-config
              cargo-deny
              cargo-edit
              cargo-watch
              rust-analyzer
              python3
            ];

            env = {
              # Required by rust-analyzer
              RUST_SRC_PATH = "${pkgs.rustToolchain}/lib/rustlib/src/rust/library";
            };
          };
        }
      );
    };
}
