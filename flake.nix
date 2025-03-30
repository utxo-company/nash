{
  description = "Nash";

  nixConfig = {
    allow-import-from-derivation = "true";
    bash-prompt = "[nash \\w] $ ";
    cores = "1";
    max-jobs = "auto";
    auto-optimise-store = "true";
  };

  inputs = {
    # binary cache doesn't work for now :(
    nixpkgs.follows = "haskell-nix/nixpkgs";

    flake-parts.url = "github:hercules-ci/flake-parts";

    haskell-nix.url = "github:input-output-hk/haskell.nix";
    iohk-nix.url = "github:input-output-hk/iohk-nix";
    iohk-nix.inputs.nixpkgs.follows = "haskell-nix/nixpkgs";

    CHaP = {
      url = "github:intersectmbo/cardano-haskell-packages?ref=repo";
      flake = false;
    };

    pre-commit-hooks.url = "github:cachix/pre-commit-hooks.nix";
  };

  outputs = inputs@{ flake-parts, nixpkgs, haskell-nix, iohk-nix, CHaP, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.pre-commit-hooks.flakeModule
      ];
      debug = true;
      systems = [ "x86_64-linux" "aarch64-darwin" "x86_64-darwin" "aarch64-linux" ];

      perSystem = { config, system, ... }:
        let
          pkgs =
            import haskell-nix.inputs.nixpkgs {
              inherit system;
              overlays = [
                haskell-nix.overlay
                iohk-nix.overlays.crypto
                iohk-nix.overlays.haskell-nix-crypto
              ];
              inherit (haskell-nix) config;
            };

          project = pkgs.haskell-nix.cabalProject' {
            src = ./.;
            compiler-nix-name = "ghc9101";
            index-state = "2025-03-28T15:38:37Z";
            inputMap = {
              "https://chap.intersectmbo.org/" = CHaP;
            };
            shell = {
              withHoogle = true;
              withHaddock = true;
              exactDeps = false;
              shellHook = config.pre-commit.installationScript;
              nativeBuildInputs = with pkgs; [
                # Add tools here
              ];
              tools = {
                cabal = { };
                haskell-language-server = { };
                hlint = { };
                fourmolu = { };
              };
            };
          };
          flake = project.flake { };
        in
        {
          inherit (flake) devShells;
          packages = flake.packages // {
            # Add other package here when needed
          };

          inherit (flake) checks;

          pre-commit = {
            settings = {
              src = ./.;

              hooks = {
                nixpkgs-fmt.enable = true;
                statix.enable = true;
                deadnix.enable = true;

                cabal-fmt.enable = true;
                fourmolu = {
                  enable = true;
                  excludes = [ ];
                };
                ormolu = {
                  settings.cabalDefaultExtensions = true;
                };
                hlint.enable = true;

                typos = {
                  enable = true;
                  excludes = [ "\.golden" "fourmolu.yaml" ];
                };

                yamllint.enable = true;
              };
            };
          };
        };
    };
}
