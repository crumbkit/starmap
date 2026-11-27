# starmap for Ruby

The `starmap` gem reads the same `starmap.toml` that the Rust crates read.
A Ruby application calls `Starmap.load!` once at boot, and the gem checks
the process environment against the file: each variable's type, its
default, the `[mode]` rule and the rules of each mode. The application then
reads typed values from `Starmap.env`.

## Reading the environment at boot

In a Rails application, load the environment after dotenv has read its
files, such as at the top of `config/application.rb`:

```ruby
require "dotenv/load"
require "starmap"

Starmap.load!
```

`Starmap.load!` reads `starmap.toml` in the current directory, or the file
that `path:` names. A pointer file is followed once, to the file its
`spec` names. The gem reads `ENV` once, so a later change to `ENV` does not
change `Starmap.env`.

When the environment does not fit the file, `Starmap.load!` raises one
`Starmap::Error` that lists every finding, in the file's order:

```text
the environment does not fit starmap.toml: SHELF_PORT: the value is not a port from 1 to 65535 (the value is "80a"); LIBRARY_TOKEN is required in production mode and has no value
```

A finding about a shown variable quotes its value. A finding about a
withheld variable names only the rule it broke and never holds the value.
`Starmap::Error#findings` lists the findings, and each has a `name` and a
`code`, such as `starmap::variable::missing`. A file that starmap refuses,
including one with a key that starmap does not know, raises
`Starmap::SpecError`, whose `problems` have the same codes as the Rust
parser's. `[tool.*]` tables are accepted and never read.

## Reading values

```ruby
Starmap.env["SHELF_PORT"]        # => 8080
Starmap.env["SHELF_DEBUG"]       # => true
Starmap.env["LIBRARY_TOKEN"]     # => Secret(••••••••)
Starmap.env["LIBRARY_TOKEN"].reveal
Starmap.env.family("SHELF_DB_URL") # => { "EU" => "https://eu.example/" }
Starmap.env.production?
```

`Starmap.env[name]` returns nil for a variable that is unset, and raises
`Starmap::Undeclared` for a name that `starmap.toml` does not declare. Each
type gives a Ruby value:

| Type | Value |
| --- | --- |
| `port`, `integer` | `Integer` |
| `boolean` | `true` or `false` |
| `path` | `Pathname` |
| `url` | `String`, serialized as the WHATWG URL Standard writes it |
| `socket-address`, `string`, `enumeration` | `String` |
| `bytes` | the decoded bytes, as a binary `String` |
| any withheld variable | `Starmap::Secret`, whose `reveal` returns the value |

## What the gem leaves out

The gem resolves nothing. It reads no bind, asks no provider and generates
no value, so a `secret`, `generated` or `provided` variable must reach
`ENV` from the program that starts the application, such as
`starmap run`. The gem has no helper that compares the file with another
tool's configuration.

## Keeping the reads and the file matched

`Starmap::Testing.assert_declared!` scans `app/`, `lib/` and `config/` for
literal reads of `ENV`, such as `ENV["NAME"]` and `ENV.fetch("NAME")`, and
raises one error that names each read of a name `starmap.toml` does not
declare. A test calls it once:

```ruby
require "starmap/testing"

class EnvironmentTest < Minitest::Test
  def test_every_variable_the_code_reads_is_declared
    Starmap::Testing.assert_declared!(root: Rails.root.to_s)
  end
end
```

`allow:` lists names that the code may read without a declaration.
`Starmap::Testing.undeclared_reads` returns the reads in place of raising.
The scan sees only a name written as a string literal, so it does not
report a read whose name is built at run time.

## Conformance

The gem's tests run every case in [conformance/](../conformance), which
the Rust crate's tests also run. For each case the gem gives the same
problem codes, canonical form and digest, or the same mode, values and
refusals. Run them with `bundle exec rake` in this directory, which also
runs RuboCop.

The URL serialization follows the WHATWG URL Standard for absolute URLs:
the scheme in lower case, and for `http`, `https`, `ws`, `wss`, `ftp` and
`file` the host in lower case, the default port left out and an empty path
written as `/`. It does not apply the standard's rules for percent-encoding
or for internationalized host names, so such a URL may reach `Starmap.env`
spelled differently from the text `starmap run` gives the child. A pattern
is compiled by Ruby's regular expressions, which accept the syntax that the
Rust `regex` crate and RE2 share.
