# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.93"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.93/tina4-darwin-arm64"
      sha256 "f6012b665f203e3745423dc5d38fe07cd9c73db83bee28dd3d20874f65e74e6c"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.93/tina4-darwin-amd64"
      sha256 "9acdc9794b98be337b88116cc0fe157b3fd9998d5307623f8b563b84d1d5ca07"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.93/tina4-linux-arm64"
      sha256 "02393a19f3b7ecc18a43262a3a7473947aded1c367d4694eb2c902da6a6e74d1"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.93/tina4-linux-amd64"
      sha256 "267646ec13f0378f51311abd2e83ce2b60c3851f7a8c4ccf8bb4f4abf6558894"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
