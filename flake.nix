{
  description = "Flutter with required native libraries";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/25.05";
    nixpkgs-unstable.url = "github:NixOS/nixpkgs/master";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      nixpkgs-unstable,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          config = {
            android_sdk.accept_license = true;
            allowUnfree = true;
          };
        };
        unstable = import nixpkgs-unstable {
          inherit system;
          config = {
            android_sdk.accept_license = true;
            allowUnfree = true;
          };
        };
      in
      {
        devShell =
          with pkgs;
          mkShell {
            buildInputs = [
              rustup
              pkg-config
              unstable.python3
              unstable.ollama
              unstable.python313Packages.ollama
              unstable.python313Packages.llama-index-llms-ollama
              unstable.basedpyright
            ];
          };
      }
    );
}
