{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      pkgs = nixpkgs.legacyPackages."x86_64-linux";
    in
    {
      packages."x86_64-linux".default = pkgs.rustPlatform.buildRustPackage {
        name = "telegoy";
        src = ./.;
        buildInputs = [ pkgs.openssl ];
        nativeBuildInputs = with pkgs; [
          pkg-config
        ];
        cargoHash = "sha256-i876GaNNq4eI2udqyb0hd8KjbAaqcDY18iLwKUXXYFw=";
      };
    };
}
