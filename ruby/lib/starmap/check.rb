# frozen_string_literal: true

require "json"

module Starmap
  # What a check holds for a variable. +kind+ is :shown for a value its
  # type read, :presence for a variable that reads its presence, whose
  # +raw+ is nil when it is absent, or :withheld for a withheld value,
  # whose +raw+ is the text as it was set.
  Held = Data.define(:kind, :value, :raw) do
    def set? = kind != :presence || !raw.nil?

    # The text the child process receives, or nil when it stays unset.
    def text = kind == :shown ? value.text : raw
  end

  # One reason the check refuses the environment, about one variable.
  # +problem+ is :missing, :invalid, :group_incomplete or :unlisted_key.
  Finding = Data.define(:name, :problem, :detail, :quote, :mode, :modes) do
    def code = Finding::CODES.fetch(problem)

    def to_s
      case problem
      when :missing then modes ? "#{name} is required in #{mode} mode and has no value" : "#{name} is required and has no value"
      when :invalid then "#{name}: #{detail}#{" (the value is #{quote})" if quote}"
      when :group_incomplete
        group, set = detail
        "#{name} is unset, and the group #{group} needs it because #{Check.list(set)} #{set.length == 1 ? "is" : "are"} set"
      when :unlisted_key
        prefix, keys = detail
        "#{name} is a member of the family #{prefix}, which lists only the keys #{Check.list(keys)}"
      end
    end
  end

  Finding::CODES = {
    missing: "starmap::variable::missing",
    invalid: "starmap::variable::invalid",
    group_incomplete: "starmap::group::incomplete",
    unlisted_key: "starmap::family::unlisted_key"
  }.freeze

  # The result of a check: the mode, the value held for each variable that
  # passed, and the findings that refuse the environment.
  Result = Data.define(:mode, :held, :findings) do
    def refused? = !findings.empty?

    # The text that each variable gives the child process. A variable that
    # stays unset is left out.
    def child_values = held.filter_map { |name, value| [name, value.text] if value&.text }.to_h
  end

  # The check of an environment against a spec, as docs/glossary.md and
  # the Rust crate's check describe it. It reads the whole file, decides
  # the mode, and reports every finding in the file's order.
  module Check
    QUOTE_CHARACTERS = 64

    module_function

    # Checks +environment+, a hash of names and values, against +spec+.
    def run(spec, environment)
      bindings = capture(spec, environment)
      mode = spec.mode ? decide(spec, bindings) : :local
      holding = spec.variables.map { |name, variable| [name, variable, hold(variable, bindings[name], mode)] }
      held = []
      findings = []
      holding.each do |name, variable, (status, result)|
        next findings << finding(name, variable, bindings[name], result, mode) if status == :error

        incomplete = incomplete(spec, holding, name) unless result&.set?
        incomplete ? findings << finding(name, variable, nil, incomplete, mode) : held << [name, result]
      end
      spec.families.each { |family| check_family(family, bindings, mode, held, findings) }
      findings = findings.map { |found| found.with(modes: !spec.mode.nil?) }
      Result.new(mode: mode, held: held, findings: findings)
    end

    # The values the spec declares, without the empty values that count as
    # unset, in name order.
    def capture(spec, environment)
      environment.to_h.filter_map do |name, raw|
        name = name.to_s
        next unless Names.error(name).nil?

        declaration = spec.variable(name) || spec.family_of(name)&.variable
        next unless declaration
        next if raw.to_s.empty? && !declaration.empty_is_value?

        [name, raw.to_s]
      end.sort.to_h
    end

    def decide(spec, bindings)
      rule = spec.mode
      variable = spec.variable(rule.variable) or return :local
      default = variable.unset.local.is_a?(Default) ? variable.unset.local.value.text : nil
      raw = bindings[rule.variable]
      return rule.decide(default) if raw.nil?

      status, held = read(variable, raw)
      return rule.decide(default) if (status == :ok && held.nil?) || (status == :error && held.last == Types::NOT_UNICODE)
      return :local if status == :error || held.kind == :withheld

      rule.decide(held.kind == :presence ? held.raw : held.value.text)
    end

    def hold(variable, raw, mode)
      if raw
        status, held = read(variable, raw)
        return [status, held] if status == :error || held
      end
      case (rule = variable.unset.in_mode(mode))
      when :required then [:error, [:missing]]
      when :optional then [:ok, variable.presence ? Held.new(kind: :presence, value: false, raw: nil) : nil]
      else [:ok, Held.new(kind: :shown, value: rule.value, raw: nil)]
      end
    end

    def read(variable, raw)
      return [:ok, Held.new(kind: :presence, value: true, raw: raw)] if variable.presence
      return [:ok, nil] if raw.empty? && variable.empty == "unset"

      if variable.withheld?
        text = Types.text(raw)
        decoded = variable.type.check_secret(text)
        return [:ok, Held.new(kind: :withheld, value: Secret.new(decoded.is_a?(String) ? decoded : text), raw: text)]
      end
      value = variable.type.parse(raw)
      [:ok, value && Held.new(kind: :shown, value: value, raw: raw)]
    rescue Invalid => e
      [:error, [:invalid, e.message]]
    end

    def incomplete(spec, holding, name)
      group = spec.group_of(name) or return nil
      set = group.variables.select do |member|
        holding.any? { |held_name, _, (status, held)| held_name == member && status == :ok && held&.set? }
      end
      [:group_incomplete, [group.name, set]] unless set.empty?
    end

    def check_family(family, bindings, mode, held, findings)
      variable = family.variable
      found = []
      checked = []
      bindings.each do |name, raw|
        key = family.key_of(name) or next
        next found << finding(name, variable, nil, [:unlisted_key, [family.prefix, family.keys]], mode) unless family.lists?(key)

        status, result = read(variable, raw)
        next found << finding(name, variable, raw, result, mode) if status == :error

        checked << [name, result] if result
      end
      if family.keys && variable.unset.in_mode(mode) == :required
        family.keys.map { |key| "#{family.prefix}_#{key}" }.each do |name|
          next if checked.any? { |held_name, _| held_name == name } || found.any? { |finding| finding.name == name }

          found << finding(name, variable, nil, [:missing], mode)
        end
      end
      held.concat(checked)
      findings.concat(found.sort_by(&:name))
    end

    def finding(name, variable, raw, (problem, detail), mode)
      quote = quote(raw) if problem == :invalid && !variable.withheld? && raw
      Finding.new(name: name, problem: problem, detail: detail, quote: quote, mode: mode, modes: false)
    end

    # A shown value quoted as a JSON string, cut at 64 characters.
    def quote(raw)
      text = raw.dup.force_encoding(Encoding::UTF_8).scrub("�")
      text = "#{text[0, QUOTE_CHARACTERS]}…" if text.length > QUOTE_CHARACTERS
      JSON.generate(text)
    end

    def list(items)
      return items.join if items.length < 2

      "#{items[0..-2].join(", ")} and #{items.last}"
    end
  end
end
