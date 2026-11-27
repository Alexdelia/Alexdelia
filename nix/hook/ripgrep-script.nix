{ pkgs }:
name:
pkgs.writeShellApplication {
  inherit name;
  runtimeInputs = [
    pkgs.coreutils
    pkgs.ripgrep
  ];
  text = builtins.readFile ./${name}.bash;
}
