{
  description = "Flutter with required native libraries";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/25.05";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
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
      in
      {

        devShell =
          with pkgs; mkShell {

            baseInputs = [
              rustc
              cargo
              pkg-config
            ];
          };
      }
    );
}
