#!/usr/bin/env ruby
# Fails unless every file in Sources/ has 100% line coverage. Run after `swift test --enable-code-coverage`.
require "json"

path = `swift test --show-codecov-path`.strip
files = JSON.parse(File.read(path))["data"][0]["files"].select { |f| f["filename"].include?("/Sources/") }
abort "no coverage data for Sources/" if files.empty?

short = files.reject { |f| f["summary"]["lines"]["percent"] == 100 }
files.each do |f|
  l = f["summary"]["lines"]
  puts format("%6.1f%%  %4d/%-4d %s", l["percent"], l["covered"], l["count"], f["filename"].sub(%r{.*/Sources/}, ""))
end
abort "line coverage below 100%" unless short.empty?
