# Example NixOS host configuration for the jellespelletjes API.
#
# In your host flake:
#   inputs.jellespelletjes-api.url = "github:pingiun/jellespelletjes-api";
# and add to the host's modules:
#   inputs.jellespelletjes-api.nixosModules.default
#   ./this-file.nix
#
# Deploying a new API version = `nix flake update jellespelletjes-api`
# followed by `nixos-rebuild switch`.
{ config, ... }:
{
  services.jellespelletjes-api = {
    enable = true;
    # RESEND_API_KEY=... ; root-owned, mode 0600. Or manage with agenix/sops-nix.
    environmentFile = "/var/lib/jellespelletjes-api/secrets.env";
  };

  # TLS-terminating reverse proxy.
  services.caddy = {
    enable = true;
    virtualHosts."api.jellespelletjes.nl".extraConfig = ''
      reverse_proxy 127.0.0.1:8080
    '';
  };

  # Continuous SQLite replication to R2/S3-compatible storage.
  services.litestream = {
    enable = true;
    settings.dbs = [{
      path = "/var/lib/jellespelletjes-api/app.db";
      replicas = [{
        url = "s3://jellespelletjes-backup.<accountid>.r2.cloudflarestorage.com/api-db";
      }];
    }];
    # LITESTREAM_ACCESS_KEY_ID / LITESTREAM_SECRET_ACCESS_KEY
    environmentFile = "/var/lib/jellespelletjes-api/litestream.env";
  };
  # Litestream must read the database written by the API service.
  users.users.litestream.extraGroups = [ "jellespelletjes-api" ];

  networking.firewall.allowedTCPPorts = [ 80 443 ];
}
