# Homebrew formula for Architext.
#
# This is the source of truth for the `architext` formula. To publish it, copy
# it to `Formula/architext.rb` in the tap repo (robot-accomplice/homebrew-architext);
# see packaging/homebrew/README.md. Bump `version` and the four `sha256` values
# on each release (the digests are published as the release's SHA256SUMS asset).
class Architext < Formula
  desc "Local JSON-backed architecture and dataflow viewer (Rust-native CLI + WASM viewer)"
  homepage "https://github.com/robot-accomplice/architext"
  version "1.7.2"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/robot-accomplice/architext/releases/download/v#{version}/architext-darwin-arm64"
      sha256 "04dfe4d0aa8654a1e869cec14cb11341da1c59a1ca13f535639cf98f057a33f6"
    end
    on_intel do
      url "https://github.com/robot-accomplice/architext/releases/download/v#{version}/architext-darwin-x64"
      sha256 "39af82172bc8fd162830c618724366b526e97d948b9fd1bef633097f7b193c2a"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/robot-accomplice/architext/releases/download/v#{version}/architext-linux-arm64"
      sha256 "5f694da4f023732b7e5262f14241d935af9481f0b7c555adfea2c439409007f5"
    end
    on_intel do
      url "https://github.com/robot-accomplice/architext/releases/download/v#{version}/architext-linux-x64"
      sha256 "540803089d217b18050bada6b64db92b2c4d93a8a1be28e06ab41ff23f104b17"
    end
  end

  def install
    # The release asset is named architext-<platform>; install it as `architext`.
    bin.install Dir["architext-*"].first => "architext"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/architext --version")
  end
end
