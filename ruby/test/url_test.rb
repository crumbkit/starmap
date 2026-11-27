# frozen_string_literal: true

require_relative "test_helper"

# The url type serializes only the parts of the WHATWG URL Standard that
# applications use, so on some inputs it accepts or refuses a value
# differently from starmap 0.4.3. These tests pin the known differences.
# Under `starmap run` the child already receives the text starmap wrote,
# which the gem keeps as it is.
class UrlTest < Minitest::Test
  URL = Starmap::Types::Url.new(schemes: nil)

  def text(raw) = URL.parse(raw).text

  def test_the_url_type_lowercases_the_scheme_and_host_and_drops_the_default_port
    assert_equal "https://library.example/", text("HTTPS://Library.Example:443")
    assert_equal "postgres://db.example/library", text("POSTGRES://db.example/library")
  end

  def test_the_url_type_accepts_hosts_that_starmap_refuses
    assert_equal "http://1.2.3.999/", text("http://1.2.3.999/")
    assert_equal "http://[zz]/", text("http://[zz]/")
  end

  def test_the_url_type_refuses_urls_that_starmap_accepts
    assert_raises(Starmap::Invalid) { URL.parse("http:example.com") }
    assert_raises(Starmap::Invalid) { URL.parse("http://a%41.example/") }
  end

  def test_the_url_type_keeps_a_host_that_starmap_rewrites
    assert_equal "http://0x7f.1/", text("http://0x7f.1/")
  end
end
