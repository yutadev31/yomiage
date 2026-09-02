{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    nixpkgs-voicevox.url = "github:NixOS/nixpkgs/24e8d730ef4adac7727652e666cbf9dce4d21d03";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-voicevox,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        pkgs-voicevox = import nixpkgs-voicevox {
          inherit system;
          config.allowUnfree = true;
        };

        rust = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
          ];
        };

        nativeBuildInputs =
          with pkgs;
          [
            rust
            taplo
            cmake
          ]
          ++ [
            pkgs-voicevox.voicevox-engine
          ];

        buildInputs = with pkgs; [
        ];
      in
      {
        devShells.default = pkgs.mkShell {
          inherit nativeBuildInputs buildInputs;
        };
      }
    );
}
