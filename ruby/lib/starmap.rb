# frozen_string_literal: true

require_relative "starmap/version"
require_relative "starmap/types"
require_relative "starmap/spec"
require_relative "starmap/parser"
require_relative "starmap/canonical"
require_relative "starmap/check"
require_relative "starmap/environment"

# starmap reads the environment variables that starmap.toml declares. An
# application calls Starmap.load! once at boot, after dotenv has loaded its
# files, and reads typed values from Starmap.env.
#
# The gem resolves nothing: it reads no bind, asks no provider and
# generates no value. A program that starts the application, such as
# `starmap run`, does that, and the gem checks what reaches ENV.
module Starmap
  # Starmap.env was read before Starmap.load! succeeded.
  class NotLoaded < StandardError; end

  class << self
    # Reads +environment+ once, checks it against the starmap.toml at
    # +path+, and keeps the result as Starmap.env. It raises SpecError when
    # the file is refused, and Error, which lists every finding, when the
    # environment is refused.
    def load!(path: "starmap.toml", environment: ENV)
      spec = Spec.load(path)
      result = Check.run(spec, environment.to_h)
      raise Error, result.findings if result.refused?

      @env = Environment.new(spec, result)
    end

    # The environment that Starmap.load! read.
    def env
      @env or raise NotLoaded, "Starmap.env is read before Starmap.load! has succeeded"
    end
  end
end
