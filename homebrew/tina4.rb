# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.95"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.95/tina4-darwin-arm64"
      sha256 "3da0981c73fc20af39ef917b84d73978f45ac7ffb997cbfbc9245067aff138f8"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.95/tina4-darwin-amd64"
      sha256 "daab729abd1456b0dc4219b184cf3f590a01997aa584041351bc9833e3f019c9"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.95/tina4-linux-arm64"
      sha256 "8516cb3cff036e098381ec021805e32c0636d0a50116c8af3bd57370e6fcb291"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.95/tina4-linux-amd64"
      sha256 "382f77a98e16fcc566bb345709f4e4db98c9956b446cd7a414cb7d5dbff9cbe1"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
