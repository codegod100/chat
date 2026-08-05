{
  description = "Chat — simple Vidya LLM client (keys from OpenBao)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    vidya = {
      url = "github:codegod100/vidya";
      flake = false;
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      vidya,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;

      eguiLibs =
        pkgs:
        with pkgs;
        [
          libxkbcommon
          libGL
          vulkan-loader
        ]
        ++ lib.optionals stdenv.hostPlatform.isLinux [
          wayland
          libx11
          libxcursor
          libxi
          libxrandr
        ];

      cargoPreamble = libPath: ''
        set -euo pipefail
        export LD_LIBRARY_PATH="${libPath}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

        if [ ! -f Cargo.toml ] && [ -f "''${FLAKE_ROOT:-}/Cargo.toml" ]; then
          cd "$FLAKE_ROOT"
        fi
        if [ ! -f Cargo.toml ]; then
          echo "chat: no Cargo.toml here (cwd=$PWD)" >&2
          echo "  cd into the chat checkout, then: nix run .#chat" >&2
          exit 1
        fi
        if [ ! -d ../vidya ]; then
          echo "chat: expected sibling ../vidya (Cargo path dep)" >&2
          echo "  clone vidya next to chat, or: nix build .#chat" >&2
          exit 1
        fi
      '';

      desktopTemplate = ./data/share/applications/uk.nandi.chat.desktop;
      iconSvg = ./assets/uk.nandi.chat.svg;

      # Install FreeDesktop entry + hicolor icons under a share root.
      # Wayland compositors (GNOME) look up icons by app_id via the *session*
      # data dirs — a temporary XDG_DATA_DIRS on the child alone is not enough.
      # Absolute Icon= is the reliable dock/overview path.
      # Shell fragment: $1=shareDir $2=execPath (absolute).
      installDesktopShareSh = ''
        _share="$1"
        _exec="$2"
        _icon="$_share/icons/hicolor/256x256/apps/uk.nandi.chat.png"
        _exec_esc=''${_exec//\\/\\\\}
        _exec_esc=''${_exec_esc//&/\\&}
        _icon_esc=''${_icon//\\/\\\\}
        _icon_esc=''${_icon_esc//&/\\&}
        mkdir -p "$_share/applications"
        mkdir -p "$_share/icons/hicolor/scalable/apps"
        install -m 644 "${desktopTemplate}" "$_share/applications/uk.nandi.chat.desktop"
        # install(1), not cp: nix-store sources are 0444 and a plain cp preserves
        # that mode, so the next run can't overwrite the installed SVG.
        install -m 644 "${iconSvg}" "$_share/icons/hicolor/scalable/apps/uk.nandi.chat.svg"
        for sz in 16 24 32 48 64 128 256; do
          mkdir -p "$_share/icons/hicolor/''${sz}x''${sz}/apps"
          rsvg-convert -w "$sz" -h "$sz" "${iconSvg}" \
            -o "$_share/icons/hicolor/''${sz}x''${sz}/apps/uk.nandi.chat.png"
          chmod u+w "$_share/icons/hicolor/''${sz}x''${sz}/apps/uk.nandi.chat.png" 2>/dev/null || true
        done
        sed -i \
          -e "s|^Exec=.*|Exec=$_exec_esc|" \
          -e "s|^Icon=.*|Icon=$_icon_esc|" \
          "$_share/applications/uk.nandi.chat.desktop"
        if command -v gtk-update-icon-cache >/dev/null 2>&1; then
          gtk-update-icon-cache -f "$_share/icons/hicolor" 2>/dev/null || true
        fi
        if command -v update-desktop-database >/dev/null 2>&1; then
          update-desktop-database "$_share/applications" 2>/dev/null || true
        fi
      '';
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          inherit (pkgs) lib;
          libs = eguiLibs pkgs;

          srcTree = pkgs.runCommand "chat-src" { } ''
            mkdir -p $out/chat $out/vidya
            cp -a ${lib.cleanSource ./.}/. $out/chat/
            cp -a ${vidya}/. $out/vidya/
            chmod -R u+w $out
            rm -rf $out/chat/{target,result,result-*,.git} 2>/dev/null || true
            rm -rf $out/vidya/{target,android-demo,host,examples,docs,.git} 2>/dev/null || true
          '';

          chat = pkgs.rustPlatform.buildRustPackage {
            pname = "chat";
            version = "0.1.0";
            src = srcTree;
            sourceRoot = "chat-src/chat";
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = [
              pkgs.makeWrapper
              pkgs.librsvg
            ];
            buildInputs = libs;

            postInstall = ''
              wrapProgram $out/bin/chat \
                --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath libs}

              install -Dm644 ${iconSvg} \
                $out/share/icons/hicolor/scalable/apps/uk.nandi.chat.svg
              for sz in 16 24 32 48 64 128 256; do
                mkdir -p $out/share/icons/hicolor/''${sz}x''${sz}/apps
                rsvg-convert -w "$sz" -h "$sz" ${iconSvg} \
                  -o $out/share/icons/hicolor/''${sz}x''${sz}/apps/uk.nandi.chat.png
              done

              install -Dm644 data/share/applications/uk.nandi.chat.desktop \
                $out/share/applications/uk.nandi.chat.desktop
              substituteInPlace $out/share/applications/uk.nandi.chat.desktop \
                --replace-fail 'Exec=chat' "Exec=$out/bin/chat" \
                --replace-fail 'Icon=uk.nandi.chat' \
                  "Icon=$out/share/icons/hicolor/256x256/apps/uk.nandi.chat.png"
            '';

            meta = {
              description = "Simple Vidya LLM chat with OpenBao keys";
              license = lib.licenses.mit;
              mainProgram = "chat";
              platforms = lib.platforms.linux;
            };
          };
        in
        {
          default = chat;
          chat = chat;
        }
      );

      apps = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          inherit (pkgs) lib;
          libs = eguiLibs pkgs;
          libPath = lib.makeLibraryPath libs;
          cargoTools = with pkgs; [
            rustc
            cargo
            pkg-config
            librsvg
            gtk3
            desktop-file-utils
          ];

          build = pkgs.writeShellApplication {
            name = "chat-build";
            runtimeInputs = cargoTools;
            text = ''
              ${cargoPreamble libPath}
              echo "→ cargo build $*"
              cargo build "$@"
              bin="$PWD/target/debug/chat"
              if [ -x "$PWD/target/release/chat" ] && printf '%s\n' "$*" | grep -q -- '--release'; then
                bin="$PWD/target/release/chat"
              fi
              echo "✓ $bin"
            '';
          };

          # Cargo build, install .desktop+icons into XDG_DATA_HOME (so GNOME/Wayland
          # can resolve Icon= by app_id), then gtk-launch.
          desktopApp = pkgs.writeShellApplication {
            name = "chat";
            runtimeInputs = cargoTools;
            text = ''
              ${cargoPreamble libPath}
              echo "→ cargo build"
              cargo build

              EXEC="$PWD/target/debug/chat"
              # Session-visible FreeDesktop tree (not a private target/xdg-data):
              # Wayland shells ignore child-only XDG_DATA_DIRS for dock icons.
              DATA_HOME="''${XDG_DATA_HOME:-$HOME/.local/share}"
              echo "→ install launcher+icon → $DATA_HOME ({applications,icons}/…)"
              (
                set -- "$DATA_HOME" "$EXEC"
                ${installDesktopShareSh}
              )
              # Mirror under target/ for inspection.
              (
                set -- "$PWD/target/xdg-data" "$EXEC"
                ${installDesktopShareSh}
              )
              install -m 644 "$DATA_HOME/applications/uk.nandi.chat.desktop" \
                "$PWD/target/uk.nandi.chat.desktop"

              export XDG_DATA_DIRS="$DATA_HOME''${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
              ICON="$DATA_HOME/icons/hicolor/256x256/apps/uk.nandi.chat.png"
              echo "→ gtk-launch uk.nandi.chat  (Icon=$ICON, Exec=$EXEC)"
              if command -v gtk-launch >/dev/null 2>&1; then
                exec gtk-launch uk.nandi.chat "$@"
              fi
              exec "$EXEC" "$@"
            '';
          };
        in
        {
          default = {
            type = "app";
            program = "${desktopApp}/bin/chat";
          };
          chat = {
            type = "app";
            program = "${desktopApp}/bin/chat";
          };
          build = {
            type = "app";
            program = "${build}/bin/chat-build";
          };
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          libs = eguiLibs pkgs;
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              rustc
              cargo
              rustfmt
              clippy
              rust-analyzer
              pkg-config
              glib
              gtk3
              librsvg
              desktop-file-utils
            ];
            buildInputs = libs;
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath libs;
            RUST_BACKTRACE = "1";
            shellHook = ''
              echo "Chat dev shell"
              echo "  nix run / nix run .#chat   # cargo build + gtk-launch .desktop"
              echo "  nix run .#build            # cargo build"
              echo "  cargo run                  # from this shell"
            '';
          };
        }
      );

      formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixfmt-rfc-style);
    };
}
