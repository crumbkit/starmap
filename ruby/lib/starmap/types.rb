# frozen_string_literal: true

require "ipaddr"
require "pathname"

module Starmap
  # A value that a type read. +text+ is what the child process receives,
  # and +ruby+ is the value that Starmap.env returns.
  Value = Data.define(:text, :ruby)

  # A value that does not fit its type. The reason never holds the value.
  class Invalid < StandardError; end

  # The types of the +type+ field, which docs/glossary.md describes. Each
  # type's #parse returns a Value, nil when the value counts as unset, or
  # raises Invalid.
  module Types
    NOT_UNICODE = "the value is not unicode text"

    def self.text(raw)
      text = raw.dup.force_encoding(Encoding::UTF_8)
      raise Invalid, NOT_UNICODE unless text.valid_encoding?

      text
    end

    # A path, which holds the value as it is.
    Path = Data.define(:absolute) do
      def word = absolute ? nil : "path"
      def to_s = absolute ? "an absolute path" : "a path"

      def parse(raw)
        return nil if absolute && !raw.start_with?("/")

        Value.new(text: raw, ruby: Pathname.new(raw))
      end

      # The path written from its components, as Rust's Path::components
      # reads them on Unix.
      def canonical(text)
        root = text.start_with?("/")
        parts = text.split("/").reject(&:empty?)
        kept = parts.each_with_index.reject { |part, index| part == "." && (root || index.positive?) }.map(&:first)
        joined = kept.join("/")
        root ? "/#{joined}" : joined
      end
    end

    # Unicode text, which a pattern may have to match as a whole.
    StringType = Data.define(:pattern) do
      def word = pattern ? nil : "string"
      def to_s = pattern ? "text that matches the pattern #{pattern.source}" : "text"

      def parse(raw)
        text = Types.text(raw)
        pattern&.check(text)
        Value.new(text: text, ruby: text)
      end

      def check_secret(text)
        pattern&.check(text)
      end
    end

    # A pattern, anchored so that it matches the whole value.
    class Pattern
      MAX_BYTES = 1024

      attr_reader :source

      def initialize(source)
        raise ArgumentError, "the pattern has #{source.bytesize} bytes, and the limit is #{MAX_BYTES}" if source.bytesize > MAX_BYTES

        @source = source
        @whole = Pattern.compile(source)
      rescue RegexpError => e
        raise ArgumentError, e.message
      end

      # Compiles the pattern alone, so that a pattern such as a)|(b cannot
      # close the group that anchors it, and then anchored. Ruby's warnings
      # are silenced, because the parser reports a pattern that is refused.
      def self.compile(source)
        verbose = $VERBOSE
        $VERBOSE = nil
        Regexp.new(source)
        Regexp.new("\\A(?:#{source})\\z")
      ensure
        $VERBOSE = verbose
      end

      def check(text)
        raise Invalid, "the value does not match the pattern #{source}" unless @whole.match?(text)
      end

      def ==(other) = other.is_a?(Pattern) && other.source == source
    end

    # A port from 1 to 65535.
    class Port
      def word = "port"
      def to_s = "a port"

      def parse(raw)
        text = Types.text(raw)
        number = text.match?(/\A\+?[0-9]+\z/) ? text.to_i : 0
        raise Invalid, "the value is not a port from 1 to 65535" unless number.between?(1, 65_535)

        Value.new(text: number.to_s, ruby: number)
      end

      def ==(other) = other.is_a?(Port)
    end

    # A whole number from +min+ to +max+, both included.
    IntegerType = Data.define(:min, :max) do
      def word = nil
      def to_s = "a whole number from #{min} to #{max}"

      def parse(raw)
        text = Types.text(raw)
        raise Invalid, "the value is not a whole number" unless text.match?(/\A[+-]?[0-9]+\z/)

        number = text.to_i
        raise Invalid, "the value is not a whole number" unless number.between?(-(2**63), (2**63) - 1)
        raise Invalid, "the value is not from #{min} to #{max}" unless number.between?(min, max)

        Value.new(text: number.to_s, ruby: number)
      end
    end

    # One of a list of values, written inline or named.
    Enumeration = Data.define(:choices, :name, :descriptions) do
      def word = nil
      def to_s = "one of #{choices.join(", ")}"

      def parse(raw)
        text = Types.text(raw)
        raise Invalid, "the value is empty, and it takes one of #{choices.join(", ")}" if text.empty?
        raise Invalid, "the value is not one of #{choices.join(", ")}" unless choices.include?(text)

        Value.new(text: text, ruby: text)
      end
    end

    # A boolean, read from the eight words or from declared spellings.
    TRUE_WORDS = %w[1 true yes on].freeze
    FALSE_WORDS = %w[0 false no off].freeze

    Boolean = Data.define(:for_true, :for_false) do
      def word = for_true ? nil : "boolean"
      def to_s = for_true ? "a boolean spelled #{for_true.join(", ")} for true and #{for_false.join(", ")} for false" : "a boolean"

      def parse(raw)
        text = Types.text(raw)
        return spelled(text) if for_true

        down = text.downcase(:ascii)
        return Value.new(text: "true", ruby: true) if TRUE_WORDS.include?(down)
        return Value.new(text: "false", ruby: false) if FALSE_WORDS.include?(down)

        raise Invalid, "the value is not a boolean"
      end

      private

      def spelled(text)
        return Value.new(text: text, ruby: true) if for_true.include?(text)
        return Value.new(text: text, ruby: false) if for_false.include?(text)

        raise Invalid, "the value is not one of #{for_true.join(", ")} for true and #{for_false.join(", ")} for false"
      end
    end

    # A URL, serialized as the WHATWG URL Standard writes it for the forms
    # that applications use.
    Url = Data.define(:schemes) do
      def word = schemes ? nil : "url"
      def to_s = schemes ? "a url whose scheme is one of #{schemes.join(", ")}" : "a url"

      def parse(raw)
        text = UrlText.serialize(Types.text(raw))
        scheme = text[/\A[^:]+/]
        raise Invalid, "the value is a url whose scheme is not one of #{schemes.join(", ")}" if schemes && !schemes.include?(scheme)

        Value.new(text: text, ruby: text)
      end
    end

    # An IP address and a port, such as 127.0.0.1:8080 or [::1]:8080.
    class SocketAddress
      def word = "socket-address"
      def to_s = "an ip socket address"

      def parse(raw)
        text = Types.text(raw)
        host, port = split(text)
        Value.new(text: "#{host}:#{port}", ruby: "#{host}:#{port}")
      end

      def ==(other) = other.is_a?(SocketAddress)

      private

      def split(text)
        if (match = text.match(/\A\[([0-9A-Fa-f:.]+)\]:(\+?[0-9]+)\z/))
          ["[#{IPAddr.new(match[1], Socket::AF_INET6)}]", port(match[2])]
        elsif (match = text.match(/\A((?:(?:0|[1-9][0-9]{0,2})\.){3}(?:0|[1-9][0-9]{0,2})):(\+?[0-9]+)\z/))
          raise IPAddr::InvalidAddressError unless match[1].split(".").all? { |octet| octet.to_i <= 255 }

          [match[1], port(match[2])]
        else
          raise IPAddr::InvalidAddressError
        end
      rescue IPAddr::Error
        raise Invalid, "the value is not an ip address and a port"
      end

      def port(text)
        number = text.to_i
        raise IPAddr::InvalidAddressError unless number <= 65_535

        number
      end
    end

    # Bytes written as text in an encoding, with an exact or a smallest
    # length after decoding.
    Bytes = Data.define(:encoding, :length, :at_least) do
      def word = nil
      def to_s = "#{encoding} bytes"

      def parse(raw)
        bytes = decode(Types.text(raw))
        Value.new(text: encode(bytes), ruby: bytes)
      end

      def check_secret(text)
        decode(text)
      end

      def decode(text)
        bytes = Codec.decode(encoding, text)
        raise Invalid, "the value is not #{encoding}" unless bytes
        raise Invalid, "the value does not decode to at least #{length} bytes" if at_least && bytes.bytesize < length
        raise Invalid, "the value does not decode to #{length} bytes" if !at_least && bytes.bytesize != length

        bytes
      end

      def encode(bytes) = Codec.encode(encoding, bytes)
    end

    # The three encodings of a bytes type.
    module Codec
      BASE64 = %r{\A(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?\z}
      BASE64URL_PADDED = /\A(?:[A-Za-z0-9_-]{4})*(?:[A-Za-z0-9_-]{2}==|[A-Za-z0-9_-]{3}=)?\z/
      BASE64URL_BARE = /\A(?:[A-Za-z0-9_-]{4})*(?:[A-Za-z0-9_-]{2,3})?\z/

      def self.decode(encoding, text)
        case encoding
        when "hex"
          [text].pack("H*") if text.match?(/\A(?:[0-9A-Fa-f]{2})*\z/)
        when "base64"
          text.unpack1("m0") if text.match?(BASE64)
        when "base64url"
          base64url(text)
        end
      rescue ArgumentError
        nil
      end

      def self.base64url(text)
        return nil unless text.match?(BASE64URL_PADDED) || text.match?(BASE64URL_BARE)

        bare = text.delete_suffix("=").delete_suffix("=")
        padded = bare + ("=" * ((4 - (bare.length % 4)) % 4))
        padded.tr("-_", "+/").unpack1("m0")
      end

      def self.encode(encoding, bytes)
        case encoding
        when "hex" then bytes.unpack1("H*")
        when "base64" then [bytes].pack("m0")
        when "base64url" then [bytes].pack("m0").tr("+/", "-_").delete("=")
        end
      end
    end

    # The serialization of a URL. It follows the WHATWG URL Standard for
    # absolute URLs: the scheme in lower case, and for a special scheme the
    # host in lower case, the default port left out and an empty path
    # written as /. It refuses text without a scheme, and a special URL
    # without a host.
    module UrlText
      SPECIAL = { "http" => 80, "https" => 443, "ws" => 80, "wss" => 443, "ftp" => 21, "file" => nil }.freeze

      def self.serialize(text)
        text = text.gsub(/\A[\u0000- ]+|[\u0000- ]+\z/, "").delete("\t\n\r")
        match = text.match(/\A([A-Za-z][A-Za-z0-9+.-]*):(.*)\z/m)
        raise Invalid, "the value is not a url" unless match

        scheme = match[1].downcase
        rest = match[2]
        SPECIAL.key?(scheme) ? special(scheme, rest) : "#{scheme}:#{rest}"
      end

      def self.special(scheme, rest)
        rest = rest.tr("\\", "/")
        raise Invalid, "the value is not a url" unless rest.start_with?("//")

        authority, path = rest[2..].match(%r{\A([^/?#]*)(.*)\z}m).captures
        userinfo, at, hostport = authority.rpartition("@")
        host, port = split_host(hostport)
        raise Invalid, "the value is not a url" if host.empty? && scheme != "file"

        port = nil if port == SPECIAL[scheme]
        path = "/#{path}" unless path.start_with?("/")
        "#{scheme}://#{userinfo}#{at}#{host}#{":#{port}" if port}#{path}"
      end

      def self.split_host(hostport)
        match = hostport.match(/\A(\[[^\]]*\]|[^:]*)(?::([0-9]*))?\z/)
        raise Invalid, "the value is not a url" unless match

        host = match[1].downcase
        raise Invalid, "the value is not a url" if host.match?(/[\s<>^|%]/)

        port = match[2].nil? || match[2].empty? ? nil : match[2].to_i
        raise Invalid, "the value is not a url" if port && port > 65_535

        [host, port]
      end
    end
  end
end
