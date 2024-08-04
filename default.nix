{ pkgs ? import
    (fetchTarball {
      name = "jpetrucciani-2024-08-01";
      url = "https://github.com/jpetrucciani/nix/archive/f156bb320d9154892562442cba05ca61101974a4.tar.gz";
      sha256 = "0yjwbvdajhk2msvy1hcfb74ip3ccx9ipdc8l6p9xpvzqh2hsmzyd";
    })
    { }
}:
let
  name = "vercel-log-drain";

  tools = with pkgs; {
    cli = [
      coreutils
      nixpkgs-fmt
    ];
    rust = [
      cargo
      clang
      rust-analyzer
      rustc
      rustfmt

      # deps
      pkg-config
      openssl
    ];
    scripts = pkgs.lib.attrsets.attrValues scripts;
  };

  drv = pkgs.rustPlatform.buildRustPackage {
    pname = "vercel-log-drain";
    version = "0.0.2";
    src = ./.;
    buildInputs = with pkgs; [ openssl ];
    nativeBuildInputs = with pkgs; [ pkg-config ];
    cargoHash = "sha256-l5H1tjGRzUlwSrkKPdhFVRba49XsFVI9svhMiIWnrVQ=";
    # RUSTFLAGS = "-C target-feature=+crt-static";
  };

  scripts = with pkgs; let
    inherit (writers) writeBashBin;
    fake_secret = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
  in
  {
    test_drain = writeBashBin "test_drain" ''
      payload="$1"
      ${curl}/bin/curl --data-binary "@$payload" -H "x-vercel-signature: $(cat ./$payload |\
        ${openssl}/bin/openssl dgst -sha1 -hmac '${fake_secret}' |\
        ${coreutils}/bin/cut -d' ' -f2)" -XPOST http://localhost:8000/vercel
    '';
    run = writeBashBin "run" ''
      ./target/debug/vercel-log-drain --enable-metrics --vercel-secret ${fake_secret} --vercel-verify verify --log DEBUG "$@"
    '';
  };
  paths = pkgs.lib.flatten [ (builtins.attrValues tools) ];
  env = pkgs.buildEnv {
    inherit name paths; buildInputs = paths;
  };
in
(env.overrideAttrs (_: {
  inherit name;
  NIXUP = "0.0.7";
})) // { inherit scripts drv; }
