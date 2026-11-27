# frozen_string_literal: true

module Starmap
  # The error that refuses an environment. Its message lists every finding
  # and quotes a shown value after its reason, and it never quotes a
  # withheld value.
  class Error < StandardError
    attr_reader :findings

    def initialize(findings)
      @findings = findings.freeze
      super("the environment does not fit starmap.toml: #{findings.join("; ")}")
    end
  end

  # A name that starmap.toml does not declare was read from Starmap.env.
  class Undeclared < KeyError; end

  # A withheld value. #inspect and #to_s never show it, and #reveal
  # returns it: the text for a string type, or the decoded bytes for a
  # bytes type.
  class Secret
    def initialize(value)
      @value = value.dup.freeze
      freeze
    end

    def reveal = @value
    def inspect = "Secret(••••••••)"
    def to_s = inspect
    def ==(other) = other.is_a?(Secret) && other.reveal == reveal
  end

  # The checked environment. #[] returns a declared variable's value in
  # its type, or nil when it is unset.
  class Environment
    attr_reader :mode

    def initialize(spec, result)
      @spec = spec
      @mode = result.mode
      @values = result.held.to_h { |name, held| [name, held && value(held)] }.freeze
      freeze
    end

    def production? = mode == :production
    def local? = mode == :local

    # The value of +name+: an Integer for a port or an integer, true or
    # false for a boolean, a Pathname for a path, a String for any other
    # shown type, decoded bytes for a bytes type, and a Secret for a
    # withheld variable. It raises Undeclared for a name that starmap.toml
    # does not declare, and for a family member whose key a listed family
    # does not list.
    def [](name)
      name = name.to_s
      raise Undeclared, "starmap.toml does not declare #{name}" unless declared?(name)

      @values[name]
    end

    # The set members of the family +prefix+, as a hash of key and value
    # in key order.
    def family(prefix)
      family = @spec.family(prefix.to_s) or raise Undeclared, "starmap.toml does not declare the family #{prefix}"

      @values.filter_map { |name, value| [family.key_of(name), value] if family.key_of(name) && !value.nil? }.sort.to_h
    end

    private

    def declared?(name)
      return true if @spec.variable(name)

      family = @spec.family_of(name)
      family&.lists?(family.key_of(name)) || false
    end

    def value(held)
      case held.kind
      when :shown then held.value.ruby
      when :presence then !held.raw.nil?
      when :withheld then held.value
      end
    end
  end
end
