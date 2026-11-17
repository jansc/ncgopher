class Ncgopher < Formula
  desc "Ncurses gopher and gemini client for the modern internet"
  homepage "https://github.com/jansc/ncgopher"
  url "https://github.com/jansc/ncgopher/archive/refs/tags/v0.8.0.tar.gz"
  sha256 "c91b139fd3ec89dd1c20c75350de1a21624c03b16d086d9e7dd4e5dd8c829761"
  license "BSD-2-Clause"
  head "https://github.com/jansc/ncgopher.git", branch: "master"

  depends_on "pkgconf" => :build
  depends_on "rust" => :build
  # macOS ships ncurses 5.4 with a 64-bit mmask_t, which the ncurses-rs crate
  # does not expect, so any mouse click crashes ncgopher. Link Homebrew's
  # ncurses 6 instead.
  depends_on "ncurses"

  uses_from_macos "sqlite"

  def install
    system "cargo", "install", *std_cargo_args
    man1.install "ncgopher.1"
  end

  test do
    output = shell_output("#{bin}/ncgopher --version")
    if build.head?
      # HEAD builds report the Cargo package version, not Homebrew's "HEAD" version
      assert_match(/\Ancgopher \d+\.\d+\.\d+/, output)
    else
      assert_match version.to_s, output
    end
  end
end
