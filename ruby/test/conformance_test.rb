# frozen_string_literal: true

require_relative "test_helper"

# Runs every case under the repository's conformance/ directory, which the
# Rust crate's tests run too, and compares the gem's result with the case's
# expectation. conformance/README.md gives the layout and the rules.
class ConformanceTest < Minitest::Test
  ROOT = File.expand_path("../../conformance", __dir__)
  RUST_TESTS = File.expand_path("../../crates/starmap/tests", __dir__)
  ROOT_ENTRIES = %w[.gitattributes README.md check parse].freeze
  PARSE_FILES = %w[canonical.toml case.toml starmap.toml].freeze
  CHECK_FILES = %w[case.toml starmap.toml].freeze

  def test_the_conformance_root_holds_only_the_entries_the_layout_names
    assert_equal ROOT_ENTRIES, Dir.children(ROOT).sort
  end

  def test_every_parse_case_gives_the_problems_or_the_canonical_form_and_digest_it_expects
    failures = cases("parse").filter_map { |dir| parse_failure(dir) }

    assert_empty failures, failures.join("\n")
  end

  def test_every_check_case_gives_the_mode_and_values_or_the_refusals_it_expects
    failures = cases("check").filter_map { |dir| check_failure(dir) }

    assert_empty failures, failures.join("\n")
  end

  def test_the_cases_cover_every_spec_code_but_too_large
    seen = cases("parse").flat_map do |dir|
      Array(PerfectTOML.parse(File.read(File.join(dir, "case.toml"), encoding: "UTF-8")).dig("expect", "problems"))
    end
    assert_equal Starmap::Problem::CODES.values.uniq.sort - ["starmap::spec::too_large"], seen.uniq.sort
  end

  def test_every_example_spec_that_the_rust_tests_embed_parses
    names = Dir.children(File.join(RUST_TESTS, "example")).select { |name| name.end_with?(".toml") } - %w[Cargo.toml clippy.toml]
    failures = names.sort.filter_map do |name|
      Starmap::Spec.parse(File.read(File.join(RUST_TESTS, "example", name), encoding: "UTF-8"))
      nil
    rescue Starmap::SpecError => e
      "example/#{name}: refused with #{e.problems.map(&:code).inspect}"
    end

    assert_equal 12, names.length
    assert_empty failures, failures.join("\n")
  end

  def test_every_problem_fixture_of_the_rust_tests_gives_the_codes_of_its_golden_in_order
    fixtures = Dir.glob(File.join(RUST_TESTS, "spec", "*.toml"))
    failures = fixtures.sort.filter_map do |path|
      expected = File.read(path.sub(/\.toml\z/, ".txt"), encoding: "UTF-8").scan(/^(starmap::spec::[a-z_]+):/).flatten
      parse_problems_failure(path, File.read(path, encoding: "UTF-8"), expected)
    end

    assert_equal 25, fixtures.length
    assert_empty failures, failures.join("\n")
  end

  def test_a_file_over_the_size_limit_gives_only_too_large
    text = "# #{"a" * Starmap::Spec::MAX_BYTES}\n"
    error = assert_raises(Starmap::SpecError) { Starmap::Spec.parse(text) }

    assert_equal ["starmap::spec::too_large"], error.problems.map(&:code)
  end

  private

  def cases(kind)
    Dir.children(File.join(ROOT, kind)).sort.map { |name| File.join(ROOT, kind, name) }
  end

  def relative(dir)
    dir.delete_prefix("#{File.dirname(ROOT)}/")
  end

  def read_case(dir, files, keys)
    present = Dir.children(dir).sort
    expected = files.dup
    expected.delete("canonical.toml") if files.include?("canonical.toml") && !present.include?("canonical.toml")
    return [nil, "#{relative(dir)}: holds #{present.inspect}, and the layout names #{expected.inspect}"] unless present == expected

    data = PerfectTOML.parse(File.read(File.join(dir, "case.toml"), encoding: "UTF-8"))
    unknown = data.keys - keys
    return [nil, "#{relative(dir)}: case.toml has the keys #{unknown.inspect}"] unless unknown.empty?

    [data, nil]
  end

  def parse_failure(dir)
    data, failure = read_case(dir, PARSE_FILES, %w[description expect])
    return failure if failure

    text = File.read(File.join(dir, "starmap.toml"), encoding: "UTF-8")
    expect = data.fetch("expect")
    unless [["digest"], ["problems"]].include?(expect.keys)
      return "#{relative(dir)}: expect holds #{expect.keys.inspect}, and a parse case expects exactly one of digest and problems"
    end
    if expect.key?("problems") == File.exist?(File.join(dir, "canonical.toml"))
      return "#{relative(dir)}: a case holds canonical.toml exactly when it expects a digest"
    end

    if expect.key?("problems")
      parse_problems_failure(dir, text, expect.fetch("problems"))
    else
      digest_failure(dir, text, expect.fetch("digest"))
    end
  end

  def parse_problems_failure(dir, text, expected)
    Starmap::Spec.parse(text)
    "#{relative(dir)}: parsed, and the case expects #{expected.inspect}"
  rescue Starmap::SpecError => e
    actual = e.problems.map(&:code)
    "#{relative(dir)}: gave #{actual.inspect}, and the case expects #{expected.inspect}" unless actual == expected
  end

  def digest_failure(dir, text, expected)
    spec = Starmap::Spec.parse(text)
    canonical = File.read(File.join(dir, "canonical.toml"), encoding: "UTF-8")
    return "#{relative(dir)}: wrote\n#{spec.to_toml}\nand the case expects\n#{canonical}" unless spec.to_toml == canonical
    return "#{relative(dir)}: gave #{spec.digest}, and the case expects #{expected}" unless spec.digest == expected

    again = Starmap::Spec.parse(canonical).digest
    "#{relative(dir)}: canonical.toml gave #{again}" unless again == expected
  rescue Starmap::SpecError => e
    "#{relative(dir)}: refused with #{e.problems.map(&:code).inspect}, and the case expects a digest"
  end

  def check_failure(dir)
    data, failure = read_case(dir, CHECK_FILES, %w[description environment expect])
    return failure if failure

    spec = Starmap::Spec.parse(File.read(File.join(dir, "starmap.toml"), encoding: "UTF-8"))
    result = Starmap::Check.run(spec, data.fetch("environment", {}))
    actual = describe(result)
    expected = data.fetch("expect")
    "#{relative(dir)}: gave #{actual.inspect}, and the case expects #{expected.inspect}" unless actual == expected
  rescue Starmap::SpecError => e
    "#{relative(dir)}: starmap.toml is refused with #{e.problems.map(&:code).inspect}, and a check case always parses"
  end

  def describe(result)
    if result.refused?
      { "refused" => result.findings.map { |finding| { "variable" => finding.name, "code" => finding.code } } }
    else
      { "mode" => result.mode.to_s, "values" => result.child_values }
    end
  end
end
