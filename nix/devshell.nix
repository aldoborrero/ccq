{
  pkgs,
  inputs,
  system,
  ...
}:
let
  fenixPkgs = inputs.fenix.packages.${system};
  rustToolchain = fenixPkgs.complete.withComponents [
    "cargo"
    "rustc"
    "rust-src"
    "rust-analyzer"
    "clippy"
    "rustfmt"
  ];
in
pkgs.mkShellNoCC {
  packages = [
    rustToolchain
    pkgs.pkg-config
    pkgs.vhs
  ];

  shellHook = ''
    export PRJ_ROOT=$PWD
  '';
}
