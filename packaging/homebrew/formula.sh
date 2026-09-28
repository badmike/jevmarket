#!/usr/bin/env bash
# Prints the Homebrew formula for a release: formula.sh <tag> <dir with the release .tar.gz files>
# The release workflow commits the output to the tap as Formula/jevmarket.rb.
set -euo pipefail

tag="$1"
dir="$2"
base="https://github.com/badmike/jevmarket/releases/download/$tag"

# The url and sha256 lines for one target's archive.
asset() {
  local file="jevmarket-$tag-$1.tar.gz"
  local sum
  sum="$(shasum -a 256 "$dir/$file" | cut -d' ' -f1)"
  printf '      url "%s/%s"\n      sha256 "%s"\n' "$base" "$file" "$sum"
}

cat <<RUBY
class Jevmarket < Formula
  desc "Polymarket trading bot priced by Jev via OpenRouter"
  homepage "https://github.com/badmike/jevmarket"
  version "${tag#v}"
  license "MIT"

  on_macos do
    on_arm do
$(asset aarch64-apple-darwin)
    end
    on_intel do
$(asset x86_64-apple-darwin)
    end
  end

  on_linux do
    on_arm do
$(asset aarch64-unknown-linux-gnu)
    end
    on_intel do
$(asset x86_64-unknown-linux-gnu)
    end
  end

  def install
    bin.install "jevmarket"
  end

  def caveats
    <<~EOS
      Set up keys, models and risk limits:
        jevmarket init
      Run the daemon and its web console as a login service:
        jevmarket service install --dry-run
      After upgrading, restart it:
        jevmarket service restart
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/jevmarket --version")
  end
end
RUBY
