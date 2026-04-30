class Polymarket < Formula
  desc "CLI for Polymarket — browse markets, trade, and manage positions"
  homepage "https://github.com/Alpha9Coding/polymarket-cli"
  version "0.3.1"
  license "MIT"

  on_macos do
    on_intel do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "60d0b4bcbf4b772eb534e3d1ef117ab4af70573ded65a4fdca4fa2f8a8e786d6"
    end

    on_arm do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "c336771a9f92870982caf41e35171a71343dcdd05d221f1b4ccc74057b11201d"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "5a7afc6d946178c8c2e74eeb2d1dccbc2a497959bdcb151321dc2378fb6fcc36"
    end

    on_arm do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "7f1144971e418efda32556671bc85bfe27882262681cf5d706554544d6eb77f5"
    end
  end

  def install
    bin.install "polymarket"
  end

  test do
    assert_match "polymarket", shell_output("#{bin}/polymarket --version")
  end
end
