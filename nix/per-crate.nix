{ pkgs, craneLib, src, features ? [ "pdf" ] }:
let
  inherit (pkgs) lib;
  manifest = builtins.fromTOML (builtins.readFile ../Cargo.toml);
  tools = import "${pkgs.crate2nix.src}/tools.nix" { inherit pkgs; };
  gitHashes = (tools.internal.vendorSupport {
    crateDir = src;
    lockFiles = [ "${src}/Cargo.lock" ];
  }).extendedHashes;
  vendor = craneLib.vendorCargoDeps { inherit src; };
  generated = pkgs.runCommand "jcode-cargo-nix" {
    nativeBuildInputs = [ pkgs.cargo pkgs.crate2nix ];
    CARGO_BUILD_RUSTC_WRAPPER = "";
    CARGO_NET_OFFLINE = "true";
  } ''
    cp -R ${src} source
    chmod -R u+w source
    cd source
    export CARGO_HOME="$TMPDIR/cargo"
    mkdir -p "$CARGO_HOME" "$out"
    cp ${vendor}/config.toml "$CARGO_HOME/config.toml"
    echo '${builtins.toJSON gitHashes}' > crate-hashes.json
    crate2nix generate -o Cargo.nix -h crate-hashes.json
    cp Cargo.nix "$out/default.nix"
  '';
  scopedSource = name: paths: lib.cleanSourceWith {
    # Filter the realized tree, not cleanSourceWith's original unfiltered root.
    src = "${src}";
    name = "${name}-source";
    filter = path: _:
      let relative = lib.removePrefix "${src}/" (toString path);
      in lib.any (selected:
        relative == selected
        || lib.hasPrefix "${selected}/" relative
        || lib.hasPrefix "${relative}/" selected
      ) paths;
  };
  memberOverrides = builtins.listToAttrs (map (member:
    let
      name = (builtins.fromTOML (builtins.readFile "${src}/${member}/Cargo.toml")).package.name;
      extra = lib.optionals (name == "jcode-build-meta") [ "Cargo.toml" ]
        ++ lib.optionals (name == "jcode-docs-data") [ "docs" "README.md" ]
        ++ lib.optionals (name == "jcode-base") [ "docs/kagi-web-search.md" ];
    in {
      inherit name;
      value = _: {
        src = scopedSource name ([ member ] ++ extra);
        sourceRoot = "${name}-source/${member}";
      };
    }
  ) (builtins.filter (member: member != ".") manifest.workspace.members));
  crates = import generated {
    inherit pkgs;
    buildRustCrateForPkgs = buildPkgs: lib.makeOverridable
      ({ defaultCrateOverrides ? buildPkgs.defaultCrateOverrides }: args:
        (buildPkgs.buildRustCrate.override {
          inherit defaultCrateOverrides;
          defaultCodegenUnits = 256;
        }) (args // { extraRustcOpts = [ "-C" "opt-level=1" ]; })
      ) { };
    rootFeatures = features;
    defaultCrateOverrides = pkgs.defaultCrateOverrides // memberOverrides // {
      crossterm = _: { src = scopedSource "crossterm" [ "vendor/crossterm" ]; sourceRoot = "crossterm-source/vendor/crossterm"; };
      jcode-build-meta = attrs: (memberOverrides.jcode-build-meta attrs) // {
        JCODE_BUILD_SEMVER = manifest.package.version;
        # A changing revision here would invalidate almost every workspace crate.
        JCODE_BUILD_GIT_HASH = "nix";
        JCODE_BUILD_GIT_DIRTY = "0";
        JCODE_RELEASE_BUILD = "1";
      };
      jcode = _: {
        src = scopedSource "jcode" [ "Cargo.toml" "src" "assets" ];
        crateBin = [ { name = "jcode"; path = "src/main.rs"; } ];
      };
    };
  };
in
pkgs.symlinkJoin {
  name = "jcode-per-crate-${manifest.package.version}";
  paths = [ crates.workspaceMembers.jcode.build ];
  nativeBuildInputs = [ pkgs.makeWrapper ];
  postBuild = ''
    wrapProgram "$out/bin/jcode" --set JCODE_NO_AUTO_UPDATE 1
  '';
  passthru = { inherit crates generated; };
  meta = {
    description = "Jcode with experimental per-crate Nix caching";
    mainProgram = "jcode";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux ++ lib.platforms.darwin;
  };
}
