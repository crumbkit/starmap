# frozen_string_literal: true

require_relative "lib/starmap/version"

Gem::Specification.new do |spec|
  spec.name = "starmap"
  spec.version = Starmap::VERSION
  spec.summary = "Reads the environment variables that starmap.toml declares."
  spec.description = <<~TEXT
    starmap reads the starmap.toml beside an application and checks the
    process environment against it once at boot: types, defaults, the mode
    rule and the rules for each mode. The application then reads typed
    values from Starmap.env.
  TEXT
  spec.authors = ["crumbkit"]
  spec.homepage = "https://github.com/crumbkit/starmap"
  spec.licenses = ["MIT", "Apache-2.0"]
  spec.required_ruby_version = ">= 3.3"
  spec.files = Dir["lib/**/*.rb", "README.md"]
  spec.require_paths = ["lib"]
  spec.metadata["rubygems_mfa_required"] = "true"

  # perfect_toml parses TOML 1.0 in pure Ruby with no dependencies of its own.
  spec.add_dependency "perfect_toml", "~> 0.9"
end
