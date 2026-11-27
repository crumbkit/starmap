# frozen_string_literal: true

require_relative "test_helper"
require "tmpdir"
require "fileutils"

class LoadTest < Minitest::Test
  SPEC = <<~TOML
    [mode]
    variable = "SHELF_ENV"
    production = ["prod"]

    [tool.codegen]
    module = "Shelf"

    [families.SHELF_DB_URL]
    purpose = "The database of one region, keyed by its code."
    type = "url"
    unset = "optional"
    show = "shown"
    source = "setting"

    [variables.SHELF_ENV]
    purpose = "Selects local or production behaviour."
    type = { enumeration = ["dev", "prod"] }
    unset = { default = "dev" }
    show = "shown"
    source = "setting"

    [variables.SHELF_PORT]
    purpose = "The port the catalogue listens on."
    type = "port"
    unset = { default = "8080" }
    show = "shown"
    source = "setting"

    [variables.SHELF_DEBUG]
    purpose = "Logs each catalogue query."
    type = "boolean"
    unset = "optional"
    show = "shown"
    source = "setting"

    [variables.SHELF_DATA_DIR]
    purpose = "The directory that holds the catalogue."
    type = "path"
    unset = { default = "data" }
    show = "shown"
    source = "setting"

    [variables.LIBRARY_TOKEN]
    purpose = "The token the library service accepts."
    type = { string = { pattern = "tok-[a-z0-9]+" } }
    unset = { local = "optional", production = "required" }
    show = "withheld"
    source = "setting"

    [variables.LIBRARY_TOKEN.tool.codegen]
    name = "token"
  TOML

  def setup
    @root = Dir.mktmpdir
    @spec = File.join(@root, "starmap.toml")
    File.write(@spec, SPEC)
    Starmap.instance_variable_set(:@env, nil)
  end

  def teardown
    FileUtils.remove_entry(@root)
    Starmap.instance_variable_set(:@env, nil)
  end

  def load(environment)
    Starmap.load!(path: @spec, environment: environment)
  end

  def test_load_reads_each_variable_in_its_type_with_defaults_applied
    load("SHELF_PORT" => "9090", "SHELF_DEBUG" => "Yes", "LIBRARY_TOKEN" => "tok-1", "UNRELATED" => "x")
    env = Starmap.env

    assert_equal 9090, env["SHELF_PORT"]
    assert(env["SHELF_DEBUG"])
    assert_equal Pathname.new("data"), env["SHELF_DATA_DIR"]
    assert_equal "dev", env["SHELF_ENV"]
    assert_equal "tok-1", env["LIBRARY_TOKEN"].reveal
    assert_predicate env, :local?
  end

  def test_an_optional_variable_that_is_unset_reads_as_nil
    load({})

    assert_nil Starmap.env["SHELF_DEBUG"]
    assert_nil Starmap.env["LIBRARY_TOKEN"]
  end

  def test_the_mode_variable_makes_a_production_rule_apply
    error = assert_raises(Starmap::Error) { load("SHELF_ENV" => "prod") }

    assert_equal ["LIBRARY_TOKEN"], error.findings.map(&:name)
    assert_includes error.message, "LIBRARY_TOKEN is required in production mode and has no value"
  end

  def test_load_raises_one_error_that_lists_every_finding_and_quotes_only_shown_values
    error = assert_raises(Starmap::Error) do
      load("SHELF_PORT" => "80a", "SHELF_DEBUG" => "perhaps", "LIBRARY_TOKEN" => "secret-value-123")
    end

    assert_equal %w[SHELF_PORT SHELF_DEBUG LIBRARY_TOKEN], error.findings.map(&:name)
    assert_includes error.message, 'SHELF_PORT: the value is not a port from 1 to 65535 (the value is "80a")'
    assert_includes error.message, '(the value is "perhaps")'
    assert_includes error.message, "LIBRARY_TOKEN: the value does not match the pattern tok-[a-z0-9]+"
    refute_includes error.message, "secret-value-123"
  end

  def test_a_failed_load_leaves_starmap_env_unread
    assert_raises(Starmap::Error) { load("SHELF_PORT" => "0") }

    assert_raises(Starmap::NotLoaded) { Starmap.env }
  end

  def test_load_reads_the_environment_once
    environment = { "SHELF_PORT" => "9090" }
    load(environment)
    environment["SHELF_PORT"] = "7070"

    assert_equal 9090, Starmap.env["SHELF_PORT"]
  end

  def test_a_name_the_file_does_not_declare_cannot_be_read
    load({})

    assert_raises(Starmap::Undeclared) { Starmap.env["SHELF_HOST"] }
  end

  def test_a_family_reads_its_set_members_by_key
    load("SHELF_DB_URL_US" => "https://US.example", "SHELF_DB_URL_EU" => "https://eu.example/")

    assert_equal({ "EU" => "https://eu.example/", "US" => "https://us.example/" }, Starmap.env.family("SHELF_DB_URL"))
    assert_equal "https://eu.example/", Starmap.env["SHELF_DB_URL_EU"]
  end

  def test_a_withheld_value_never_shows_in_inspect
    load("LIBRARY_TOKEN" => "tok-hidden")

    refute_includes Starmap.env["LIBRARY_TOKEN"].inspect, "tok-hidden"
    refute_includes Starmap.env.inspect, "tok-hidden"
  end

  def test_a_file_with_an_unknown_key_is_refused_and_tool_tables_are_accepted
    File.write(@spec, "#{SPEC}\n[variables.SHELF_HOST]\npurpose = \"The host.\"\ntype = \"string\"\n" \
                      "unset = \"optional\"\nshow = \"shown\"\nsource = \"setting\"\ncolour = \"blue\"\n")

    error = assert_raises(Starmap::SpecError) { load({}) }
    assert_equal ["starmap::spec::unknown_field"], error.problems.map(&:code)
  end

  def test_a_pointer_file_is_followed_to_the_file_it_names
    FileUtils.mkdir_p(File.join(@root, "config"))
    FileUtils.mv(@spec, File.join(@root, "config", "environment.toml"))
    File.write(@spec, %(spec = "config/environment.toml"\n))
    load("SHELF_PORT" => "9091")

    assert_equal 9091, Starmap.env["SHELF_PORT"]
  end

  def test_a_pointer_that_names_another_pointer_is_refused
    File.write(File.join(@root, "other.toml"), %(spec = "starmap.toml"\n))
    File.write(@spec, %(spec = "other.toml"\n))

    error = assert_raises(Starmap::SpecError) { load({}) }
    assert_equal ["starmap::spec::pointer_chain"], error.problems.map(&:code)
  end
end
