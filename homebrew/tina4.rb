# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.96"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.96/tina4-darwin-arm64"
      sha256 "dd6be3d1a847fec3f3faee3b9d56b3cc0bfe92687091e632c9d20e3df1019074"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.96/tina4-darwin-amd64"
      sha256 "c8769d02c3dad17b538bcceca6548b908024e0b17991728fa1b5fc6e32ced52e"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.96/tina4-linux-arm64"
      sha256 "aeb68a9d1d7ae46cae42ad20ef23ea26d595c5abb7d0d71d54c103ea989e2ccb"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.96/tina4-linux-amd64"
      sha256 "ae048622eee1094a349f25120cf2703c817eb5bc01675c37cbb42cc2cf22a855"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
