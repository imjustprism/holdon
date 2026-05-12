class Holdon < Formula
  desc "Wait for anything. Know why if it doesn't."
  homepage "https://github.com/imjustprism/holdon"
  version "0.2.0"
  license "MIT OR Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/imjustprism/holdon/releases/download/v0.2.0/holdon-aarch64-apple-darwin.tar.gz"
      sha256 "ac2424d3038659a7393f5c43038f8b49c6ffdf768023725600c62649e0743d4e"
    end
    on_intel do
      url "https://github.com/imjustprism/holdon/releases/download/v0.2.0/holdon-x86_64-apple-darwin.tar.gz"
      sha256 "7783d9d7ae7f29e57d482881e0ce3441ec4df8b000b247edab71e7caba4c0bdd"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/imjustprism/holdon/releases/download/v0.2.0/holdon-aarch64-unknown-linux-musl.tar.gz"
      sha256 "ef06f0efec6b17971f1a5a632958425d4d0896c4d50b62d91d0b06762f6699ac"
    end
    on_intel do
      url "https://github.com/imjustprism/holdon/releases/download/v0.2.0/holdon-x86_64-unknown-linux-musl.tar.gz"
      sha256 "6a340f26434d0d3c2a16ffed35a5b6754755eb7a5b37929cb6dffead1a2c59b0"
    end
  end

  def install
    bin.install "holdon"
    generate_completions_from_executable(bin/"holdon", "--generate-completion")
  end

  test do
    assert_match "holdon", shell_output("#{bin}/holdon --version")
  end
end
