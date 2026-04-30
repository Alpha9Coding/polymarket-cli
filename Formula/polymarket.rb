class Polymarket < Formula
  desc "CLI for Polymarket — browse markets, trade, and manage positions"
  homepage "https://github.com/Alpha9Coding/polymarket-cli"
  version "0.2.1"
  license "MIT"

  on_macos do
    on_intel do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "81fe14fbf8dccd63c3f1f5ef0a390ab2e5547cdc96047aec0143fdd97095baaa"
    end

    on_arm do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "ca1abcf24c2ae282078fa9028ef51c1d16564ff1a2cfc7d0681d27f9faaa2652"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "6b5efaf5dd045161b17e9193df02f4a98807ef745e48f933ddb7dc4071cc8bbe"
    end

    on_arm do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "95a7f8b058ec1aad4f8b2a9358dd4a0dcf543f834d3e635189013debbc9e8178"
    end
  end

  def install
    bin.install "polymarket"
  end

  test do
    assert_match "polymarket", shell_output("#{bin}/polymarket --version")
  end
end
