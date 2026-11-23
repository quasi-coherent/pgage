system := `nix-instantiate --raw --strict --eval -E builtins.currentSystem`

reload:
    @nix-direnv-reload 2>/dev/null

chkk *a:
    nix flake check {{a}}
