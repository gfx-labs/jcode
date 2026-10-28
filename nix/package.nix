{
  lib,
  stdenv,
  craneLib,
  rustPlatform,
  pkg-config,
  cmake,
  perl,
  openssl,
  makeWrapper,
  src,
  revision ? "unknown",
  features ? [ "pdf" ],
}:
let
  manifest = builtins.fromTOML (builtins.readFile ../Cargo.toml);
  crosstermSource = lib.cleanSourceWith {
    src = "${src}/vendor/crossterm";
    name = "jcode-crossterm";
  };
  commonArgs = {
    inherit src;
    pname = "jcode";
    version = manifest.package.version;
    strictDeps = true;
    cargoExtraArgs = lib.concatStringsSep " " (
      [ "--locked" "--package jcode" "--bin jcode" "--no-default-features" ]
      ++ lib.optionals (features != [ ]) [ "--features ${lib.concatStringsSep "," features}" ]
    );
    # Do not compile workspace tests, dev tools, or their dependencies for installation.
    doCheck = false;
    CARGO_BUILD_RUSTC_WRAPPER = "";
    CARGO_INCREMENTAL = "0";
    nativeBuildInputs = [ pkg-config cmake perl ]
      ++ lib.optionals stdenv.isDarwin [ rustPlatform.bindgenHook ];
    buildInputs = [ openssl ];
  };
  cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
    buildPhaseCargoCommand = "cargoWithProfile build ${commonArgs.cargoExtraArgs}";
    # Registry crates depend on this path patch's real API, not a workspace stub.
    extraDummyScript = ''
      rm -rf "$out/vendor/crossterm"
      mkdir -p "$out/vendor/crossterm"
      cp -R ${crosstermSource}/. "$out/vendor/crossterm/"
    '';
  });
in
craneLib.buildPackage (commonArgs // {
  inherit cargoArtifacts;
  nativeBuildInputs = commonArgs.nativeBuildInputs ++ [ makeWrapper ];
  # Keep changing revision metadata out of the dependency derivation.
  JCODE_BUILD_SEMVER = manifest.package.version;
  JCODE_BUILD_GIT_HASH = lib.removeSuffix "-dirty" revision;
  JCODE_BUILD_GIT_DIRTY = if lib.hasSuffix "-dirty" revision then "1" else "0";
  JCODE_RELEASE_BUILD = "1";
  postInstall = ''
    wrapProgram "$out/bin/jcode" --set JCODE_NO_AUTO_UPDATE 1
  '';
  passthru = { inherit cargoArtifacts features; };
  meta = {
    description = "Terminal coding agent";
    homepage = "https://github.com/1jehuang/jcode";
    license = lib.licenses.mit;
    mainProgram = "jcode";
    platforms = lib.platforms.linux ++ lib.platforms.darwin;
  };
})
