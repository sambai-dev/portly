class Portly < Formula
  desc "Terminal cockpit for everything running on your machine's ports"
  homepage "https://github.com/sambai-dev/portly"
  version "0.1.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/sambai-dev/portly/releases/download/v0.1.0/portly-aarch64-apple-darwin.tar.gz"
      sha256 "a3608f4783381fa8fd48b6b59f241c9970a39163a51b4b6902eb40ce7f679b69"
    else
      url "https://github.com/sambai-dev/portly/releases/download/v0.1.0/portly-x86_64-apple-darwin.tar.gz"
      sha256 "7b303b3b48e10f747a327bea47b31c156326b01d5c58af2af25ddc163064d2a4"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/sambai-dev/portly/releases/download/v0.1.0/portly-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "92ce9a2ad7bf630667ad8516da13f7ff15c6de0a1551fc1d1c01c1b295fbc599"
    end
  end

  def install
    bin.install "portly"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/portly --version")
  end
end
