{ pkgs, lib, config, inputs, ... }:
{
  # https://devenv.sh/packages/
  packages = with pkgs; [ ];

  # https://devenv.sh/languages/
  languages.rust.enable = true;

  enterTest = ''
    cargo nextest run --locked --no-fail-fast
  '';
}
