class Portly < Formula
  desc "Terminal cockpit for everything running on your machine's ports"
  homepage "https://github.com/sambai-dev/portly"
  version "0.1.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/sambai-dev/portly/releases/download/v0.1.0/portly-aarch64-apple-darwin.tar.gz"
      sha256 "__AARCH64_SHA256__"
    else
      url "https://github.com/sambai-dev/portly/releases/download/v0.1.0/portly-x86_64-apple-darwin.tar.gz"
      sha256 "__X86_64_MAC_SHA256__"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/sambai-dev/portly/releases/download/v0.1.0/portly-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "__LINUX_GNU_SHA256__"
    end
  end

  def install
    bin.install "portly"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/portly --version")
  end
end
