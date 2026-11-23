{ pkgs }:
name:
pkgs.writeShellApplication {
  inherit name;
  runtimeInputs = [ pkgs.ripgrep ];
  text = builtins.readFile ./${name}.bash;
}
