class Polymarket < Formula
  desc "CLI for Polymarket — browse markets, trade, and manage positions"
  homepage "https://github.com/Alpha9Coding/polymarket-cli"
  version "0.6.0"
  license "MIT"

  on_macos do
    on_intel do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "ff0bdd0bfa6f711849399c843cf791c59f47932d43901f9f6b710fe45460622c"
    end

    on_arm do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "2ee0799ea0c450cf8a0cb474817966c31c6686d3c1ba9b57911fe830d881f652"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "ef5c2f8386f34cdecba0abf11e7d728e1f52702bbc5ff702de479a4b741e51ef"
    end

    on_arm do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "14fc420f58efa4e6aadcf81cce0cb2b05d93574a7659c3b76d9ffa02dd8f00cc"
    end
  end

  def install
    bin.install "polymarket"
  end

  test do
    assert_match "polymarket", shell_output("#{bin}/polymarket --version")
  end
end
