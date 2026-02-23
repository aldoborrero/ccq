{
  pkgs,
  inputs,
  system,
  ...
}:
let
  fenix = inputs.fenix.packages.${system};
  toolchain = fenix.complete.withComponents [
    "cargo"
    "rustc"
    "rust-src"
  ];

  craneLib = (inputs.crane.mkLib pkgs).overrideToolchain toolchain;

  src = craneLib.cleanCargoSource ../../..;

  commonArgs = {
    inherit src;
    strictDeps = true;
    pname = "ccq";
    version = "0.1.0";
    nativeBuildInputs = [ pkgs.pkg-config ];
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
craneLib.buildPackage (
  commonArgs
  // {
    inherit cargoArtifacts;
  }
)
