# XSLT 3.0 conformance

Per element.

## xsl:accept

Not planned — part of the xsl:package system (see xsl:package).

## xsl:accumulator

We don't go for streaming support.

## xsl:accumulator-rule

We don't go for streaming support.

## xsl:analyze-string

We have regexml now, so should be able to implement.

## xsl:apply-imports

TODO: import subsystem

## xsl:apply-templates

Not yet:

- Mode support

- Variables in patterns

- Rooted patterns

- Certain axes

- Fallback templates

## xsl:assert

TODO

## xsl:attribute

Cannot add after normal child.

Not yet:

- type

- validation

## xsl:attribute-set

TODO

## xsl:break

TODO: xsl:iterate

## xsl:call-template

TODO: function subsystem

## xsl:catch

TODO

## xsl:character-map

TODO

## xsl:choose

Done

## xsl:comment

Done

## xsl:context-item

TODO

## xsl:copy

Not yet:

- copy-namespaces, inherit-namespaces, use-attribute-set, type, validation

## xsl:copy-of

Not yet:

- copy-accumulators, copy-namespaces, type, validation

## xsl:decimal-format

TOD: awaiting xee-format

## xsl:document

TODO: nodes

## xsl:element

Not yet:

- inherit-namespaces

- use-attribute sets

- type

- validation

## xsl:evaluate

TODO

## xsl:expose

Not planned — part of the xsl:package system (see xsl:package).

## xsl:fallback

TODO

## xsl:for-each

Todo:

- xsl:sort support

## xsl:for-each-group

TODO

## xsl:fork

TODO

## xsl:function

TODO: function subsystem

## xsl:global-context-item

TODO

## xsl:if

Done

## xsl:import

TODO: import subsystem

## xsl:import-schema

TODO: schema support

## xsl:include

TODO: imoprt subsystem

## xsl:iterate

TODO

## xsl:key

TODO

## xsl:map

TODO

## xsl:map-entry

TODO

## xsl:matching-substring

TODO: regexml

## xsl:merge

TODO

## xsl:merge-action

TODO

## xsl:merge-key

TODO

## xsl:merge-source

TODO

## xsl:message

TODO

## xsl:mode

TODO

## xsl:namespace

Not yet:

- validation that namespace cannot be added if a normal child has been added already.

## xsl:namespace-alias

TODO

## xsl:next-iteration

TODO: xsl:iterate

## xsl:next-match

TODO: template rule subsystem, import system

## xsl:non-matching-substring

Have regexml now.

## xsl:number

TODO: xee-format

## xsl:on-completion

TODO: xsl:iterate

## xsl:on-empty

TODO

## xsl:on-non-empty

TODO

## xsl:otherwise

Done

## xsl:output

TODO

## xsl:output

TODO: output method subsystem

## xsl:output-character

TODO

## xsl:override

Not planned — part of the xsl:package system (see xsl:package).

## xsl:package

Not planned. xsl:package (and the related xsl:use-package, xsl:override,
xsl:accept, xsl:expose) define a modular packaging system for XSLT 3.0.
However:

- Even Saxon gates xsl:package behind its paid Enterprise Edition (EE) license.
  The free Home Edition (HE) and open-source Community Edition do not support it.
- Real-world adoption is minimal — virtually no stylesheets in the wild use it.
- Implementation cost is enormous: package versioning, component visibility
  (public/private/final/abstract), cross-package linking, and the full
  accept/expose/override machinery.
- The W3C vendor test suite contains ~308 package-related tests across 5 test
  sets (expose, override, package, package-version, use-package), all excluded
  via wildcard filter.

xsl:import and xsl:include (the traditional module system) are supported.

## xsl:param

TODO: function subsystem

## xsl:perform-sort

TODO

## xsl:preserve-space

TODO

## xsl:processing-instruction

Done

## xsl:result-document

TODO

## xsl:sequence

Done

## xsl:sort

TODO

## xsl:source-document

TODO

## xsl:strip-space

TODO

## xsl:stylesheet

Not yet: all of the attibutes

## xsl:template

Including priority.

Not yet:

- match: variable support, rooted paths, certain axes

- name

- mode

- as

- visibility

## xsl:text

Not yet:

- depecrated disable-output-escaping

## xsl:transform

See xsl:stylesheet

## xsl:try

TODO

## xsl:use-package

Not planned — part of the xsl:package system (see xsl:package).

## xsl:value-of

Done except:

- disable-output-escaping (backwards compatibility)

## xsl:variable

Not yet:

- compile-time variables used as global variables

- global variables

- attributes: as, visbiility

## xsl:when

Done

## xsl:where-populated

TODO

## xsl:with-param

Todo: function subsystem
