{
  description = "Devshell for dijkstra bot framework";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/25.05";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs =
    {
      self,
      nixpkgs,
      flake-utils
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          config = {
            allowUnfree = true;
          };
        };
      in
      {
        devShell =
          with pkgs;
          mkShell {
            baseInputs = [
              rustup
              openssl
              fish
              jdk
              rust-analyzer
              nil
              nixd
              zed-editor
            ];
          };
      }
    );
}
