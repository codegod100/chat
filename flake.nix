{
  description = "Chat — simple Vidya LLM client (keys from OpenBao)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    vidya = {
      url = "git+https://tangled.org/nandi.uk/vidya";
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

            nativeBuildInputs = [ pkgs.makeWrapper ];
            buildInputs = libs;

            postInstall = ''
              wrapProgram $out/bin/chat \
                --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath libs}

              install -Dm644 data/share/applications/uk.nandi.chat.desktop \
                $out/share/applications/uk.nandi.chat.desktop
              substituteInPlace $out/share/applications/uk.nandi.chat.desktop \
                --replace-fail 'Exec=chat' "Exec=$out/bin/chat"
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
          ];

          stageXdg = ''
            xdg="$PWD/target/xdg-data"
            mkdir -p "$xdg/applications"
            desktop="$xdg/applications/uk.nandi.chat.desktop"
            install -m 644 "${desktopTemplate}" "$desktop"
            install -m 644 "$desktop" "$PWD/target/uk.nandi.chat.desktop"
            export XDG_DATA_DIRS="$xdg''${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
          '';

          build = pkgs.writeShellApplication {
            name = "chat-build";
            runtimeInputs = cargoTools;
            text = ''
              ${cargoPreamble libPath}
              ${stageXdg}
              echo "→ cargo build $*"
              cargo build "$@"
              bin="$PWD/target/debug/chat"
              if [ -x "$PWD/target/release/chat" ] && printf '%s\n' "$*" | grep -q -- '--release'; then
                bin="$PWD/target/release/chat"
              fi
              if [ -x "$bin" ]; then
                bin_esc=''${bin//\\/\\\\}
                bin_esc=''${bin_esc//&/\\&}
                sed -i -e "s|^Exec=.*|Exec=$bin_esc|" \
                  "$PWD/target/xdg-data/applications/uk.nandi.chat.desktop" \
                  "$PWD/target/uk.nandi.chat.desktop"
              fi
              echo "✓ $bin"
            '';
          };

          chatApp = pkgs.writeShellApplication {
            name = "chat";
            runtimeInputs = cargoTools;
            text = ''
              ${cargoPreamble libPath}
              ${stageXdg}

              bin="$PWD/target/debug/chat"
              bin_esc=''${bin//\\/\\\\}
              bin_esc=''${bin_esc//&/\\&}
              sed -i -e "s|^Exec=.*|Exec=$bin_esc|" \
                "$PWD/target/xdg-data/applications/uk.nandi.chat.desktop" \
                "$PWD/target/uk.nandi.chat.desktop"

              echo "→ cargo run $*"
              echo "    app_id=uk.nandi.chat  XDG_DATA_DIRS=$PWD/target/xdg-data:…"
              exec cargo run -- "$@"
            '';
          };
        in
        {
          default = {
            type = "app";
            program = "${chatApp}/bin/chat";
          };
          chat = {
            type = "app";
            program = "${chatApp}/bin/chat";
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
            ];
            buildInputs = libs;
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath libs;
            RUST_BACKTRACE = "1";
            shellHook = ''
              echo "Chat dev shell"
              echo "  nix run / nix run .#chat   # cargo run"
              echo "  nix run .#build            # cargo build"
              echo "  cargo run                  # from this shell"
            '';
          };
        }
      );

      formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixfmt-rfc-style);
    };
}
