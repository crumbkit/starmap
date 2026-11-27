# frozen_string_literal: true

require "digest"

module Starmap
  # One problem that refuses a starmap.toml. +kind+ is a symbol such as
  # :bad_default, and #code is its code, such as starmap::spec::bad_default.
  Problem = Data.define(:kind, :message) do
    def code = Problem::CODES.fetch(kind)
    def to_s = "#{code}: #{message}"
  end

  Problem::CODES = %i[
    too_large syntax unknown_field missing_field bad_value bad_name guess
    bad_default bad_range bad_pattern empty_enumeration shown_secret
    withheld_type withheld_default bind_mismatch bad_bytes unknown_placeholder
    mode_undeclared mode_source pointer_declares pointer_chain
    enumeration_undeclared group_undeclared bad_group bad_family
  ].to_h { |kind| [kind, "starmap::spec::#{kind}"] }.freeze

  # A default, which an unset rule holds for local mode.
  Default = Data.define(:value)

  # What happens when a variable has no value, in each mode. Each rule is
  # :required, :optional or a Default.
  UnsetRule = Data.define(:local, :production) do
    def in_mode(mode) = mode == :production ? production : local
  end

  # Where a variable's value comes from. +bind+ is the bind's fields as an
  # array of key and value pairs, in the order the canonical form writes
  # them. +bytes+ is the count of a generated value.
  Source = Data.define(:kind, :bind, :bytes)

  # One declared variable, or the fields that every member of a family
  # shares.
  Variable = Data.define(:purpose, :effect, :check, :type, :presence, :empty, :unset, :show, :source) do
    def withheld? = show == "withheld"

    # Whether an empty value is a value. A variable that reads its presence
    # counts an empty value as a value.
    def empty_is_value? = presence || empty == "value"
  end

  # A family of variables named PREFIX_KEY, whose keys are listed or open.
  Family = Data.define(:prefix, :variable, :keys) do
    def key_of(name)
      key = name.delete_prefix("#{prefix}_")
      key if name.start_with?("#{prefix}_") && Names.key?(key)
    end

    def lists?(key) = keys.nil? || keys.include?(key)
  end

  # A set of variables that are set together or not at all.
  Group = Data.define(:name, :variables)

  # The rule that decides the mode from a variable.
  ModeRule = Data.define(:variable, :production) do
    def decide(value) = value && production.include?(value) ? :production : :local
  end

  # A tool table, which starmap checks is a table and never reads.
  ToolTable = Data.define(:name, :table)

  # The rules of names and keys.
  module Names
    MAX_BYTES = 128

    def self.error(text)
      return "the name is empty" if text.empty?
      return "the name has #{text.bytesize} bytes, and a name has at most #{MAX_BYTES}" if text.bytesize > MAX_BYTES

      return if text.match?(/\A[A-Za-z_][A-Za-z0-9_]*\z/)

      "a name holds only ASCII letters, digits and underscores and does not start with a digit"
    end

    def self.key?(text) = text.match?(/\A[A-Z0-9][A-Z0-9_]*\z/)
    def self.lower_word?(text) = text.match?(/\A[a-z][a-z0-9-]*\z/)
  end

  # A parsed starmap.toml.
  class Spec
    MAX_BYTES = 256 * 1024

    attr_reader :mode, :enumerations, :groups, :families, :variables, :tools

    # Parses the text of a starmap.toml, or raises SpecError with every
    # problem the parser finds.
    def self.parse(text)
      Parser.new.parse(text)
    end

    # Reads the starmap.toml at +path+. A pointer file is followed once, to
    # the file its +spec+ names.
    def self.load(path)
      path = File.expand_path(path)
      text = File.read(path, mode: "rb")
      target = Parser.pointer(text)
      return parse(text) unless target

      parse(File.read(File.expand_path(target, File.dirname(path)), mode: "rb"))
    end

    def initialize(mode:, enumerations:, groups:, families:, variables:, tools:) # rubocop:disable Metrics/ParameterLists
      @mode = mode
      @enumerations = enumerations
      @groups = groups
      @families = families
      @variables = variables
      @tools = tools
      freeze
    end

    def variable(name) = variables.find { |declared, _| declared == name }&.last
    def family(prefix) = families.find { |family| family.prefix == prefix }
    def family_of(name) = families.find { |family| family.key_of(name) }
    def group_of(name) = groups.find { |group| group.variables.include?(name) }

    # Whether +name+ is a declared variable, or a member of a family that is
    # open or lists the member's key.
    def declared?(name)
      return true if variable(name)

      family = family_of(name)
      !family.nil? && family.lists?(family.key_of(name))
    end

    # The canonical form, which docs/glossary.md describes.
    def to_toml = Canonical.write(self)

    # sha256: and the hex digest of the canonical form.
    def digest = "sha256:#{Digest::SHA256.hexdigest(to_toml)}"
  end
end
