{
  description = "Accounts and score sync for woordle.nl and sudokudo.nl";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
  };

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        jellespelletjes-api = pkgs.rustPlatform.buildRustPackage {
          pname = "jellespelletjes-api";
          version = "0.1.0";
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
          # Tests run against in-memory SQLite; no network needed.
          meta = {
            description = "Accounts and score sync for woordle.nl and sudokudo.nl";
            mainProgram = "jellespelletjes-api";
          };
        };
        default = jellespelletjes-api;
      });

      nixosModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.services.jellespelletjes-api;
        in
        {
          options.services.jellespelletjes-api = {
            enable = lib.mkEnableOption "jellespelletjes API";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.system}.jellespelletjes-api;
              description = "Package to run.";
            };

            listenAddr = lib.mkOption {
              type = lib.types.str;
              default = "127.0.0.1:8080";
              description = "Address the API listens on (put a reverse proxy in front).";
            };

            hubUrl = lib.mkOption {
              type = lib.types.str;
              default = "https://jellespelletjes.nl";
              description = "Public URL of the hub site where magic links land.";
            };

            allowedOrigins = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [
                "https://jellespelletjes.nl"
                "https://sudokudo.nl"
                "https://woordle.nl"
              ];
              description = "Origins allowed for CORS and SSO.";
            };

            emailFrom = lib.mkOption {
              type = lib.types.str;
              default = "Jellespelletjes <login@jellespelletjes.nl>";
              description = "From address for auth email.";
            };

            environmentFile = lib.mkOption {
              type = lib.types.nullOr lib.types.path;
              default = null;
              description = ''
                Environment file with secrets (RESEND_API_KEY=...). Without it
                the server runs in dev mode and logs magic links instead of
                sending email.
              '';
            };
          };

          config = lib.mkIf cfg.enable {
            users.users.jellespelletjes-api = {
              isSystemUser = true;
              group = "jellespelletjes-api";
              home = "/var/lib/jellespelletjes-api";
            };
            users.groups.jellespelletjes-api = { };

            # The CLI (e.g. `jellespelletjes-api seed-sudoku`) available on the host.
            environment.systemPackages = [ cfg.package ];

            systemd.services.jellespelletjes-api = {
              description = "jellespelletjes API";
              wantedBy = [ "multi-user.target" ];
              after = [ "network-online.target" ];
              wants = [ "network-online.target" ];
              environment = {
                DATABASE_URL = "sqlite:///var/lib/jellespelletjes-api/app.db?mode=rwc";
                LISTEN_ADDR = cfg.listenAddr;
                PUBLIC_HUB_URL = cfg.hubUrl;
                ALLOWED_ORIGINS = lib.concatStringsSep "," cfg.allowedOrigins;
                EMAIL_FROM = cfg.emailFrom;
              };
              serviceConfig = {
                ExecStart = "${lib.getExe cfg.package} serve";
                User = "jellespelletjes-api";
                Group = "jellespelletjes-api";
                StateDirectory = "jellespelletjes-api";
                WorkingDirectory = "/var/lib/jellespelletjes-api";
                Restart = "always";
                RestartSec = 2;
                NoNewPrivileges = true;
                ProtectSystem = "strict";
                ProtectHome = true;
                PrivateTmp = true;
              } // lib.optionalAttrs (cfg.environmentFile != null) {
                EnvironmentFile = cfg.environmentFile;
              };
            };
          };
        };
    };
}
