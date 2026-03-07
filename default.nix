{
  pkgs ? import <nixpkgs> { },
}:

pkgs.rustPlatform.buildRustPackage (finalAttrs: {
  pname = "dijkstra";
  version = "0.0.1";

  src = pkgs.lib.cleanSource ./.;

  cargoHash = "sha256-HcUZ+zkcbkKMcCLXMzP/i/o6z2/Xc/a87NLa+oNqB5k=";
  nativeBuildInputs = with pkgs; [
    perl
    openssl
  ];
  meta = {
    description = "Mildly competent telegram bot";
    homepage = "https://github.com/fmeef/dijkstra";
    license = pkgs.lib.licenses.agpl3Plus;
    maintainers = [ ];
  };
})
