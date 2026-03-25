{
  description = "hh - A Go TUI application";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        
        # Get Go version from go.mod (1.25.4)
        goVersion = "1.24"; # Using 1.24 as 1.25 is not yet available in nixpkgs
        
        go = pkgs."go_${builtins.replaceStrings ["."] ["_"] goVersion}" or pkgs.go;
      in
      {
        packages = {
          default = pkgs.buildGoModule {
            pname = "hh";
            version = "0.0.1";
            
            src = ./.;

            # Only build/install the root command, exclude binaries under cmd/
            subPackages = [ "." ];
            
            vendorHash = "sha256-8kfClqMS7Nfq1jfzd2FjEMi+mnzS+LXZ984kqjtxqWE=";
            
            # Use the Go version specified
            inherit go;
            
            # If you have CGO dependencies, uncomment the following:
            # CGO_ENABLED = "1";
            
            # Build flags if needed
            # ldflags = [ "-s" "-w" ];
            
            meta = with pkgs.lib; {
              description = "A Go TUI application";
              homepage = "https://github.com/liznear/hh";
              license = licenses.mit;
              maintainers = [ ];
              mainProgram = "hh";
            };
          };
        };
        
        # Development shell
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            go
            gopls
            gotools
            go-tools
            delve
          ];
          
          shellHook = ''
            echo "Go development environment"
            go version
          '';
        };
        
        # App for easy running
        apps.default = flake-utils.lib.mkApp {
          drv = self.packages.${system}.default;
        };
      }
    );
}
