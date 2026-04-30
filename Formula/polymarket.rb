class Polymarket < Formula
  desc "CLI for Polymarket — browse markets, trade, and manage positions"
  homepage "https://github.com/Alpha9Coding/polymarket-cli"
  version "0.2.1"
  license "MIT"

  on_macos do
    on_intel do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "7b9ac5b923a9d9d03f0a6597963992b3c2012255eb4f4362f731b2c057ed32fc"
    end

    on_arm do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "67892caa5abe5d33a9183eb23de72b5ef905e662b8dce899021e5635627342fd"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "c54fd5b2365f2d2ca151a317200606438ed0fe61326d20ea29f8dfd44ba19c45"
    end

    on_arm do
      url "https://github.com/Alpha9Coding/polymarket-cli/releases/download/v#{version}/polymarket-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "45dc2b4e95102a83900f9913ca95a17fa7193eaab65f5189e07dcfcc73cdb687"
    end
  end

  def install
    bin.install "polymarket"
  end

  test do
    assert_match "polymarket", shell_output("#{bin}/polymarket --version")
  end
end
