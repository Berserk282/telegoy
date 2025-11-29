{
  pkgs,
  lib,
  config,
  inputs,
  ...
}:

{
  dotenv.enable = true;
  languages.rust = {
    enable = true;
    channel = "stable";
    mold.enable = true;
  };
  packages = with pkgs; [
    lldb
    cargo-watch
    zed-editor
    vscode-json-languageserver
  ];
  enterShell = ''
    export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig"
  '';
}
