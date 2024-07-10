self: {
  config,
  pkgs,
  lib,
  ...
}:
with lib; let
  cfg = config.programs.clipshare;
  defaultPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
in {
  options.programs.clipshare = with types; {
    enable = mkEnableOption "Whether or not to enable clipshare.";
    package = mkOption {
      type = with types; nullOr package;
      default = defaultPackage;
      defaultText = literalExpression "inputs.clipshare.packages.${pkgs.stdenv.hostPlatform.system}.default";
      description = ''
        The clipshare package to use.

        By default, this option will use the `packages.default` as exposed by this flake.
      '';
    };
    systemd = mkOption {
      type = types.bool;
      default = pkgs.stdenv.isLinux;
      description = "Whether to enable to systemd service for clipshare on linux.";
    };
    port = lib.mkOption {
      type = types.port;
      default = 35713;
      example = 35713;
      description = ''
        clipshare server port
      '';
    };
  };

  config = mkIf cfg.enable {
    systemd.user.services.clipshare = lib.mkIf cfg.systemd {
      Unit = {
        Description = "Systemd service for Clipshare";
        Requires = ["graphical-session.target"];
      };
      Service = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/clipshare --port ${cfg.port}";
        Restart = "on-failure";
      };
      Install.WantedBy = [
        (lib.mkIf config.wayland.windowManager.hyprland.systemd.enable "hyprland-session.target")
        (lib.mkIf config.wayland.windowManager.sway.systemd.enable "sway-session.target")
      ];
    };

    home.packages = [
      cfg.package
    ];
  };
}
