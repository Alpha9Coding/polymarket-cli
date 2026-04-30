class Polymarket < Formula
  desc "CLI for Polymarket — browse markets, trade, and manage positions"
  homepage "https://github.com/Alpha9Coding/polymarket-cli"
  version "0.3.2"
  license "MIT"

  on_macos do
    on_intel do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "9964f6c151e92d86d3aa85d8811a2354d0f84508486c35e0817c46f015ea39bc"
    end

    on_arm do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "61208f37d963d632c2e56751dbfe266e69cc8ad5c6fb1ead3ee0c53507d9e80d"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "def946c1af1fae42ad5ac60912a191dbf6911c50f348cbd714b275979dfd3ae5"
    end

    on_arm do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "f91be774cf17f29861f02802054f3af030de50c6b793557f953861a47e627eff"
    end
  end

  def install
    bin.install "polymarket"
  end

  test do
    assert_match "polymarket", shell_output("#{bin}/polymarket --version")
  end
end
