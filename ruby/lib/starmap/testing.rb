# frozen_string_literal: true

require "starmap"

module Starmap
  # The test helper that keeps an application's reads and its starmap.toml
  # matched. It scans app/, lib/ and config/ for literal reads of ENV, such
  # as ENV["NAME"] and ENV.fetch("NAME"), and reports each name that
  # starmap.toml does not declare. It sees only a name written as a string
  # literal, so a read whose name is built at run time is not reported. A
  # read after a # comment marker is skipped, and a file that is not valid
  # UTF-8 is scanned with each invalid byte replaced by U+FFFD.
  module Testing
    DIRECTORIES = %w[app lib config].freeze
    EXTENSIONS = %w[.rb .rake .erb .yml .yaml].freeze
    READ = /(?<![\w:])(?:::)?ENV(?:\[\s*(?<open>["'])(?<index>[A-Za-z_][A-Za-z0-9_]*)\k<open>\s*\]|
             \.(?:fetch|key\?|has_key\?|include\?|member\?)(?:\(\s*|[ \t]+)(?<quote>["'])(?<call>[A-Za-z_][A-Za-z0-9_]*)\k<quote>)/x

    # A read of +name+ at +line+ of +path+, which is relative to the root.
    Read = Data.define(:path, :line, :name) do
      def to_s = "#{path}:#{line} reads #{name}"
    end

    # Some reads name a variable that starmap.toml does not declare.
    class UndeclaredReads < StandardError; end

    module_function

    # Returns each literal read of an undeclared name, in path and line
    # order. +allow+ lists names that may be read without a declaration.
    def undeclared_reads(root: Dir.pwd, spec: File.join(root, "starmap.toml"), allow: [])
      spec = Spec.load(spec)
      files(root).flat_map do |path|
        reads(root, path).reject { |read| allow.include?(read.name) || spec.declared?(read.name) }
      end
    end

    # Raises UndeclaredReads, which names every undeclared read, when there
    # is one.
    def assert_declared!(root: Dir.pwd, spec: File.join(root, "starmap.toml"), allow: [])
      reads = undeclared_reads(root: root, spec: spec, allow: allow)
      return if reads.empty?

      raise UndeclaredReads, "starmap.toml does not declare what these reads name: #{reads.join("; ")}"
    end

    def files(root)
      DIRECTORIES.flat_map { |directory| Dir.glob(File.join(root, directory, "**", "*")) }
                 .select { |path| File.file?(path) && EXTENSIONS.include?(File.extname(path)) }
                 .sort
    end

    def reads(root, path)
      relative = path.delete_prefix("#{root}/")
      text = File.binread(path).force_encoding(Encoding::UTF_8).scrub("\uFFFD")
      text.each_line.with_index(1).flat_map do |line, number|
        code(line).to_enum(:scan, READ).map do
          match = Regexp.last_match
          Read.new(path: relative, line: number, name: match[:index] || match[:call])
        end
      end
    end

    # The line up to its first # that is outside a quoted string.
    def code(line)
      quote = nil
      line.each_char.with_index do |char, index|
        if quote
          quote = nil if char == quote && line[index - 1] != "\\"
        elsif ["\"", "'"].include?(char)
          quote = char
        elsif char == "#"
          return line[0, index]
        end
      end
      line
    end
  end
end
