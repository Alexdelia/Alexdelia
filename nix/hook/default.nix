{
  git-hooks,
  toolchain,
  system,
  pkgs,
  src,
}:
let
  ripgrepScript = import ./ripgrep-script.nix { inherit pkgs; };
  policyHook = name: files: {
    enable = true;
    inherit name files;
    entry = "${ripgrepScript name}/bin/${name}";
  };
  fastHook = {
    rustfmt = {
      enable = true;
      packageOverrides = {
        cargo = toolchain;
        rustfmt = toolchain;
      };
    };
    nixfmt.enable = true;
    typos.enable = true;

    no-comment = policyHook "no-comment" "\\.(rs|nix|sh|bash|toml|yml|yaml)$";
    ascii-only = policyHook "ascii-only" "" // {
      types = [ "text" ];
    };
    no-lint-suppression = policyHook "no-lint-suppression" "\\.rs$";
    file-size = policyHook "file-size" "\\.rs$";

    deadnix = {
      enable = true;
      stages = [ "pre-push" ];
    };
    statix = {
      enable = true;
      stages = [ "pre-push" ];
    };

    clippy = {
      enable = true;
      packageOverrides = {
        cargo = toolchain;
        clippy = toolchain;
      };
      entry = pkgs.lib.mkForce "cargo clippy --all-targets -- --deny warnings";
      always_run = true;
      pass_filenames = false;
      stages = [ "pre-push" ];
    };
    nextest = {
      enable = true;
      name = "cargo nextest";
      entry = "cargo nextest run";
      always_run = true;
      pass_filenames = false;
      stages = [ "pre-push" ];
    };
    cargo-deny = {
      enable = true;
      name = "cargo deny";
      entry = "cargo deny check";
      files = "(\\.toml|Cargo\\.lock)$";
      pass_filenames = false;
      stages = [ "pre-push" ];
    };
    cargo-machete = {
      enable = true;
      name = "cargo machete";
      entry = "${pkgs.lib.getExe pkgs.cargo-machete}";
      files = "(\\.toml|Cargo\\.lock|\\.rs)$";
      pass_filenames = false;
      stages = [ "pre-push" ];
    };
  };
in
git-hooks.lib.${system}.run {
  inherit src;
  tools = {
    cargo = toolchain;
    rustfmt = toolchain;
    clippy = toolchain;
  };
  hooks = fastHook // {
    llvm-cov = {
      enable = true;
      name = "cargo llvm-cov";
      entry = "cargo llvm-cov nextest --fail-under-lines 100 --fail-under-functions 100 --fail-under-regions 100";
      always_run = true;
      pass_filenames = false;
      stages = [ "pre-push" ];
      after = builtins.attrNames fastHook;
    };
  };
}
