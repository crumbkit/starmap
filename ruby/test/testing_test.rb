# frozen_string_literal: true

require_relative "test_helper"
require "tmpdir"
require "fileutils"

class TestingTest < Minitest::Test
  SPEC = <<~TOML
    [families.SHELF_DB_URL]
    purpose = "The database of one region, keyed by its code."
    type = "url"
    unset = "optional"
    show = "shown"
    source = "setting"
    keys = ["EU"]

    [variables.SHELF_PORT]
    purpose = "The port the catalogue listens on."
    type = "port"
    unset = { default = "8080" }
    show = "shown"
    source = "setting"
  TOML

  def setup
    @root = Dir.mktmpdir
    File.write(File.join(@root, "starmap.toml"), SPEC)
  end

  def teardown
    FileUtils.remove_entry(@root)
  end

  def write(path, text)
    path = File.join(@root, path)
    FileUtils.mkdir_p(File.dirname(path))
    File.write(path, text)
  end

  def test_a_literal_read_of_an_undeclared_name_in_app_lib_or_config_is_reported
    write("app/models/shelf.rb", %(port = ENV["SHELF_PORT"]\nhost = ENV['SHELF_HOST']\n))
    write("lib/tasks/shelf.rake", %(ENV.fetch("SHELF_TIMEOUT", "5")\n))
    write("config/database.yml", %(url: <%= ENV.key?("SHELF_DATABASE") %>\n))

    reads = Starmap::Testing.undeclared_reads(root: @root)

    assert_equal([["app/models/shelf.rb", 2, "SHELF_HOST"], ["config/database.yml", 1, "SHELF_DATABASE"],
                  ["lib/tasks/shelf.rake", 1, "SHELF_TIMEOUT"]],
                 reads.map { |read| [read.path, read.line, read.name] })
  end

  def test_a_member_of_a_listed_family_counts_as_declared_only_for_a_listed_key
    write("app/db.rb", %(ENV["SHELF_DB_URL_EU"]\nENV["SHELF_DB_URL_US"]\n))

    assert_equal ["SHELF_DB_URL_US"], Starmap::Testing.undeclared_reads(root: @root).map(&:name)
  end

  def test_reads_outside_the_three_directories_and_in_comments_are_not_scanned
    write("script/run.rb", %(ENV["SHELF_HOST"]\n))
    write("app/shelf.rb", %(# ENV["SHELF_HOST"] is read by the proxy\n))

    assert_empty Starmap::Testing.undeclared_reads(root: @root)
  end

  def test_an_allowed_name_is_not_reported
    write("config/boot.rb", %(ENV["BUNDLE_GEMFILE"]\n))

    assert_empty Starmap::Testing.undeclared_reads(root: @root, allow: ["BUNDLE_GEMFILE"])
  end

  def test_assert_declared_raises_one_error_that_names_every_undeclared_read
    write("app/shelf.rb", %(ENV["SHELF_HOST"]\nENV["SHELF_USER"]\n))

    error = assert_raises(Starmap::Testing::UndeclaredReads) { Starmap::Testing.assert_declared!(root: @root) }
    assert_includes error.message, "app/shelf.rb:1 reads SHELF_HOST"
    assert_includes error.message, "app/shelf.rb:2 reads SHELF_USER"
  end
end
