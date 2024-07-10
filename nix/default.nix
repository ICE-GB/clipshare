{
  rustPlatform,
  lib,
  pkgs,
}: let
  cargoToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
  pname = cargoToml.package.name;
  version = cargoToml.package.version;
in
rustPlatform.buildRustPackage {
  pname = pname;
  version = version;

  nativeBuildInputs = with pkgs; [
    pkg-config
  ];

  buildInputs = with pkgs; [
  ];

  src = builtins.path {
    name = pname;
    path = lib.cleanSource ../.;
  };

  cargoLock.lockFile = ../Cargo.lock;

  # Set Environment Variables
  RUST_BACKTRACE = "full";

  meta = with lib; {
    description = "Share your clipboard between machines of your local network.";
    longDescription = ''
      Share your clipboard between machines of your local network.
    '';
    mainProgram = pname;
    platforms = platforms.all;
  };
}
