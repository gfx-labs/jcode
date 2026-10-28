{
  description = "Jcode packages and Rust development environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  inputs.crane.url = "github:ipetkov/crane";

  outputs = { self, nixpkgs, crane, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          source = pkgs.lib.cleanSourceWith {
            src = self;
            name = "jcode-source";
            filter = path: _:
              let
                relative = pkgs.lib.removePrefix "${self}/" (toString path);
                root = builtins.head (pkgs.lib.splitString "/" relative);
              in
              builtins.elem root [
                "Cargo.toml" "Cargo.lock" "README.md" "LICENSE" "LICENSE.md"
                ".cargo" "src" "crates" "vendor" "assets" "docs"
              ];
          };
          jcode = pkgs.callPackage ./nix/package.nix {
            craneLib = crane.mkLib pkgs;
            src = source;
            revision = self.shortRev or (self.dirtyShortRev or "unknown");
          };
        in
        {
          default = jcode;
          inherit jcode;
          jcode-minimal = jcode.override { features = [ ]; };
          jcode-bedrock = jcode.override { features = [ "pdf" "bedrock" ]; };
        });

      apps = forAllSystems (system: {
        default = self.apps.${system}.jcode;
        jcode = {
          type = "app";
          program = "${self.packages.${system}.jcode}/bin/jcode";
          meta.description = "Jcode coding agent";
        };
      });

      checks = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          package = self.packages.${system}.jcode;
        in
        {
          jcode-cli = pkgs.runCommand "jcode-cli-check" { } ''
            export HOME="$TMPDIR/home"
            mkdir -p "$HOME"
            ${package}/bin/jcode --version > "$out"
            ${package}/bin/jcode --help > /dev/null
            test -s "$out"
          '';
        });

      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            # mkShell supplies the platform C/C++ compiler through stdenv.
            packages = with pkgs; [
              cargo
              rustc
              rustfmt
              clippy
              rust-analyzer
              pkg-config
              cmake
              gnumake
              perl
              git
              ripgrep
            ];

            # macOS voice support generates CoreAudio bindings with libclang.
            nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
              pkgs.rustPlatform.bindgenHook
            ];

            # Native TLS dependencies are used by parts of the provider stack.
            buildInputs = [ pkgs.openssl ];
          };
        });
    };
}
