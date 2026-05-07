# Ideas and Plans

Here are some ideas and plans for Xee, some fanciful, some more concrete:

## Concrete

* Complete XPath implementation - this involves implementing missing XPath
  functions. See `conformance/fn-todo.md` for a list.

* XSLT implementation (except for streaming). A lot of basics are there, but an awful
  lot remains to be done. See `conformance/xslt.md` for a list.

* Locale-aware collation for `xsl:sort @lang` — currently silently ignored,
  falls back to default Unicode codepoint collation. Needs ICU or similar
  locale-sensitive collation support.

* Localized `fn:format-date`/`fn:format-dateTime`/`fn:format-time` — the
  `$language`, `$calendar`, and `$place` parameters are currently accepted but
  ignored. Month names, day-of-week names, and AM/PM markers are English-only.

* Complete PHP bindings for XPath - datatype support, the basics to get
  information from a Xot node (non-structural, just attributes, text, etc),
  documentation.

* Externally pluggable library functions written in Rust.

* Externally pluggable library functions written in PHP.

## Challenging

* Higher level interpreter bytecodes for optimization of common operations. Using
  a hashmap for common operations.

* XSLT streaming.

* Integrate the Xust XML Schema implementation. Support for user-defined schema types.

* Expand XPath to XQuery

* Other standards such as XProc

## Very fancyful

* Type inference

* Compiler using Cranelift. AOT means type inference is required. A JIT may get
  away without it?
