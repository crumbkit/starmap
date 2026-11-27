# frozen_string_literal: true

module Starmap
  # The canonical form of a spec, as the glossary's canonical form entry
  # gives it. Its bytes match what the Rust crate's Spec::to_toml writes.
  module Canonical
    module_function

    def write(spec)
      tables = []
      tables << ["mode", [["variable", spec.mode.variable], ["production", spec.mode.production]]] if spec.mode
      spec.enumerations.each do |enumeration|
        tables << ["enumerations.#{key(enumeration.name)}", enumeration.descriptions.to_a]
      end
      spec.groups.each { |group| tables << ["groups.#{key(group.name)}", [["variables", group.variables]]] }
      spec.families.each do |family|
        fields = variable(family.variable)
        fields << ["keys", family.keys] if family.keys
        tables << ["families.#{key(family.prefix)}", fields]
      end
      spec.variables.each { |name, declared| tables << ["variables.#{key(name)}", variable(declared)] }
      tables.map { |header, fields| table(header, fields) }.join("\n")
    end

    def table(header, fields)
      "[#{header}]\n#{fields.map { |name, value| "#{key(name)} = #{value(value)}\n" }.join}"
    end

    def variable(variable)
      fields = [["purpose", variable.purpose]]
      fields << ["effect", variable.effect] if variable.effect
      fields << ["check", variable.check] if variable.check
      fields << ["type", Inline.new(type(variable.type))]
      fields << ["presence", true] if variable.presence
      fields << %w[empty value] if variable.empty == "value"
      fields << ["unset", Inline.new(unset(variable))]
      fields << ["show", variable.show]
      fields << ["source", variable.source.kind]
      fields << ["bind", Inline.new(variable.source.bind)] if variable.source.bind
      fields << ["bytes", variable.source.bytes] if variable.source.kind == "generated" && variable.source.bytes
      fields
    end

    # An inline table, written from its key and value pairs, or a plain
    # value when it holds no pairs.
    Inline = Data.define(:pairs)

    def type(type)
      return type.word if type.word

      case type
      when Types::Path then [["path", Inline.new([["absolute", true]])]]
      when Types::IntegerType then [["integer", Inline.new([["min", type.min], ["max", type.max]])]]
      when Types::Enumeration then [["enumeration", type.name || type.choices]]
      when Types::Bytes
        [["bytes", Inline.new([["encoding", type.encoding], [type.at_least ? "min-length" : "length", type.length]])]]
      when Types::Url then [["url", Inline.new([["schemes", type.schemes]])]]
      when Types::StringType then [["string", Inline.new([["pattern", type.pattern.source]])]]
      when Types::Boolean then [["boolean", Inline.new([["true", type.for_true], ["false", type.for_false]])]]
      end
    end

    def unset(variable)
      rule = variable.unset
      local = rule.local
      production = rule.production == local ? nil : rule.production.to_s
      first = local.is_a?(Default) ? ["default", default_text(variable.type, local.value)] : ["local", local.to_s]
      return first.last if first.first == "local" && production.nil?

      [first, *(production ? [["production", production]] : [])]
    end

    def default_text(type, value)
      type.is_a?(Types::Path) ? type.canonical(value.text) : value.text
    end

    def value(value)
      case value
      when Inline then value.pairs.is_a?(Array) ? inline(value.pairs) : value(value.pairs)
      when String then string(value)
      when ::Integer, true, false then value.to_s
      when Array then "[#{value.map { |item| value(item) }.join(", ")}]"
      end
    end

    def inline(pairs)
      "{ #{pairs.map { |name, item| "#{key(name)} = #{value(item)}" }.join(", ")} }"
    end

    CONTROL = /[\u0000-\u0008\u000A-\u001F\u007F]/
    CONTROL_BUT_NEWLINE = /[\u0000-\u0008\u000B-\u001F\u007F]/

    # A string in the first of the glossary's five forms that fits it.
    def string(text)
      return %("#{text.gsub("\t", "\\t")}") unless text.match?(/["\\]/) || text.match?(CONTROL)
      return "'#{text}'" unless text.include?("'") || text.match?(CONTROL)

      newline = text.include?("\n") ? "\n" : ""
      unless text.include?("\\") || text.match?(/"{3}/) || text.match?(CONTROL_BUT_NEWLINE)
        return %("""#{newline}#{text.gsub("\t", "\\t")}""")
      end
      return "'''#{newline}#{text}'''" unless text.match?(/'{3}/) || text.match?(CONTROL_BUT_NEWLINE)

      escaped(text)
    end

    ESCAPES = { "\\" => "\\\\", "\b" => "\\b", "\t" => "\\t", "\f" => "\\f", "\r" => "\\r" }.freeze

    def escaped(text, multiline: text.include?("\n"))
      body = +""
      run = 0
      text.each_char do |char|
        run = char == %(") ? run + 1 : 0
        body << escape(char, multiline, run)
        run = 0 if char == %(") && multiline && run == 3
      end
      multiline ? %("""\n#{body}""") : %("#{body}")
    end

    # One character of an escaped string. A quote is escaped in a one-line
    # string, and in a multi-line string when it would make a run of three.
    def escape(char, multiline, run)
      return ESCAPES[char] if ESCAPES.key?(char)
      return multiline ? "\n" : "\\n" if char == "\n"
      return !multiline || run == 3 ? %(\\") : char if char == %(")
      return format("\\u%04X", char.ord) if char.match?(CONTROL)

      char
    end

    # A key, bare when it is letters, digits, - and _.
    def key(name)
      return name if name.match?(/\A[A-Za-z0-9_-]+\z/)
      return %("#{name.gsub("\t", "\\t")}") unless name.match?(/["\\]/) || name.match?(CONTROL)
      return "'#{name}'" unless name.include?("'") || name.match?(CONTROL)

      escaped(name, multiline: false)
    end
  end
end
