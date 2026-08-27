{
  description = "Vireo is a clean, fast, GNOME-native email client with a calm three-pane layout, unified inbox, and support for OAuth accounts. Fast to open, effortless to read, and private by default.";

  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts";
    make-shell.url = "github:nicknovitski/make-shell";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-flake.url = "github:juspay/rust-flake";
  };

  outputs = inputs @ {flake-parts, ...}:
    flake-parts.lib.mkFlake {inherit inputs;} {
      imports = [
        inputs.rust-flake.flakeModules.default
        inputs.rust-flake.flakeModules.nixpkgs
        inputs.make-shell.flakeModules.default
      ];
      systems = ["x86_64-linux" "aarch64-linux" "aarch64-darwin"];
      perSystem = {
        config,
        lib,
        self',
        inputs',
        pkgs,
        system,
        ...
      }: {
        rust-project.toolchain = pkgs.rust-bin.stable.latest.default;
        rust-project.src = lib.cleanSource ./.;
        rust-project.crates.vireo.crane.args = {
          buildInputs = with pkgs; [
            cairo
            dbus
            glib
            gtk4
            libadwaita
            openssl
            pango
            pkg-config
            poppler
            webkitgtk_6_0
          ];
          nativeBuildInputs = with pkgs; [(lib.getDev glib) pkg-config];
        };
        make-shells.default.inputsFrom = [self'.packages.vireo];
        packages.default = self'.packages.vireo;

        apps.vireo.program = "${self'.packages.vireo}/bin/vireo";
        apps.default = self'.apps.vireo;
      };
      flake = {
      };
    };
}
