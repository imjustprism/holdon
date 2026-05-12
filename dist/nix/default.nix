{
  lib,
  rustPlatform,
  fetchFromGitHub,
  installShellFiles,
  pkg-config,
  stdenv,
}:
rustPlatform.buildRustPackage rec {
  pname = "holdon";
  version = "0.2.0";

  src = fetchFromGitHub {
    owner = "imjustprism";
    repo = "holdon";
    rev = "v${version}";
    hash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
  };

  cargoHash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

  nativeBuildInputs = [installShellFiles pkg-config];

  buildFeatures = ["full"];

  postInstall = ''
    installShellCompletion --cmd holdon \
      --bash <($out/bin/holdon --generate-completion bash) \
      --fish <($out/bin/holdon --generate-completion fish) \
      --zsh  <($out/bin/holdon --generate-completion zsh)
    $out/bin/holdon --generate-manpage > holdon.1
    installManPage holdon.1
  '';

  meta = with lib; {
    description = "Wait for anything. Know why if it doesn't.";
    homepage = "https://github.com/imjustprism/holdon";
    changelog = "https://github.com/imjustprism/holdon/blob/v${version}/CHANGELOG.md";
    license = with licenses; [mit asl20];
    mainProgram = "holdon";
    maintainers = [];
  };
}
