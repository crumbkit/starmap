# frozen_string_literal: true

require "perfect_toml"

module Starmap
  # The parser of starmap.toml. It follows the Rust parser's rules and
  # reports every problem, in the order that conformance/README.md gives.
  class Parser
    ROOT_FIELDS = %w[mode enumerations groups families variables tool].freeze
    VARIABLE_FIELDS = %w[purpose effect check type presence empty unset show source bind bytes tool].freeze
    FAMILY_FIELDS = %w[purpose effect check type empty unset show source bind bytes tool keys].freeze
    TYPE_TABLES = %w[path integer enumeration string url bytes boolean].freeze
    UNSET_FIELDS = %w[local default production].freeze
    MODE_FIELDS = %w[variable production].freeze
    GROUP_FIELDS = %w[variables].freeze
    POINTER_FIELDS = %w[spec src dotenv tool].freeze
    DECLARING_FIELDS = %w[mode enumerations groups families variables].freeze
    BIND_KINDS = %w[provided path one-password].freeze
    PLACEHOLDERS = %w[workspace checkout home].freeze

    TYPE_WORDS = {
      "path" => Types::Path.new(absolute: false),
      "port" => Types::Port.new,
      "url" => Types::Url.new(schemes: nil),
      "socket-address" => Types::SocketAddress.new,
      "boolean" => Types::Boolean.new(for_true: nil, for_false: nil),
      "string" => Types::StringType.new(pattern: nil)
    }.freeze
    ENCODING_WORDS = %w[hex base64 base64url].to_h { |word| [word, word] }.freeze
    UNSET_WORDS = { "required" => :required, "optional" => :optional }.freeze
    SHOW_WORDS = %w[shown withheld].to_h { |word| [word, word] }.freeze
    EMPTY_WORDS = %w[unset value].to_h { |word| [word, word] }.freeze
    SOURCE_WORDS = %w[setting secret generated inherited provided].to_h { |word| [word, word] }.freeze

    # A key of a table and its value, with the dotted path that names it.
    Field = Data.define(:path, :key, :item) do
      def child(table, name)
        Field.new(path: "#{path}.#{name}", key: name, item: table[name]) if table.key?(name)
      end
    end

    # Stands for a field that did not parse, where nil means it is absent.
    FAILED = Object.new.freeze

    # Parses a pointer file, as the Rust crate's Pointer::parse does. It
    # returns the path that +spec+ names, or nil for a file without +spec+,
    # and raises SpecError when the pointer also declares variables, holds a
    # key a pointer does not take, or gives a path that is not text or is
    # empty.
    def self.pointer(text)
      new.pointer(text)
    end

    def pointer(text)
      @problems = []
      root = toml(text) or raise SpecError, @problems
      return nil unless root.key?("spec")

      declares_too?(root)
      root.each_key do |key|
        next if POINTER_FIELDS.include?(key) || DECLARING_FIELDS.include?(key)

        report(:unknown_field, "the pointer has the field `#{key}`, and it takes #{POINTER_FIELDS.join(", ")}")
      end
      target = pointer_path(root_field(root, "spec"))
      pointer_paths(root_field(root, "src")) if root.key?("src")
      pointer_path(root_field(root, "dotenv")) if root.key?("dotenv")
      tools(root_field(root, "tool")) if root.key?("tool")
      raise SpecError, @problems unless @problems.empty?

      target
    end

    def parse(text)
      @problems = []
      @enumerations = {}
      spec = document(text)
      raise SpecError, @problems unless @problems.empty?

      spec
    end

    private

    def report(kind, message)
      @problems << Problem.new(kind: kind, message: message)
    end

    def document(text)
      parts = { mode: nil, enumerations: [], groups: [], families: [], variables: [], tools: [] }
      root = toml(text)
      return Spec.new(**parts) unless root

      if root.key?("spec")
        points(root)
        return Spec.new(**parts)
      end
      known(root, "", ROOT_FIELDS)
      if (field = root_field(root, "enumerations"))
        @enumerations = enumerations(field)
        parts[:enumerations] = @enumerations.values.compact.sort_by(&:name)
      end
      parts[:families] = families(root_field(root, "families")) if root.key?("families")
      declared = []
      if (field = root_field(root, "variables")) && (table = table(field))
        table.each do |key, item|
          field = Field.new(path: "variables.#{key}", key: key, item: item)
          name = name(field)
          kind, variable = variable(field, VARIABLE_FIELDS)
          not_a_member(field, name, parts[:families])
          next unless name

          declared << [name, kind]
          parts[:variables] << [name, variable] if variable
        end
      end
      parts[:mode] = mode(root_field(root, "mode"), declared, parts[:variables]) if root.key?("mode")
      parts[:groups] = groups(root_field(root, "groups"), declared, parts[:variables], parts[:families]) if root.key?("groups")
      parts[:tools] = tools(root_field(root, "tool")) || [] if root.key?("tool")
      Spec.new(**parts)
    end

    def root_field(root, name)
      Field.new(path: name, key: name, item: root[name]) if root.key?(name)
    end

    def toml(text)
      if text.bytesize > Spec::MAX_BYTES
        report(:too_large, "the file has #{text.bytesize} bytes, and the limit is #{Spec::MAX_BYTES} bytes")
        return nil
      end
      text = text.dup.force_encoding(Encoding::UTF_8)
      raise PerfectTOML::ParseError, "the file is not UTF-8" unless text.valid_encoding?

      PerfectTOML.parse(text)
    rescue PerfectTOML::ParseError, ArgumentError => e
      report(:syntax, "the file is not valid toml: #{e.message}")
      nil
    end

    def points(root)
      return if declares_too?(root)

      report(:pointer_chain, "the file has `spec`, so it points to another file, and starmap reads variables " \
                             "only from a file that declares them")
    end

    # Reports each key of a pointer that declares, and returns whether there
    # was one.
    def declares_too?(root)
      found = root.keys.select { |key| DECLARING_FIELDS.include?(key) }
      found.each do |key|
        report(:pointer_declares, "the file has `spec`, so it points to another file, and it has `#{key}` too, " \
                                  "and a file either points or declares variables")
      end
      !found.empty?
    end

    def pointer_path(field)
      text = string(field) or return nil
      return text unless text.empty?

      report(:bad_value, "`#{field.path}` is empty, and it takes a path")
      nil
    end

    def pointer_paths(field)
      unless field.item.is_a?(Array)
        wrong_type(field, "a list of paths")
        return
      end
      bad = field.item.find { |value| !value.is_a?(String) || value.empty? }
      report(:bad_value, "`#{field.path}` holds #{bad.inspect}, and each entry is a path") unless bad.nil?
    end

    def known(table, path, fields)
      owner = path.empty? ? "the file" : "`#{path}`"
      table.each_key do |key|
        report(:unknown_field, "#{owner} has the field `#{key}`, and it takes #{fields.join(", ")}") unless fields.include?(key)
      end
    end

    def required(owner, table, name)
      field = owner.child(table, name)
      report(:missing_field, "`#{owner.path}` has no `#{name}` field, and it needs one") unless field
      field
    end

    def type_name(item)
      case item
      when String then "string"
      when ::Integer then "integer"
      when Float then "float"
      when true, false then "boolean"
      when Array then "array"
      when Hash then "table"
      else "datetime"
      end
    end

    def wrong_type(field, wanted)
      report(:bad_value, "`#{field.path}` is a toml #{type_name(field.item)}, and it takes #{wanted}")
    end

    def table(field)
      return field.item if field.item.is_a?(Hash)

      wrong_type(field, "a table")
      nil
    end

    def string(field)
      return field.item if field.item.is_a?(String)

      wrong_type(field, "text")
      nil
    end

    def boolean(field)
      return field.item if [true, false].include?(field.item)

      wrong_type(field, "true or false")
      nil
    end

    def confirmed(field)
      text = string(field) or return nil
      if text.end_with?("?")
        report(:guess, "`#{field.path}` is #{text.inspect}, which still ends in the ? that starmap init wrote")
        return nil
      end
      text
    end

    def word(field, words)
      text = confirmed(field) or return nil
      return words[text] if words.key?(text)

      report(:bad_value, "`#{field.path}` is #{text.inspect}, and it takes one of #{words.keys.join(", ")}")
      nil
    end

    def sentence(field, noun)
      text = string(field) or return nil
      if text == "?"
        report(:guess, "`#{field.path}` is \"?\", the guess that starmap init wrote, and #{noun} is one sentence")
        nil
      elsif text.match?(/\A[[:space:]]*\z/)
        report(:bad_value, "`#{field.path}` is #{text.inspect}, which is empty, and #{noun} is one sentence")
        nil
      else
        text
      end
    end

    def name(field)
      error = Names.error(field.key)
      return field.key unless error

      report(:bad_name, "`#{field.path}` is not a variable name, because #{error}")
      nil
    end

    def optional(field)
      return nil unless field

      yield(field).then { |value| value.nil? ? FAILED : value }
    end

    def variable(field, fields)
      table = table(field) or return [nil, nil]
      known(table, field.path, fields)
      purpose = required(field, table, "purpose")&.then { |purpose| sentence(purpose, "a purpose") }
      effect = optional(field.child(table, "effect")) { |effect| sentence(effect, "an effect") }
      check = optional(field.child(table, "check")) { |check| sentence(check, "a check") }
      type = required(field, table, "type")&.then { |type| value_type(type) }
      show = required(field, table, "show")&.then { |show| word(show, SHOW_WORDS) }
      unset = required(field, table, "unset")&.then { |unset| unset(unset, type, show == "withheld") }
      presence = presence_field(field, table, fields, type, unset)
      empty = optional(field.child(table, "empty")) { |empty| empty(empty, presence) } || "unset"
      kind = required(field, table, "source")&.then { |source| word(source, SOURCE_WORDS) }
      bind = optional(field.child(table, "bind")) { |bind| bind(bind) }
      bytes = field.child(table, "bytes")
      tools = optional(field.child(table, "tool")) { |tool| tools(tool) }

      if show == "shown" && %w[secret generated].include?(kind)
        report(:shown_secret, "`#{field.path}.show` is \"shown\", and a secret or generated variable is withheld")
      end
      if show == "withheld" && type && !type.is_a?(Types::StringType) && !type.is_a?(Types::Bytes)
        report(:withheld_type, "`#{field.path}.type` is not \"string\" or a bytes type, and a withheld variable " \
                               "has the type string, with or without a pattern, or bytes")
      end
      source = kind && source(field, kind, bind, bytes, type.is_a?(Types::Bytes) ? type : nil)
      parts = [purpose, effect, check, type, presence, empty, unset, show, source, tools]
      return [kind, nil] if parts.any?(FAILED) || [purpose, type, presence, empty, unset, show, source].any?(&:nil?)

      [kind, Variable.new(purpose:, effect:, check:, type:, presence:, empty:, unset:, show:, source:)]
    end

    def presence_field(field, table, fields, type, unset)
      presence = field.child(table, "presence")
      return false unless presence && fields.include?("presence")

      presence(presence, type, unset)
    end

    def presence(field, type, unset)
      value = boolean(field)
      return value if value.nil? || value == false

      rule = if type.is_a?(Types::Boolean) && type.for_true
               "a variable that reads its presence takes any value, so its type declares no spellings"
             elsif type && !type.is_a?(Types::Boolean)
               "only a variable with the type \"boolean\" reads its presence"
             elsif unset && [unset.local, unset.production].any?(Default)
               "a variable that reads its presence is false when it is unset, so it takes no default"
             end
      return true unless rule

      report(:bad_value, "`#{field.path}` is true, and #{rule}")
      nil
    end

    def empty(field, presence)
      empty = word(field, EMPTY_WORDS) or return nil
      return empty unless presence == true

      report(:bad_value, "`#{field.path}` is #{empty.inspect}, and a variable that reads its presence already " \
                         "counts an empty value as set, so it takes no `empty`")
      nil
    end

    def value_type(field)
      return word(field, TYPE_WORDS) if field.item.is_a?(String)

      table = table(field) or return nil
      known(table, field.path, TYPE_TABLES)
      named = TYPE_TABLES.select { |name| table.key?(name) }
      if named.length == 1
        send(:"#{named.first}_type", field.child(table, named.first))
      elsif named.empty?
        report(:missing_field, "`#{field.path}` is an empty table, and it takes one of #{TYPE_TABLES.join(", ")}") if table.empty?
        nil
      else
        report(:bad_value, "`#{field.path}` names #{named.join(", ")}, and a type names one of them")
        nil
      end
    end

    def path_type(field)
      table = table(field) or return nil
      known(table, field.path, %w[absolute])
      absolute = field.child(table, "absolute")
      return Types::Path.new(absolute: false) unless absolute

      value = boolean(absolute)
      Types::Path.new(absolute: value) unless value.nil?
    end

    def string_type(field)
      table = table(field) or return nil
      known(table, field.path, %w[pattern])
      pattern = field.child(table, "pattern")
      return Types::StringType.new(pattern: nil) unless pattern

      text = string(pattern) or return nil
      Types::StringType.new(pattern: Types::Pattern.new(text))
    rescue ArgumentError => e
      report(:bad_pattern, "`#{pattern.path}` is #{text.inspect}, and #{e.message}")
      nil
    end

    def integer_type(field)
      table = table(field) or return nil
      known(table, field.path, %w[min max])
      min, max = %w[min max].map do |name|
        bound = required(field, table, name) or next nil
        next bound.item if bound.item.is_a?(::Integer)

        wrong_type(bound, "a whole number")
        nil
      end
      return nil unless min && max
      return Types::IntegerType.new(min: min, max: max) if min <= max

      report(:bad_range, "`#{field.path}` has the min #{min} above the max #{max}")
      nil
    end

    def enumeration_type(field)
      return choices(field)&.then { |choices| Types::Enumeration.new(choices:, name: nil, descriptions: nil) } if field.item.is_a?(Array)

      unless field.item.is_a?(String)
        wrong_type(field, "the name of an enumeration or an array of text")
        return nil
      end
      return @enumerations[field.item] if @enumerations.key?(field.item)

      report(:enumeration_undeclared, "`#{field.path}` is #{field.item.inspect}, which the file does not declare as an enumeration")
      nil
    end

    def url_type(field)
      table = table(field) or return nil
      known(table, field.path, %w[schemes])
      list = field.child(table, "schemes")
      return Types::Url.new(schemes: nil) unless list

      schemes = texts(list) { |text| "a scheme is lower case letters, digits, +, - and ." unless text.match?(/\A[a-z][a-z0-9+.-]*\z/) }
      schemes && distinct?(list, schemes) && Types::Url.new(schemes: schemes)
    end

    def boolean_type(field)
      table = table(field) or return nil
      known(table, field.path, %w[true false])
      for_true, for_false = %w[true false].map { |name| required(field, table, name)&.then { |list| texts(list) { nil } } }
      return nil unless for_true && for_false

      all = for_true + for_false
      if for_true.empty? || for_false.empty? || all.any?(&:empty?) || all.uniq.length != all.length
        report(:empty_enumeration, "`#{field.path}` needs a true list and a false list of spellings that are not empty " \
                                   "and list each spelling once")
        return nil
      end
      Types::Boolean.new(for_true: for_true, for_false: for_false)
    end

    def bytes_type(field)
      table = table(field) or return nil
      known(table, field.path, %w[encoding length min-length])
      encoding = required(field, table, "encoding")&.then { |encoding| word(encoding, ENCODING_WORDS) }
      exact = field.child(table, "length")
      smallest = field.child(table, "min-length")
      length = if exact && !smallest
                 count(exact, 1)
               elsif smallest && !exact
                 count(smallest, 1)
               else
                 kind, count = exact ? [:bad_value, "both"] : [:missing_field, "neither"]
                 report(kind, "`#{field.path}` sets #{count} length and min-length, and it takes one of them")
                 nil
               end
      Types::Bytes.new(encoding: encoding, length: length, at_least: !smallest.nil?) if encoding && length
    end

    def count(field, least)
      unless field.item.is_a?(::Integer)
        wrong_type(field, "a whole number")
        return nil
      end
      return field.item if field.item.between?(least, 1024)

      report(:bad_bytes, "`#{field.path}` is #{field.item}, and a count of bytes runs from #{least} to 1024")
      nil
    end

    def texts(field)
      unless field.item.is_a?(Array)
        wrong_type(field, "an array of text")
        return nil
      end
      parsed = field.item.each_with_index.filter_map do |value, index|
        path = "#{field.path}[#{index}]"
        unless value.is_a?(String)
          report(:bad_value, "`#{path}` is a toml #{type_name(value)}, and it takes text")
          next
        end
        error = yield(value)
        report(:bad_value, "`#{path}` is #{value.inspect}, and #{error}") if error
        value unless error
      end
      parsed if parsed.length == field.item.length
    end

    def choices(field)
      choices = texts(field) do |text|
        if text.empty? then "a value is not empty"
        elsif text.match?(/[[:space:]]/) then "a value holds no whitespace"
        end
      end
      choices if choices && distinct?(field, choices)
    end

    def distinct?(field, values)
      if values.empty?
        report(:empty_enumeration, "`#{field.path}` has no values, and it lists at least one")
        return false
      end
      repeat = values.find { |value| values.count(value) > 1 }
      return true unless repeat

      report(:empty_enumeration, "`#{field.path}` lists #{repeat.inspect} twice, and it lists each value once")
      false
    end

    def unset(field, type, withheld)
      if field.item.is_a?(String)
        local = word(field, UNSET_WORDS) or return nil
        return UnsetRule.new(local: local, production: local)
      end
      table = table(field) or return nil
      known(table, field.path, UNSET_FIELDS)
      production = optional(field.child(table, "production")) { |production| word(production, UNSET_WORDS) }
      local_field = field.child(table, "local")
      default = field.child(table, "default")
      local = if local_field && !default
                word(local_field, UNSET_WORDS)
              elsif default && withheld
                report(:withheld_default, "`#{field.path}` sets a default, and a withheld variable has none")
                nil
              elsif default && !local_field
                default_value(default, type)
              else
                kind, count = local_field ? [:bad_value, "both"] : [:missing_field, "neither"]
                report(kind, "`#{field.path}` sets #{count} local and default, and it takes one of them")
                nil
              end
      return nil if local.nil? || production == FAILED

      UnsetRule.new(local: local, production: production || local)
    end

    def default_value(field, type)
      unless field.item.is_a?(String)
        wrong_type(field, "text, such as \"20\"")
        return nil
      end
      return nil unless type

      value = type.parse(field.item)
      return Default.new(value: value) if value

      report(:bad_default, "`#{field.path}` is #{field.item.inspect}, and the type takes only an absolute path")
      nil
    rescue Invalid => e
      report(:bad_default, "`#{field.path}` is #{field.item.inspect}, and #{e.message}")
      nil
    end

    def bind(field)
      table = table(field) or return nil
      named = BIND_KINDS.select { |kind| table.key?(kind) }
      case named
      when ["provided"]
        provided(field, table)
      when ["path"]
        known(table, field.path, %w[path])
        value = field.child(table, "path")
        text = confirmed(value) or return nil
        path_template(value, text)
      when ["one-password"]
        known(table, field.path, %w[one-password cache])
        value = field.child(table, "one-password")
        text = confirmed(value) or return nil
        cache = cache(field.child(table, "cache"))
        reference = one_password(value, text)
        ["one-password", [["one-password", reference], *(cache == false ? [["cache", false]] : [])]] if reference && !cache.nil?
      else
        named = named.empty? ? "no kind of bind" : named.join(", ")
        report(:bad_value, "`#{field.path}` names #{named}, and a bind names exactly one of provided, path and one-password")
        nil
      end
    end

    def path_template(field, text)
      if text.empty?
        report(:bad_value, "`#{field.path}` is empty, and a path bind is a template")
        return nil
      end
      pieces = text.split("{", -1)
      error = "a brace in the template is not part of a placeholder" if pieces.first.include?("}")
      pieces.drop(1).each do |piece|
        break if error

        name, brace, after = piece.partition("}")
        error = if brace.empty? || after.include?("}")
                  "a brace in the template is not part of a placeholder"
                elsif !PLACEHOLDERS.include?(name)
                  "{#{name}} is not a placeholder, which is one of {workspace}, {checkout} and {home}"
                end
      end
      return ["path", [["path", text]]] unless error

      report(:unknown_placeholder, "`#{field.path}` is #{text.inspect}, and #{error}")
      nil
    end

    def one_password(field, text)
      parts = text.start_with?("op://") ? text.delete_prefix("op://").split("/", -1) : []
      return text if parts.length.between?(3, 4) && parts.none? { |part| part.empty? || part.match?(/[[:cntrl:]]/) }

      report(:bad_value, "`#{field.path}` is #{text.inspect}, and a one-password reference is op://vault/item/field")
      nil
    end

    def cache(field)
      return true unless field

      boolean(field)
    end

    def provided(field, table)
      kind_field = field.child(table, "provided")
      kind = confirmed(kind_field)&.then do |text|
        host, slash, rest = text.partition("/")
        next text if !slash.empty? && Names.lower_word?(host) && Names.lower_word?(rest)

        report(:bad_value, "`#{kind_field.path}` is #{text.inspect}, and a provider kind is <host>/<kind>")
        nil
      end
      whole = true
      qualifiers = {}
      table.each do |key, value|
        next if key == "provided"

        if key == "cache"
          report(:unknown_field, "`#{field.path}` has the field `cache`, which only a `one-password` bind takes")
          whole = false
          next
        end
        text = string(Field.new(path: "#{field.path}.#{key}", key: key, item: value))
        text ? qualifiers[key] = text : whole = false
      end
      return nil unless kind && whole

      bad = qualifiers.keys.sort.find { |key| BIND_KINDS.include?(key) || !Names.lower_word?(key) }
      return ["provided", [["provided", kind], *qualifiers.sort]] unless bad

      report(:bad_value, "`#{field.path}` has the qualifier #{bad.inspect}, and a qualifier is a lower case word")
      nil
    end

    def source(field, kind, bind, bytes_field, rule)
      bytes = generated_bytes(field, kind, bytes_field, !rule.nil?)
      return nil if bind == FAILED

      bind_kind, bind_fields = bind
      case [kind, bind_kind]
      when ["setting", nil], %w[setting path], ["secret", nil], %w[secret one-password], ["inherited", nil], %w[provided provided]
        source = Source.new(kind: kind, bind: bind_fields, bytes: nil)
      when ["generated", nil]
        count = rule ? generated_length(field, rule) : bytes || 32
        return nil if count.nil? || count == FAILED

        source = Source.new(kind: kind, bind: nil, bytes: rule ? nil : count)
      when ["provided", nil]
        report(:bind_mismatch, "`#{field.path}.source` is \"provided\", and a provided variable needs a provided bind")
        return nil
      else
        report(:bind_mismatch, "`#{field.path}.bind` is a #{bind_kind} bind, and a #{kind} variable does not take it")
        return nil
      end
      source unless bytes == FAILED
    end

    def generated_bytes(field, kind, bytes, typed)
      return nil unless bytes

      if kind != "generated"
        report(:bad_bytes, "`#{field.path}` sets bytes, and only a generated variable takes bytes")
        return FAILED
      end
      if typed
        report(:bad_bytes, "`#{bytes.path}` is set, and a generated variable with a bytes type takes its length from the type")
        return FAILED
      end
      count(bytes, 16) || FAILED
    end

    def generated_length(field, rule)
      return rule.length if rule.length.between?(16, 1024)

      report(:bad_bytes, "`#{field.path}.type` asks for #{rule.length} bytes, and a generated value has 16 to 1024 bytes")
      nil
    end

    def tools(field)
      table = table(field) or return nil
      parsed = true
      tools = table.filter_map do |key, item|
        if key.empty?
          report(:bad_value, "`#{field.path}` has an empty key, and a tool's name is not empty")
          parsed = false
          next
        end
        tool = table(Field.new(path: "#{field.path}.#{key}", key: key, item: item))
        parsed = false unless tool
        ToolTable.new(name: key, table: tool) if tool
      end
      tools.sort_by(&:name) if parsed
    end

    def enumerations(field)
      table = table(field) or return {}
      table.to_h do |key, item|
        [key, enumeration(Field.new(path: "enumerations.#{key}", key: key, item: item))]
      end
    end

    def enumeration(field)
      name = field.key
      unless Names.lower_word?(name)
        report(:bad_value, "`#{field.path}` is not an enumeration name, which is lower case letters, digits and hyphens")
        name = nil
      end
      table = table(field) or return nil
      if table.empty?
        report(:empty_enumeration, "`#{field.path}` has no values, and it lists at least one")
        return nil
      end
      whole = true
      descriptions = table.to_h do |key, item|
        value = Field.new(path: "#{field.path}.#{key}", key: key, item: item)
        if key.empty? || key.match?(/[[:space:]]/)
          report(:bad_value, "`#{value.path}` is #{key.inspect}, and a value is not empty and holds no whitespace")
          whole = false
        end
        description = sentence(value, "a description")
        whole = false unless description
        [key, description]
      end
      Types::Enumeration.new(choices: descriptions.keys, name: name, descriptions: descriptions) if name && whole
    end

    def families(field)
      table = table(field) or return []
      prefixes = []
      families = table.filter_map do |key, item|
        family = Field.new(path: "families.#{key}", key: key, item: item)
        prefix = name(family)
        prefixes << prefix if prefix
        family(family, prefix)
      end
      prefixes.each do |longer|
        shorter = prefixes.find { |prefix| longer.start_with?("#{prefix}_") && Names.key?(longer.delete_prefix("#{prefix}_")) }
        next unless shorter

        report(:bad_family, "`families.#{longer}` is a member name of the family #{shorter}, so the two families would share members")
      end
      families.sort_by(&:prefix)
    end

    def family(field, prefix)
      _, variable = variable(field, FAMILY_FIELDS)
      table = field.item
      return nil unless table.is_a?(Hash)

      keys = optional(field.child(table, "keys")) { |keys| keys(keys, prefix) }
      return nil if variable.nil? || prefix.nil? || keys == FAILED
      return nil unless family_rules(field, table, variable, !keys.nil?)

      Family.new(prefix: prefix, variable: variable, keys: keys)
    end

    def keys(field, prefix)
      keys = texts(field) do |text|
        unless Names.key?(text)
          "a key starts with an upper case letter or a digit and holds only upper case letters, digits and underscores"
        end
      end
      return nil unless keys

      if keys.empty?
        report(:bad_family, "`#{field.path}` lists no keys, and a family that lists its keys lists at least one")
        return nil
      end
      keys = keys.sort
      whole = true
      repeat = keys.each_cons(2).find { |first, second| first == second }&.first
      if repeat
        report(:bad_family, "`#{field.path}` lists #{repeat.inspect} twice, and a family lists each key once")
        whole = false
      end
      keys.each do |key|
        bytes = (prefix || "").bytesize + 1 + key.bytesize
        next unless prefix && bytes > Names::MAX_BYTES

        report(:bad_family, "`#{field.path}` lists #{key.inspect}, which makes the member name #{bytes} bytes long")
        whole = false
      end
      keys if whole
    end

    def family_rules(field, table, variable, listed)
      whole = true
      if variable.unset.local.is_a?(Default)
        report(:bad_family, "`#{field.path}.unset` sets a default, and a family has none")
        whole = false
      end
      if !listed && [variable.unset.local, variable.unset.production].include?(:required)
        report(:bad_family, "`#{field.path}.unset` requires a value, and only a family with `keys` may be required")
        whole = false
      end
      source = variable.source
      unless source.kind == "inherited" || (source.kind == "setting" && source.bind.nil?)
        key = source.kind == "setting" ? "bind" : "source"
        report(:bad_family, "`#{field.path}.#{key}` is refused, and a family is a setting without a bind or inherited")
        whole = false
      end
      _ = table
      whole
    end

    def not_a_member(field, name, families)
      return unless name

      family = families.find { |declared| declared.key_of(name) }
      return unless family

      report(:bad_family, "`#{field.path}` is a member of the family #{family.prefix}, and a declared variable is no family's member")
    end

    def mode(field, declared, variables)
      table = table(field) or return nil
      known(table, "mode", MODE_FIELDS)
      variable = required(field, table, "variable")
      name = variable && string(variable)&.then do |text|
        error = Names.error(text)
        report(:bad_name, "`mode.variable` is #{text.inspect}, and #{error}") if error
        text unless error
      end
      production = required(field, table, "production")&.then { |list| choices(list) }
      return nil unless name

      entry = declared.find { |declared_name, _| declared_name == name }
      kind = entry&.last
      if entry.nil?
        report(:mode_undeclared, "`mode.variable` is #{name.inspect}, which the file does not declare")
      elsif kind && !%w[setting inherited].include?(kind)
        report(:mode_source, "`mode.variable` is #{name.inspect}, which is a #{kind} variable, and the mode variable " \
                             "is a setting or inherited variable")
      elsif variables.any? { |declared_name, declaration| declared_name == name && declaration.withheld? }
        report(:mode_source, "`mode.variable` is #{name.inspect}, which is withheld, and the mode variable is shown")
      elsif production
        return ModeRule.new(variable: name, production: production)
      end
      nil
    end

    def groups(field, declared, variables, families)
      table = table(field) or return []
      members = []
      groups = table.filter_map do |key, item|
        group(Field.new(path: "groups.#{key}", key: key, item: item), declared, variables, families, members)
      end
      groups.sort_by(&:name)
    end

    def group(field, declared, variables, families, members)
      name = field.key
      unless Names.lower_word?(name)
        report(:bad_value, "`#{field.path}` is not a group name, which is lower case letters, digits and hyphens")
        name = nil
      end
      table = table(field) or return nil
      known(table, field.path, GROUP_FIELDS)
      list = required(field, table, "variables") or return nil
      listed = group_members(list) or return nil
      whole = !name.nil?
      if listed.length < 2
        report(:bad_group, "`#{list.path}` lists #{listed.length} variables, and a group lists at least two")
        whole = false
      end
      kept = []
      listed.each do |member|
        problem = member_problem(member, kept, declared, variables, families, members)
        if problem
          report(problem.first, "`#{list.path}` lists #{member.inspect}#{problem.last}")
          whole = false
        end
        kept << member unless kept.include?(member)
      end
      return nil unless name

      kept.each { |member| members << [member, name] unless members.any? { |listed_name, _| listed_name == member } }
      Group.new(name: name, variables: kept) if whole
    end

    def group_members(field)
      unless field.item.is_a?(Array)
        wrong_type(field, "an array of variable names")
        return nil
      end
      parsed = field.item.each_with_index.filter_map do |value, index|
        path = "#{field.path}[#{index}]"
        unless value.is_a?(String)
          report(:bad_value, "`#{path}` is a toml #{type_name(value)}, and it takes a variable name")
          next
        end
        error = Names.error(value)
        report(:bad_name, "`#{path}` is #{value.inspect}, and #{error}") if error
        value unless error
      end
      parsed if parsed.length == field.item.length
    end

    def member_problem(member, kept, declared, variables, families, members)
      return [:bad_group, " twice, and a group lists each variable once"] if kept.include?(member)

      family = families.find { |declared_family| declared_family.prefix == member || declared_family.key_of(member) }
      return [:bad_group, ", which belongs to the family #{family.prefix}, and families cannot join groups"] if family

      entry = declared.find { |name, _| name == member }
      return [:group_undeclared, ", which the file does not declare"] unless entry
      return [:bad_group, ", which is generated, and a member of a group is not"] if entry.last == "generated"

      defaulted = variables.any? { |name, variable| name == member && variable.unset.local.is_a?(Default) }
      return [:bad_group, ", which has a default, and a member of a group has none"] if defaulted

      other = members.find { |name, _| name == member }&.last
      [:bad_group, ", which the group #{other} lists too, and a variable belongs to at most one group"] if other
    end
  end

  # The error that refuses a starmap.toml. It lists every problem.
  class SpecError < StandardError
    attr_reader :problems

    def initialize(problems)
      @problems = problems.freeze
      super("starmap.toml is refused: #{problems.join("; ")}")
    end
  end
end
