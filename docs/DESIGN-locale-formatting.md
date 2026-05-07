# Locale-Aware Date/Time Formatting

## Overview

Xee uses the `pure_rust_locales` crate (v0.8.2) to provide localized month
names, day-of-week names, and AM/PM markers for `format-date()`,
`format-time()`, and `format-dateTime()`. The crate bundles 336 glibc locale
definitions with zero external dependencies.

## Language Resolution

The `language` parameter on the formatting functions accepts a BCP 47 language
tag (e.g. `"de"`, `"fr"`, `"sv"`). Resolution proceeds as follows:

1. Try an exact match against `pure_rust_locales::Locale` variants.
2. Fall back to a default locale for the base language (e.g. `"de"` →
   `de_DE`, `"fr"` → `fr_FR`). Approximately 90 language codes are mapped.
3. If no locale can be resolved, the output is prefixed with
   `[Language: en]` and English is used, per spec §9.8.4.8.

## AM/PM in 24-Hour Cultures

**Decision:** When a resolved locale has no AM/PM strings (e.g. German,
French), the `[P]` component produces an **empty string**.

**Rationale:** Silently injecting English "AM"/"PM" into German output
violates the principle of least surprise and removes the stylesheet author's
ability to detect and adapt. Returning an empty string allows authors to
probe the locale:

```xslt
<xsl:variable name="has-ampm"
    select="format-time(xs:time('15:00:00'), '[P]', $lang, (), ()) != ''"/>
<xsl:value-of select="
    if ($has-ampm)
    then format-time($t, '[h]:[m01] [PN]', $lang, (), ())
    else format-time($t, '[H01]:[m01]', $lang, (), ())
"/>
```

**Spec basis:** §9.8.4.7 states that the `[P]` component output is
"entirely implementation-defined."

**Divergence from Saxon:** Saxon (PE/EE with ICU) uses CLDR data, which
provides "AM"/"PM" strings for all locales including German. Our approach
differs by respecting the glibc convention that 24-hour cultures have no
AM/PM concept. Both approaches are spec-compliant.

## Day-of-Week Indexing

The `pure_rust_locales` DAY array follows the glibc/POSIX convention:
Sunday-first (index 0 = Sunday). Chrono uses Monday-first (0 = Monday).
Conversion: `(day_from_monday + 1) % 7`.

## Case Conventions

| Picture | Internal format   | Example (English) |
|---------|-------------------|-------------------|
| `[Mn]`  | NameLower         | `january`         |
| `[MN]`  | NameUpper         | `JANUARY`         |
| `[MNn]` | NameTitle         | `January`         |
| `[Pn]`  | NameLower         | `am`              |
| `[PN]`  | NameUpper         | `AM`              |
| `[PNn]` | NameTitle         | `Am`              |

The `[F]` (day of week) component defaults to NameTitle; the `[P]` component
defaults to NameLower; the `[M]` (month) component defaults to NameLower.
