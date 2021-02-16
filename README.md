# constant_sandbox

Constant Sandbox is a cli tool for ruby codebases used to enforce boundaries and modularize Rails applications. Inspired by [packwerk](https://github.com/Shopify/packwerk) but without the rails 6 requirement.

Constant Sandbox can be used to:

* Combine groups of files into packages
* Define package-level constant visibility (i.e. have publicly accessible constants)
* Enforce privacy (inbound) and dependency (outbound) boundaries between packages
* Help existing codebases to become more modular without obstructing development

## Prerequisites

No dependencies are required for your ruby codebase. The [parser library used](https://github.com/lib-ruby-parser/lib-ruby-parser) support ruby 3.0.

## Installation

At this time the only path to install this is using rust's cargo install. I plan on publishing precompiled versions and adding homebrew recipes soon.

## Usage

The primary sub-command to use this tool is `verify`. This looks for `box.yml` files in your codebase that it treats as entry points for packages. These `box.yml` allow you to define what constants this folder exports and imports to the rest of your codebase.

To create your first constant sandbox the `init` command can take a folder and generate values for imports and exports based on your current usage.

```
constant_sandbox init lib/rubrowser/parser
```

You can now verify the box that was created by typing:

```
constant_sandbox verify
```

The last command available is `inspect`. This command evaluates your ruby codebase and outputs to stdout all of the connections that exist to the provided folder. Outputing a box configuration that would cover your current usage. This is useful for learning more about the cohesion of your codebase.

Example output:
```
For more information try --help
../constant_sandbox/target/release/constant_sandbox inspect lib/rubrowser/parser
non exported reference Rubrowser::Parser::Factory found in lib/rubrowser/data.rb on line 26
non imported reference Parser::Builders::Default found in lib/rubrowser/parser/file/builder.rb on line 8
non imported reference Parser::SyntaxError found in lib/rubrowser/parser/file.rb on line 26
non imported reference Parser::Source::Buffer found in lib/rubrowser/parser/file.rb on line 33
non imported reference Encoding::UTF_8 found in lib/rubrowser/parser/file.rb on line 34
non imported reference Parser::CurrentRuby found in lib/rubrowser/parser/file.rb on line 41
non imported reference Parser::AST::Node found in lib/rubrowser/parser/file.rb on line 145
---
imports:
  - "Encoding::UTF_8"
  - "Parser::AST::Node"
  - "Parser::Builders::Default"
  - "Parser::CurrentRuby"
  - "Parser::Source::Buffer"
  - "Parser::SyntaxError"
exports:
  - "Rubrowser::Parser::Factory"

```