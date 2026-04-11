use std::fmt::Write;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use xee_interpreter::{
  declaration::OnMultipleMatch,
    context::{StaticContext, StaticContextBuilder},
    error,
    sequence::Sequence,
    xml::Documents,
};
use xee_name::{Namespaces, FN_NAMESPACE};
use xee_xslt_ast::parse_transform as parse_xslt_transform;
use xee_xslt_compiler::{evaluate, evaluate_with_stylesheet_path, parse, parse_with_base_dir};
use xot::Xot;

fn xml(xot: &Xot, sequence: Sequence) -> String {
    let mut f = String::new();

    for item in sequence.iter() {
        f.write_str(&xot.to_string(item.to_node().unwrap()).unwrap())
            .unwrap();
    }
    f
}

fn evaluate_with_stylesheet_base(
    xot: &mut Xot,
    xml: &str,
    xslt: &str,
    stylesheet_path: &std::path::Path,
) -> error::SpannedResult<Sequence> {
    let stylesheet_uri = format!("file://{}", stylesheet_path.display()).replace(' ', "%20");
    let mut static_context_builder = StaticContextBuilder::default();
    static_context_builder.static_base_uri(Some(stylesheet_uri.try_into().unwrap()));
    let static_context = static_context_builder.build();
    let program = parse_with_base_dir(
        static_context,
        xslt,
        stylesheet_path.parent().map(|parent| parent.to_path_buf()),
    )
    .unwrap();

    let root = xot.parse(xml).unwrap();
    let mut documents = Documents::new();
    let handle = documents.add_root(None, root).unwrap();
    let root = documents.get_node_by_handle(handle).unwrap();
    let mut dynamic_context_builder = program.dynamic_context_builder();
    dynamic_context_builder.context_node(root);
    dynamic_context_builder.documents(documents);
    let context = dynamic_context_builder.build();
    let runnable = program.runnable(&context);
    runnable.many(xot)
}

  fn evaluate_named_template_with_stylesheet_base(
    xot: &mut Xot,
    xml: &str,
    xslt: &str,
    stylesheet_path: &std::path::Path,
    template_name: &str,
  ) -> error::SpannedResult<Sequence> {
    let stylesheet_uri = format!("file://{}", stylesheet_path.display()).replace(' ', "%20");
    let mut static_context_builder = StaticContextBuilder::default();
    static_context_builder.static_base_uri(Some(stylesheet_uri.try_into().unwrap()));
    let static_context = static_context_builder.build();
    let program = parse_with_base_dir(
      static_context,
      xslt,
      stylesheet_path.parent().map(|parent| parent.to_path_buf()),
    )
    .unwrap();

    let root = xot.parse(xml).unwrap();
    let mut documents = Documents::new();
    let handle = documents.add_root(None, root).unwrap();
    let root = documents.get_node_by_handle(handle).unwrap();
    let mut dynamic_context_builder = program.dynamic_context_builder();
    dynamic_context_builder.context_node(root);
    dynamic_context_builder.documents(documents);
    let context = dynamic_context_builder.build();
    let runnable = program.runnable(&context);
    runnable.named_template(template_name, xot)
  }

  fn evaluate_with_processor_xslt_version(
    xot: &mut Xot,
    xml: &str,
    xslt: &str,
    processor_xslt_version: u8,
  ) -> error::SpannedResult<Sequence> {
    let mut static_context_builder = StaticContextBuilder::default();
    static_context_builder.processor_xslt_version(Some(processor_xslt_version));
    let static_context = static_context_builder.build();
    let program = parse(static_context, xslt).unwrap();

    let root = xot.parse(xml).unwrap();
    let mut documents = Documents::new();
    let handle = documents.add_root(None, root).unwrap();
    let root = documents.get_node_by_handle(handle).unwrap();
    let mut dynamic_context_builder = program.dynamic_context_builder();
    dynamic_context_builder.context_node(root);
    dynamic_context_builder.documents(documents);
    let context = dynamic_context_builder.build();
    let runnable = program.runnable(&context);
    runnable.many(xot)
  }

  fn evaluate_with_processor_versions(
    xot: &mut Xot,
    xml: &str,
    xslt: &str,
    processor_xslt_version: u8,
    processor_xpath_version: u8,
  ) -> error::SpannedResult<Sequence> {
    let mut static_context_builder = StaticContextBuilder::default();
    static_context_builder.processor_xslt_version(Some(processor_xslt_version));
    static_context_builder.processor_xpath_version(Some(processor_xpath_version));
    let static_context = static_context_builder.build();
    let program = parse(static_context, xslt).unwrap();

    let root = xot.parse(xml).unwrap();
    let mut documents = Documents::new();
    let handle = documents.add_root(None, root).unwrap();
    let root = documents.get_node_by_handle(handle).unwrap();
    let mut dynamic_context_builder = program.dynamic_context_builder();
    dynamic_context_builder.context_node(root);
    dynamic_context_builder.documents(documents);
    let context = dynamic_context_builder.build();
    let runnable = program.runnable(&context);
    runnable.many(xot)
  }

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir =
        std::env::temp_dir().join(format!("xee-{}-{}-{}", prefix, std::process::id(), unique));
    fs::create_dir_all(&temp_dir).unwrap();
    temp_dir
}

  fn evaluate_with_stylesheet_base_and_on_multiple_match(
    xot: &mut Xot,
    xml: &str,
    xslt: &str,
    stylesheet_path: &std::path::Path,
    on_multiple_match: OnMultipleMatch,
  ) -> error::SpannedResult<Sequence> {
    let stylesheet_uri = format!("file://{}", stylesheet_path.display()).replace(' ', "%20");
    let mut static_context_builder = StaticContextBuilder::default();
    static_context_builder.static_base_uri(Some(stylesheet_uri.try_into().unwrap()));
    let static_context = static_context_builder.build();
    let program = parse_with_base_dir(
      static_context,
      xslt,
      stylesheet_path.parent().map(|parent| parent.to_path_buf()),
    )
    .unwrap();

    let root = xot.parse(xml).unwrap();
    let mut documents = Documents::new();
    let handle = documents.add_root(None, root).unwrap();
    let root = documents.get_node_by_handle(handle).unwrap();
    let mut dynamic_context_builder = program.dynamic_context_builder();
    dynamic_context_builder.context_node(root);
    dynamic_context_builder.documents(documents);
    dynamic_context_builder.on_multiple_match(on_multiple_match);
    let context = dynamic_context_builder.build();
    let runnable = program.runnable(&context);
    runnable.many(xot)
  }

#[test]
fn test_transform() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/"><a/></xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<a/>");
}

#[test]
fn test_transform_nested() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/"><a><b/><b/></a></xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<a><b/><b/></a>");
}

#[test]
fn test_transform_text_node() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/"><a>foo</a></xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<a>foo</a>");
}

#[test]
fn test_transform_nested_apply_templates() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><foo/><bar/></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:apply-templates select="doc/*" /></o>
  </xsl:template>
  <xsl:template match="foo">
    <f/>
  </xsl:template>
  <xsl:template match="bar">
    <b/>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o><f/><b/></o>");
}

#[test]
fn test_match_node_pattern_does_not_capture_initial_document_node() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc>\n  <child1>This is the child number 1.</child1>\n</doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc">
    <out>
      <xsl:apply-templates select="node()" mode="mode1"/>
    </out>
  </xsl:template>

  <xsl:template match="node()" mode="mode1">
    <xsl:value-of select="."/>
  </xsl:template>

  <xsl:template match="node()">
    This test failed to execute properly.
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out>\n  This is the child number 1.\n</out>"
    );
}


    #[test]
    fn test_match_pattern_predicate_accepts_parent_axis_path_expression() {
        let mut xot = Xot::new();
        let output = evaluate(
            &mut xot,
            "<root><sup><a>x</a></sup><a>y</a></root>",
            r#"
    <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
      <xsl:template match="/">
        <out><xsl:apply-templates/></out>
      </xsl:template>

      <xsl:template match="text()[parent::a/parent::sup]">
        <hit/>
      </xsl:template>

      <xsl:template match="text()"/>
    </xsl:stylesheet>"#,
        )
        .unwrap();

        assert_eq!(xml(&xot, output), "<out><hit/></out>");
    }
#[test]
fn test_apply_templates_sort_with_param() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc>
  <a-set s="217" g="1st"><a>35</a><a>44</a><a>12</a><a>98</a><a>28</a></a-set>
  <a-set s="531" g="2nd"><a>62</a><a>440</a><a>29</a></a-set>
  <a-set s="172" g="3rd"><a>16</a><a>45</a><a>78</a><a>33</a></a-set>
</doc>"#,
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="doc">
    <out>
      <xsl:apply-templates select="a-set">
        <xsl:sort select="@s" data-type="number" order="ascending"/>
        <xsl:with-param name="total" select="sum(a-set/a)"/>
      </xsl:apply-templates>
    </out>
  </xsl:template>

  <xsl:template match="a-set">
    <xsl:param name="total"/>
    <list from="{@g}" proportion="{concat(sum(a), '/', $total)}">
      <xsl:for-each select="a">
        <xsl:value-of select="."/>
        <xsl:text>,</xsl:text>
      </xsl:for-each>
    </list>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out><list from=\"3rd\" proportion=\"172/920\">16,45,78,33,</list><list from=\"1st\" proportion=\"217/920\">35,44,12,98,28,</list><list from=\"2nd\" proportion=\"531/920\">62,440,29,</list></out>"
    );
}

#[test]
fn test_apply_templates_missing_mode_uses_builtin_text_only_copy() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc><a test="a attribute">a-text</a></doc>"#,
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="/">
    <out>
      <xsl:apply-templates select="doc/a" mode="z"/>
    </out>
  </xsl:template>

  <xsl:template match="text()" mode="a">
    <xsl:text>mode-a:</xsl:text>
    <xsl:value-of select="."/>
  </xsl:template>

  <xsl:template match="text()">
    <xsl:text>no-mode:</xsl:text>
    <xsl:value-of select="."/>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

  assert_eq!(xml(&xot, output), "<out>a-text</out>");
}

#[test]
fn test_named_template_with_match_is_callable() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><x test=\"why\">content</x><y>why</y></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="/">
    <out>
      <xsl:apply-templates select="doc" mode="a"/>
    </out>
  </xsl:template>

  <xsl:template match="doc" mode="a">
    <xsl:text>Found doc...</xsl:text>
    <xsl:call-template name="scan"/>
  </xsl:template>

  <xsl:template name="scan" match="*" mode="a">
    <xsl:text>Scanned </xsl:text>
    <xsl:value-of select="name(.)"/>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>Found doc...Scanned doc</out>");
}

#[test]
fn test_apply_templates_with_param_as_validates_value_type() {
    let mut xot = Xot::new();
  let error = evaluate(
        &mut xot,
        "<doc><item/></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"
                xmlns:xs="http://www.w3.org/2001/XMLSchema"
                exclude-result-prefixes="xs">
  <xsl:template match="doc">
    <out>
      <xsl:apply-templates select="item">
        <xsl:with-param name="value" select="'43'" as="xs:integer"/>
      </xsl:apply-templates>
    </out>
  </xsl:template>

  <xsl:template match="item">
    <xsl:param name="value"/>
    <xsl:value-of select="$value"/>
    <xsl:value-of select="$value instance of xs:integer"/>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap_err();

    assert_eq!(error.error, error::Error::XTTE0570);
}

#[test]
fn test_stylesheet_namespace_available_to_xs_qname() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"
                xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:my="http://myexamplefunc.org"
                exclude-result-prefixes="xs my">
  <xsl:template match="doc">
    <out>
      <xsl:value-of select="xs:QName('my:local') instance of xs:QName"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>true</out>");
}

#[test]
fn test_builtin_xml_namespace_available_without_declaration() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc xml:id="x"/>"#,
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:value-of select="/doc/@xml:id"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>x</out>");
}

#[test]
fn test_builtin_xs_namespace_available_without_declaration() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="doc">
    <out>
      <xsl:value-of select="'abc' instance of xs:string"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>true</out>");
}

#[test]
fn test_xsl_text_treats_curly_braces_as_literal_text() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:text> {</xsl:text>
      <xsl:text>}</xsl:text>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out> {}</out>");
}

#[test]
fn test_literal_result_attribute_value_template_allows_trailing_whitespace_before_closing_curly()
{
  let mut xot = Xot::new();
  let output = evaluate(
    &mut xot,
    "<doc/>",
    r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
  <out class="footnote-number{
        if (true())
        then ' table-footnote'
        else ()
        }"/>
  </xsl:template>
</xsl:stylesheet>"#,
  )
  .unwrap();

  assert_eq!(xml(&xot, output), "<out class=\"footnote-number table-footnote\"/>");
}

#[test]
fn test_template_param_default_as_converts_runtime_value_for_tunnel_param() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><item1>0</item1><item-list/></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"
                xmlns:xs="http://www.w3.org/2001/XMLSchema"
                exclude-result-prefixes="xs">
  <xsl:template match="/">
    <xsl:param name="par1" select="//item1" as="xs:double"/>
    <out>
      <xsl:apply-templates select="doc/item-list">
        <xsl:with-param name="par1" select="$par1" tunnel="yes"/>
      </xsl:apply-templates>
    </out>
  </xsl:template>

  <xsl:template match="item-list">
    <xsl:param name="par1" tunnel="yes"/>
    <par1>
      <xsl:value-of select="$par1"/>
      <xsl:value-of select="$par1 instance of xs:double"/>
    </par1>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out><par1>0true</par1></out>");
}

#[test]
fn test_whitespace_padded_required_attribute_values() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="doc">
    <out>
      <xsl:call-template name="foo">
        <xsl:with-param name="par1" select="'required'"/>
        <xsl:with-param name="par2" select="'notRequired'"/>
      </xsl:call-template>
    </out>
  </xsl:template>

  <xsl:template name="foo">
    <xsl:param name="par1" required=" true "/>
    <xsl:param name="par2" required=" 0 "/>
    <xsl:if test="$par1 = 'required'">
      <xsl:text>Required parameter;</xsl:text>
    </xsl:if>
    <xsl:if test="$par2 = 'notRequired'">
      <xsl:text>Not required parameter</xsl:text>
    </xsl:if>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out>Required parameter;Not required parameter</out>"
    );
}

#[test]
fn test_next_match_with_param_falls_back_to_builtin_rule() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><data><inner><in><last>abc</last></in></inner></data></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc">
    <out>
      <xsl:next-match>
        <xsl:with-param name="par1" select="'hola'"/>
      </xsl:next-match>
    </out>
  </xsl:template>

  <xsl:template match="data">
    <xsl:variable name="par1" select="'defaultValue'"/>
    <xsl:value-of select="$par1"/>
  </xsl:template>

  <xsl:template match="text()"/>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>defaultValue</out>");
}

#[test]
fn test_next_match_import_precedence_beats_priority() {
    let temp_dir = unique_temp_dir("next-match-006");
    fs::write(
        temp_dir.join("next-match-006.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:import href="next-match-006a.xsl" />

  <xsl:template match="node()">
    <a>
      <xsl:next-match />
    </a>
  </xsl:template>

  <xsl:template match="doc">
    <b>
      <xsl:next-match />
    </b>
  </xsl:template>

  <xsl:template match="*[foo]">
    <c>
      <xsl:next-match />
    </c>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("next-match-006a.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:import href="next-match-006b.xsl" />

  <xsl:template match="node()">
    <aa>
      <xsl:next-match />
    </aa>
  </xsl:template>

  <xsl:template match="doc">
    <ab>
      <xsl:next-match />
    </ab>
  </xsl:template>

  <xsl:template match="*[foo]">
    <ac>
      <xsl:next-match />
    </ac>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("next-match-006b.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="node()">
    <ba>
      <xsl:next-match />
    </ba>
  </xsl:template>

  <xsl:template match="doc">
    <bb>
      <xsl:next-match />
    </bb>
  </xsl:template>

  <xsl:template match="*[foo]">
    <bc>
      <xsl:next-match />
    </bc>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let mut xot = Xot::new();
    let stylesheet_path = temp_dir.join("next-match-006.xsl");
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc><foo/></doc>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<c><b><a><ac><ab><aa><bc><bb><ba><a><aa><ba/></aa></a></ba></bb></bc></aa></ab></ac></a></b></c>"
    );

    fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn test_next_match_inside_named_template_uses_calling_template_rule() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><a><b test='c'>1</b><b test='c'>2</b><b test='c'>3</b></a></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:strip-space elements="*"/>

  <xsl:template match="/">
    <out>
      <xsl:apply-templates/>
    </out>
  </xsl:template>

  <xsl:template match="b" priority="1">
    <x>
      <xsl:apply-templates/>
    </x>
  </xsl:template>

  <xsl:template match="b[@test='c']" priority="2">
    <y>
      <xsl:call-template name="test"/>
    </y>
  </xsl:template>

  <xsl:template name="test">
    <z>
      <xsl:next-match/>
    </z>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out><y><z><x>1</x></z></y><y><z><x>2</x></z></y><y><z><x>3</x></z></y></out>"
    );
}

#[test]
fn test_next_match_allows_same_stylesheet_to_be_imported_and_included() {
    let temp_dir = unique_temp_dir("next-match-017");
    fs::write(
        temp_dir.join("next-match-017.xsl"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:import href="impwparam8.xsl"/>
  <xsl:include href="impwparam8.xsl"/>

  <xsl:template match="doc">
    <out>
      <xsl:text>
</xsl:text>
      <xsl:apply-templates select="*">
        <xsl:with-param name="p1" select="'top'"/>
      </xsl:apply-templates>
      <xsl:text>
</xsl:text>
    </out>
  </xsl:template>

  <xsl:template match="tag">
    <xsl:param name="p1" select="'fallback'"/>
    <main-t>
      <xsl:value-of select="$p1"/>
    </main-t>
    <xsl:text>
</xsl:text>
    <div>
      <xsl:next-match>
        <xsl:with-param name="p1" select="'primary template'"/>
      </xsl:next-match>
    </div>
  </xsl:template>

  <xsl:template match="bag">
    <xsl:text>
</xsl:text>
    <bag/>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("impwparam8.xsl"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="tag" priority="2">
    <xsl:param name="p1" select="'default'"/>
    <imp1-t><xsl:value-of select="$p1"/></imp1-t>
    <xsl:text>
</xsl:text>
    <xsl:next-match>
      <xsl:with-param name="p1" select="'included template'"/>
    </xsl:next-match>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let mut xot = Xot::new();
    let stylesheet_path = temp_dir.join("next-match-017.xsl");
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc><tag>Example of apply-imports</tag><bag>Example of apply-templates</bag></doc>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
      "<out>\n<imp1-t>top</imp1-t>\n<main-t>included template</main-t>\n<div><imp1-t>primary template</imp1-t>\nExample of apply-imports</div>\n<bag/>\n</out>"
    );

    fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn test_next_match_in_attribute_set_on_literal_element_uses_imported_template() {
    let temp_dir = unique_temp_dir("next-match-012");
    fs::write(
        temp_dir.join("next-match-012.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:import href="next-match-012a.xsl"/>

  <xsl:template match="doc">
    <a xsl:use-attribute-sets="myAttrib">hello</a>
  </xsl:template>

  <xsl:template match="*">
    <b>next matched</b>
  </xsl:template>

  <xsl:attribute-set name="myAttrib">
    <xsl:attribute name="a1">5</xsl:attribute>
    <xsl:attribute name="a2">
      <xsl:next-match/>
    </xsl:attribute>
  </xsl:attribute-set>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("next-match-012a.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc">
    <b>imported attribute</b>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let mut xot = Xot::new();
    let stylesheet_path = temp_dir.join("next-match-012.xsl");
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc foo=\"bar\"/>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<a a1=\"5\" a2=\"next matched\">hello</a>");

    fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn test_result_document_collects_secondary_output() {
    let mut xot = Xot::new();
  let stylesheet_path = std::env::temp_dir().join("xee-result-document-collects-secondary-output.xsl");
  let static_context = StaticContextBuilder::default()
    .static_base_uri(Some(
      format!("file://{}", stylesheet_path.display())
        .replace(' ', "%20")
        .try_into()
        .unwrap(),
    ))
    .build();
    let program = parse(
        static_context,
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc">
    <out>
      <log>Before redirect</log>
      <xsl:result-document href="multresult1.out">
        <xsl:copy-of select="foo"/>
      </xsl:result-document>
      <log>After redirect</log>
    </out>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    let root = xot.parse("<doc><foo place=\"secondary\">Hello</foo></doc>").unwrap();
    let mut documents = Documents::new();
    let handle = documents.add_root(None, root).unwrap();
    let root = documents.get_node_by_handle(handle).unwrap();
    let mut dynamic_context_builder = program.dynamic_context_builder();
    dynamic_context_builder.context_node(root);
    dynamic_context_builder.documents(documents);
    let context = dynamic_context_builder.build();
    let runnable = program.runnable(&context);
    let output = runnable.many(&mut xot).unwrap();

    assert_eq!(xml(&xot, output), "<out><log>Before redirect</log><log>After redirect</log></out>");

    let secondary = context.secondary_result_document("multresult1.out").unwrap();
    assert_eq!(xml(&xot, secondary), "<foo place=\"secondary\">Hello</foo>");
}

#[test]
fn test_result_document_registers_secondary_document_uri() {
    let mut xot = Xot::new();
    let stylesheet_path = std::env::temp_dir().join(format!(
        "xee-result-document-base-uri-{}.xsl",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let program = parse_with_base_dir(
        StaticContextBuilder::default()
            .static_base_uri(Some(
                format!("file://{}", stylesheet_path.display())
                    .replace(' ', "%20")
                    .try_into()
                    .unwrap(),
            ))
            .build(),
        r#"
<xsl:transform version="3.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:template match="/">
    <xsl:result-document href="out/second.xml">
      <two>
        <k>Kilo</k>
        <l xml:base="in/third.xml">Lima</l>
      </two>
    </xsl:result-document>
  </xsl:template>
</xsl:transform>"#,
        stylesheet_path.parent().map(|parent| parent.to_path_buf()),
    )
    .unwrap();

    let root = xot.parse("<doc/>").unwrap();
    let mut documents = Documents::new();
    let handle = documents.add_root(None, root).unwrap();
    let root = documents.get_node_by_handle(handle).unwrap();
    let mut dynamic_context_builder = program.dynamic_context_builder();
    dynamic_context_builder.context_node(root);
    dynamic_context_builder.documents(documents);
    let context = dynamic_context_builder.build();
    let runnable = program.runnable(&context);
    runnable.many(&mut xot).unwrap();

    let secondary = context.secondary_result_document("out/second.xml").unwrap();
    let secondary_document = secondary.iter().next().unwrap().to_node().unwrap();

    let documents = context.documents();
    let documents = documents.borrow();
    let uri = documents
        .get_uri_by_document_node(secondary_document)
        .unwrap()
        .to_string();

    assert!(uri.ends_with("/out/second.xml"), "unexpected URI: {uri}");
}

#[test]
fn test_nested_result_document_without_href_writes_to_principal_output() {
    let mut xot = Xot::new();
    let stylesheet_path = std::env::temp_dir().join("xee-result-document-nested-principal.xsl");
    let program = parse_with_base_dir(
        StaticContextBuilder::default()
            .static_base_uri(Some(
                format!("file://{}", stylesheet_path.display())
                    .replace(' ', "%20")
                    .try_into()
                    .unwrap(),
            ))
            .build(),
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="/">
    <xsl:result-document href="secondary.xml">
      <secondary>
        <xsl:result-document>
          <primary>principal</primary>
        </xsl:result-document>
      </secondary>
    </xsl:result-document>
  </xsl:template>
</xsl:transform>"#,
        stylesheet_path.parent().map(|parent| parent.to_path_buf()),
    )
    .unwrap();

    let root = xot.parse("<doc/>").unwrap();
    let mut documents = Documents::new();
    let handle = documents.add_root(None, root).unwrap();
    let root = documents.get_node_by_handle(handle).unwrap();
    let mut dynamic_context_builder = program.dynamic_context_builder();
    dynamic_context_builder.context_node(root);
    dynamic_context_builder.documents(documents);
    let context = dynamic_context_builder.build();
    let runnable = program.runnable(&context);
    let output = runnable.many(&mut xot).unwrap();

    assert_eq!(xml(&xot, output), "<primary>principal</primary>");

    let secondary = context.secondary_result_document("secondary.xml").unwrap();
    assert_eq!(xml(&xot, secondary), "<secondary/>");
}

#[test]
fn test_recursive_attribute_set_reentry_raises_xtde0640() {
    let error = parse(
        StaticContextBuilder::default().build(),
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:attribute-set name="set1">
    <xsl:attribute name="color">
      <xsl:for-each select="*">
        <xsl:variable name="x">
          <e xsl:use-attribute-sets="set1"/>
        </xsl:variable>
        <xsl:value-of select="string-join($x//@*, '|')"/>
      </xsl:for-each>
    </xsl:attribute>
    <xsl:attribute name="texture">matt</xsl:attribute>
  </xsl:attribute-set>

  <xsl:template match="/">
    <out>
      <test1 xsl:use-attribute-sets="set1"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap_err();

    assert_eq!(error.error, error::Error::XTDE0640);
}

#[test]
fn test_next_match_on_atomic_values_uses_builtin_text_copy_fallback() {
    let mut xot = Xot::new();
    let program = parse(
        StaticContextBuilder::default().build(),
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template name="xsl:initial-template">
    <out>
      <xsl:apply-templates select="1 to 5"/>
    </out>
  </xsl:template>

  <xsl:template match=".[. ge 5]" priority="5">E<xsl:next-match/></xsl:template>
  <xsl:template match=".[. ge 4]" priority="4">D<xsl:next-match/></xsl:template>
  <xsl:template match=".[. ge 3]" priority="3">C<xsl:next-match/></xsl:template>
  <xsl:template match=".[. ge 2]" priority="2">B<xsl:next-match/></xsl:template>
  <xsl:template match=".[. ge 1]" priority="1">A<xsl:next-match/></xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    let dynamic_context_builder = program.dynamic_context_builder();
    let context = dynamic_context_builder.build();
    let runnable = program.runnable(&context);
    let output = runnable.many(&mut xot).unwrap();

    assert_eq!(xml(&xot, output), "<out>A1BA2CBA3DCBA4EDCBA5</out>");
}

#[test]
fn test_apply_imports_on_atomic_values_respects_import_chain() {
    let temp_dir = unique_temp_dir("apply-imports-001");
    fs::write(
        temp_dir.join("apply-imports-001.xsl"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:apply-templates select="1 to 5"/>
    </out>
  </xsl:template>

  <xsl:template match=".[. ge 5]" priority="5">A<xsl:apply-imports/></xsl:template>
  <xsl:template match=".[. ge 3]" priority="3">B<xsl:apply-imports/></xsl:template>

  <xsl:import href="apply-imports-001a.xsl"/>
  <xsl:import href="apply-imports-001b.xsl"/>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("apply-imports-001a.xsl"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:apply-templates select="1 to 5"/>
    </out>
  </xsl:template>

  <xsl:template match=".[. ge 5]" priority="5">X<xsl:apply-imports/></xsl:template>
  <xsl:template match=".[. ge 3]" priority="3">Y<xsl:apply-imports/></xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("apply-imports-001b.xsl"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:apply-templates select="1 to 5"/>
    </out>
  </xsl:template>

  <xsl:template match=".[. ge 5]" priority="5">P<xsl:apply-imports/></xsl:template>
  <xsl:template match=".[. ge 3]" priority="3">Q<xsl:apply-imports/></xsl:template>
  <xsl:template match=".[. ge 1]" priority="1">R<xsl:apply-imports/></xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let mut xot = Xot::new();
    let stylesheet_path = temp_dir.join("apply-imports-001.xsl");
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc/>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>R1R2BQ3BQ4AP5</out>");

    fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn test_xsl_element_applies_attribute_sets() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="/">
    <out>
      <xsl:element name="test" use-attribute-sets="set1 set2"/>
    </out>
  </xsl:template>

  <xsl:attribute-set name="set2">
    <xsl:attribute name="text-decoration">underline</xsl:attribute>
  </xsl:attribute-set>

  <xsl:attribute-set name="set1">
    <xsl:attribute name="color">black</xsl:attribute>
  </xsl:attribute-set>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out><test color=\"black\" text-decoration=\"underline\"/></out>");
}

#[test]
fn test_xsl_copy_preserves_namespaces() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<root xmlns="http://example.com"><item>test</item></root>"#,
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="@*|node()">
    <xsl:copy>
      <xsl:apply-templates select="@*|node()"/>
    </xsl:copy>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        r#"<root xmlns="http://example.com"><item>test</item></root>"#
    );
}

#[test]
fn test_xsl_copy_preserves_prefixed_namespaces() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<ex:root xmlns:ex="http://example.com"><ex:item>test</ex:item></ex:root>"#,
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="@*|node()">
    <xsl:copy>
      <xsl:apply-templates select="@*|node()"/>
    </xsl:copy>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        r#"<ex:root xmlns:ex="http://example.com"><ex:item>test</ex:item></ex:root>"#
    );
}

#[test]
fn test_xsl_copy_applies_attribute_sets() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="/doc">
    <out>
      <xsl:copy use-attribute-sets="set1 set2"/>
    </out>
  </xsl:template>

  <xsl:attribute-set name="set2">
    <xsl:attribute name="text-decoration">underline</xsl:attribute>
  </xsl:attribute-set>

  <xsl:attribute-set name="set1">
    <xsl:attribute name="color">black</xsl:attribute>
  </xsl:attribute-set>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out><doc color=\"black\" text-decoration=\"underline\"/></out>");
}

#[test]
fn test_attribute_set_only_sees_global_variables() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:variable name="foo" select="'correct'"/>

  <xsl:template match="/">
    <xsl:variable name="foo" select="'incorrect'"/>
    <out xsl:use-attribute-sets="attrs"/>
  </xsl:template>

  <xsl:attribute-set name="attrs">
    <xsl:attribute name="test">
      <xsl:value-of select="$foo"/>
    </xsl:attribute>
  </xsl:attribute-set>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out test=\"correct\"/>");
}

#[test]
fn test_missing_attribute_set_reference_is_xtse0710() {
    let static_context = StaticContextBuilder::default().build();
    let error = parse(
        static_context,
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:attribute-set name="attributeSet1">
    <xsl:attribute name="attr1"/>
  </xsl:attribute-set>
  <xsl:attribute-set name="attributeSet2" use-attribute-sets="attributeSet">
    <xsl:attribute name="attr1"/>
  </xsl:attribute-set>
</xsl:stylesheet>"#,
    )
    .unwrap_err();

    assert_eq!(error.error, error::Error::XTSE0710);
}

#[test]
fn test_multiple_matching_templates_can_raise_xtre0540() {
    let temp_dir = unique_temp_dir("multiple-match-error");
    let stylesheet_path = temp_dir.join("multiple-match-error.xsl");
    fs::write(
        &stylesheet_path,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="/">
    <out><xsl:apply-templates select="doc/a"/></out>
  </xsl:template>

  <xsl:template match="a">first</xsl:template>
  <xsl:template match="a">second</xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let mut xot = Xot::new();
    let output = evaluate_with_stylesheet_base_and_on_multiple_match(
        &mut xot,
        "<doc><a/></doc>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
        OnMultipleMatch::UseLast,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<out>second</out>");

    let mut xot = Xot::new();
    let error = evaluate_with_stylesheet_base_and_on_multiple_match(
        &mut xot,
        "<doc><a/></doc>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
        OnMultipleMatch::Fail,
    )
    .unwrap_err();
    assert_eq!(error.error, error::Error::XTRE0540);

    fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn test_multiple_matching_templates_across_includes_can_raise_xtre0540() {
    let temp_dir = unique_temp_dir("multiple-match-imports");
    fs::write(
        temp_dir.join("main.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="/">
    <out><xsl:apply-templates select="doc/title" /></out>
  </xsl:template>

  <xsl:include href="c.xsl"/>

  <xsl:template match="title">MAIN</xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("c.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:import href="e.xsl"/>
  <xsl:template match="title">C</xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("e.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="title">E</xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let stylesheet_path = temp_dir.join("main.xsl");
    let stylesheet = fs::read_to_string(&stylesheet_path).unwrap();

    let mut xot = Xot::new();
    let output = evaluate_with_stylesheet_base_and_on_multiple_match(
        &mut xot,
        "<doc><title>T</title></doc>",
        &stylesheet,
        &stylesheet_path,
        OnMultipleMatch::UseLast,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<out>MAIN</out>");

    let mut xot = Xot::new();
    let error = evaluate_with_stylesheet_base_and_on_multiple_match(
        &mut xot,
        "<doc><title>T</title></doc>",
        &stylesheet,
        &stylesheet_path,
        OnMultipleMatch::Fail,
    )
    .unwrap_err();
    assert_eq!(error.error, error::Error::XTRE0540);

    fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn test_next_match_in_absent_context_named_template_raises_xtde0560() {
    let mut xot = Xot::new();
    let error = evaluate(
        &mut xot,
        "<doc><a><b test='c'>1</b></a></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:strip-space elements="*"/>

  <xsl:template match="/">
    <out>
      <xsl:apply-templates/>
    </out>
  </xsl:template>

  <xsl:template match="b" priority="1">
    <x>
      <xsl:apply-templates/>
    </x>
  </xsl:template>

  <xsl:template match="b[@test='c']" priority="2">
    <y>
      <xsl:call-template name="test"/>
    </y>
  </xsl:template>

  <xsl:template name="test">
    <xsl:context-item use="absent"/>
    <z>
      <xsl:next-match/>
    </z>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap_err();

    assert_eq!(error.error, error::Error::XTDE0560);
}

#[test]
fn test_next_match_inside_copy_select_raises_xtde0560() {
    let mut xot = Xot::new();
    let error = evaluate(
        &mut xot,
        "<doc><a><b test='c'>1</b></a></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:strip-space elements="*"/>

  <xsl:template match="/">
    <out>
      <xsl:apply-templates/>
    </out>
  </xsl:template>

  <xsl:template match="b" priority="1">
    <x>
      <xsl:apply-templates/>
    </x>
  </xsl:template>

  <xsl:template match="b[@test='c']" priority="2">
    <y>
      <xsl:copy select="..">
        <xsl:next-match/>
      </xsl:copy>
    </y>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap_err();

    assert_eq!(error.error, error::Error::XTDE0560);
}

#[test]
fn test_next_match_inside_for_each_raises_xtde0560() {
    let mut xot = Xot::new();
    let error = evaluate(
        &mut xot,
        "<doc><a><b test='c'>1</b></a></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:strip-space elements="*"/>

  <xsl:template match="/">
    <out>
      <xsl:apply-templates/>
    </out>
  </xsl:template>

  <xsl:template match="b" priority="1">
    <x>
      <xsl:apply-templates/>
    </x>
  </xsl:template>

  <xsl:template match="b[@test='c']" priority="2">
    <y>
      <xsl:for-each select="..">
        <xsl:next-match/>
      </xsl:for-each>
    </y>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap_err();

    assert_eq!(error.error, error::Error::XTDE0560);
}

#[test]
fn test_next_match_uses_deep_skip_builtin_rule_without_skipping_document_root() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><tag>Example of apply-imports</tag><bag>Example of apply-templates</bag></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:mode on-no-match="deep-skip"/>

  <xsl:template match="doc">
    <out>
      <xsl:apply-templates select="*"/>
    </out>
  </xsl:template>

  <xsl:template match="tag">
    <tag>
      <xsl:next-match/>
    </tag>
  </xsl:template>

  <xsl:template match="bag">
    <bag>
      <xsl:next-match/>
    </bag>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out><tag/><bag/></out>");
}

#[test]
fn test_repeated_local_variable_reference_in_union_expression() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <xsl:variable name="analysis">
      <result><span/></result>
      <unique><span/></unique>
    </xsl:variable>
    <out>
      <xsl:value-of select="count($analysis/result/span | $analysis/unique/span)"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>2</out>");
}

#[test]
fn test_global_variable_can_reference_later_global_param() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:variable name="tata" select="$toto"/>
  <xsl:param name="toto" select="'titi'"/>

  <xsl:template match="/">
    <xsl:param name="toto" select="'templ'"/>
    <out>
      <xsl:value-of select="$toto"/>
      <xsl:text>, </xsl:text>
      <xsl:value-of select="$tata"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>templ, titi</out>");
}

#[test]
fn test_global_variable_can_be_built_with_xsl_map() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:map="http://www.w3.org/2005/xpath-functions/map"
    xmlns:xs="http://www.w3.org/2001/XMLSchema"
    version="3.0">
  <xsl:variable name="params" as="map(xs:QName, item()*)">
    <xsl:map>
      <xsl:map-entry key="QName('', 'debug')" select="'keep'"/>
    </xsl:map>
  </xsl:variable>

  <xsl:template match="/">
    <out>
      <xsl:value-of select="map:get($params, QName('', 'debug'))"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let rendered = xml(&xot, output);
    assert!(rendered.starts_with("<out"));
    assert!(rendered.contains(">keep</out>"));
}

#[test]
fn test_call_template_unknown_param_is_ignored_in_xslt_1_mode() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
  <xsl:param name="test" select="'global'"/>

  <xsl:template match="/">
    <out>
      <xsl:call-template name="temtest">
        <xsl:with-param name="test" select="'local'"/>
      </xsl:call-template>
    </out>
  </xsl:template>

  <xsl:template name="temtest">
    <xsl:choose>
      <xsl:when test="$test = 'global'">It is global!</xsl:when>
      <xsl:otherwise>Not global!!!</xsl:otherwise>
    </xsl:choose>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>It is global!</out>");
}

#[test]
fn test_union_match_without_explicit_priority_registers_multiple_rules() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><a>1</a><d>2</d></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="doc">
    <out><xsl:apply-templates/></out>
  </xsl:template>

  <xsl:template match="a|d">
    <xsl:value-of select="name(.)"/>
    <xsl:text>=</xsl:text>
    <xsl:value-of select="."/>
    <xsl:text>;</xsl:text>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>a=1;d=2;</out>");
}

#[test]
fn test_builtin_text_template_rule_constructs_text_nodes() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><a>begin</a><b>middle</b><c>end</c></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="doc">
    <out>
      <xsl:apply-templates select="a/text(), b/text(), c/text()"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>beginmiddleend</out>");
}

#[test]
fn test_xslt_user_defined_function_call_in_select() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:test="urn:test"
    xmlns:xs="http://www.w3.org/2001/XMLSchema"
    exclude-result-prefixes="test xs"
    version="3.0">
  <xsl:function name="test:double" as="xs:integer">
    <xsl:param name="value" as="xs:integer"/>
    <xsl:sequence select="$value * 2"/>
  </xsl:function>

  <xsl:template match="/">
    <out><xsl:value-of select="test:double(21)"/></out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>42</out>");
}

#[test]
fn test_for_each_sort_uses_xslt_sort_order() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:for-each select="('id-0', 'id-2', 'id-10')">
        <xsl:sort select="."/>
        <xsl:value-of select="."/>
        <xsl:text>|</xsl:text>
      </xsl:for-each>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>id-0|id-10|id-2|</out>");
}

#[test]
fn test_for_each_numeric_sort_preserves_order_for_nan_keys() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><t>First</t><t>p2</t><t>1.0.9</t><t>00k</t><t>1.u</t><t>1-m</t><t>0.5s</t><t>Last</t></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc">
    <out>
      <xsl:for-each select="t">
        <xsl:sort data-type="number"/>
        <xsl:value-of select="."/>
        <xsl:text>|</xsl:text>
      </xsl:for-each>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out>First|p2|1.0.9|00k|1.u|1-m|0.5s|Last|</out>"
    );
}

#[test]
fn test_for_each_descending_numeric_sort_places_nan_last() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
               version="2.0">
  <xsl:template match="/">
    <out>
      <xsl:for-each select="(xs:float(12.5), xs:integer(1), xs:float('NaN'), xs:double('NaN'), xs:float(0.009), xs:double(-0.05), xs:string(-0.00))">
        <xsl:sort select="." data-type="number" order="descending"/>
        <xsl:value-of select="."/>
        <xsl:text>|</xsl:text>
      </xsl:for-each>
    </out>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out>12.5|1|0.009|0|-0.05|NaN|NaN|</out>"
    );
}

#[test]
fn test_mode_all_template_matches_initial_unnamed_mode() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r##"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/" mode="#all">
    <out>ok</out>
  </xsl:template>
</xsl:stylesheet>"##,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>ok</out>");
}

#[test]
fn test_default_mode_attribute_overrides_nested_apply_templates_mode() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc><a test="attribute"/></doc>"#,
        r##"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0" default-mode="a">
  <xsl:template match="/" mode="#all">
    <out xsl:default-mode="b">
      <xsl:apply-templates select="doc/a" default-mode="a"/>
    </out>
  </xsl:template>

  <xsl:template match="a" default-mode="a">
    <xsl:text>element-mode-a:</xsl:text>
    <xsl:apply-templates select="@test"/>
  </xsl:template>

  <xsl:template match="a" default-mode="b">
    <xsl:text>element-mode-b:</xsl:text>
    <xsl:apply-templates select="@test"/>
  </xsl:template>

  <xsl:template match="@*" mode="a">
    <xsl:value-of select="."/>
  </xsl:template>

  <xsl:template match="@*" mode="b">
    <xsl:text>attribute-mode-b</xsl:text>
  </xsl:template>
</xsl:stylesheet>"##,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>element-mode-a:attribute</out>");
}

#[test]
fn test_default_mode_attribute_on_apply_templates_selects_nested_mode() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc><a test="attribute"/></doc>"#,
        r##"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0" default-mode="a">
  <xsl:template match="/" mode="#all">
    <out xsl:default-mode="b">
      <xsl:apply-templates select="doc/a" default-mode="a"/>
    </out>
  </xsl:template>

  <xsl:template match="a" default-mode="a">
    <xsl:text>element-mode-a:</xsl:text>
    <xsl:apply-templates select="@test" default-mode="b"/>
  </xsl:template>

  <xsl:template match="a" default-mode="b">
    <xsl:text>element-mode-b:</xsl:text>
    <xsl:apply-templates select="@test"/>
  </xsl:template>

  <xsl:template match="@*" mode="a">
    <xsl:value-of select="."/>
  </xsl:template>

  <xsl:template match="@*" mode="b">
    <xsl:text>attribute-mode-b</xsl:text>
  </xsl:template>
</xsl:stylesheet>"##,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out>element-mode-a:attribute-mode-b</out>"
    );
}

#[test]
fn test_apply_templates_current_uses_current_mode_inside_template_rule() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><a><b/></a></doc>",
        r##"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:apply-templates select="doc/a" mode="m"/>
    </out>
  </xsl:template>

  <xsl:template match="a" mode="m">
    <xsl:apply-templates select="b" mode="#current"/>
  </xsl:template>

  <xsl:template match="b" mode="m">
    <m/>
  </xsl:template>

  <xsl:template match="b">
    <u/>
  </xsl:template>
</xsl:stylesheet>"##,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out><m/></out>");
}

#[test]
fn test_rooted_variable_pattern_matches_in_xslt_30() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><foo><baz att1=\"wrong\"/></foo></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:variable name="x" select="/doc"/>

  <xsl:template match="doc">
    <out><xsl:apply-templates/></out>
  </xsl:template>

  <xsl:template match="$x//baz">
    <hit><xsl:value-of select="name(.)"/></hit>
  </xsl:template>

  <xsl:template match="text()"/>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out><hit>baz</hit></out>");
}

#[test]
fn test_rooted_variable_pattern_is_rejected_in_xslt_20() {
    let static_context = StaticContextBuilder::default().build();
    let error = parse(
        static_context,
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:variable name="x" select="/doc"/>
  <xsl:template match="$x"/>
</xsl:stylesheet>"#,
    )
    .unwrap_err();

    assert_eq!(error.value(), error::Error::XTSE0340);
}

#[test]
fn test_rooted_doc_pattern_matches_in_xslt_30() {
    let temp_dir = unique_temp_dir("rooted-doc-pattern");
    fs::write(temp_dir.join("match02.xml"), "<doc><foo><a/></foo></doc>").unwrap();

    let mut xot = Xot::new();
    let stylesheet_path = temp_dir.join("rooted-doc-pattern.xsl");
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:strip-space elements="*"/>

  <xsl:template match="/">
    <out>
      <xsl:apply-templates select="doc('match02.xml')"/>
    </out>
  </xsl:template>

  <xsl:template match="doc('match02.xml')">
    <first>
      <xsl:apply-templates/>
    </first>
  </xsl:template>

  <xsl:template match="doc('match02.xml')/doc/foo/a">
    <two>
      <xsl:copy-of select="."/>
    </two>
  </xsl:template>
</xsl:stylesheet>"#,
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out><first><two><a/></two></first></out>"
    );

    fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn test_rooted_doc_pattern_without_steps_matches_document_node_in_xslt_30() {
    let temp_dir = unique_temp_dir("rooted-doc-node-pattern");
    fs::write(temp_dir.join("match1002.xml"), "<foo/>").unwrap();

    let mut xot = Xot::new();
    let stylesheet_path = temp_dir.join("rooted-doc-node-pattern.xsl");
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:mode on-no-match="deep-skip"/>

  <xsl:template match="/">
    <out>
      <xsl:apply-templates select="doc('match1002.xml')"/>
    </out>
  </xsl:template>

  <xsl:template match="doc('match1002.xml')">
    <ok/>
  </xsl:template>
</xsl:stylesheet>"#,
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out><ok/></out>");

    fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn test_rooted_doc_pattern_is_rejected_in_xslt_20() {
    let static_context = StaticContextBuilder::default().build();
    let error = parse(
        static_context,
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc('match02.xml')"/>
</xsl:stylesheet>"#,
    )
    .unwrap_err();

    assert_eq!(error.value(), error::Error::XTSE0340);
}

#[test]
fn test_apply_templates_current_falls_back_to_unnamed_mode_outside_template_rule() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><a><b>text</b></a></doc>",
        r##"
<xsl:stylesheet xmlns:f="http://example.com/test"
                xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:apply-templates select="doc/a" mode="m"/>
    </out>
  </xsl:template>

  <xsl:template match="a" mode="m">
    <xsl:sequence select="f:apply(.)"/>
  </xsl:template>

  <xsl:template match="b" mode="m">
    <m/>
  </xsl:template>

  <xsl:template match="b">
    <u/>
  </xsl:template>

  <xsl:function name="f:apply">
    <xsl:param name="node" as="element(a)"/>
    <xsl:apply-templates select="$node/b" mode="#current"/>
  </xsl:function>
</xsl:stylesheet>"##,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out><u/></out>");
}

#[test]
fn test_mode_attributes_accept_whitespace_padded_values() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><a/></doc>",
        r##"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                version=" 3.0 ">
  <xsl:mode name=" Q{}s " on-no-match=" shallow-skip " warning-on-no-match=" no "/>

  <xsl:template match="/">
    <out>
      <xsl:apply-templates select="doc" mode=" Q{}s "/>
    </out>
  </xsl:template>

  <xsl:template match="doc" mode=" s ">
    <xsl:apply-templates select="a" mode=" #current "/>
  </xsl:template>

  <xsl:template match="a" mode="Q{}s">
    <ok/>
  </xsl:template>
</xsl:stylesheet>"##,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out><ok/></out>");
}

#[test]
fn test_mode_typed_no_is_accepted() {
    parse(
        StaticContext::default(),
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:mode name="s" on-no-match="shallow-copy" typed=" no "/>
</xsl:stylesheet>"#,
    )
    .unwrap();
}

#[test]
fn test_mode_on_no_match_shallow_copy_preserves_attributes() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc a=\"1\"><child>text</child></doc>",
        r##"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:mode name="s" on-no-match="shallow-copy"/>

  <xsl:template match="/">
    <out>
      <xsl:apply-templates select="doc" mode="s"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"##,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out><doc a=\"1\"><child>text</child></doc></out>"
    );
}

#[test]
fn test_mode_typed_yes_rejects_untyped_nodes() {
    let mut xot = Xot::new();
    let err = evaluate(
        &mut xot,
        "<doc/>",
        r##"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:mode name="s" typed="yes"/>

  <xsl:template match="/">
    <xsl:apply-templates select="doc" mode="s"/>
  </xsl:template>
</xsl:stylesheet>"##,
    )
    .unwrap_err();

    assert_eq!(err.value(), error::Error::XTTE3100);
}

#[test]
fn test_document_instruction_creates_document_node_variable() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:variable name="var1" as="document-node()">
    <xsl:document>
      <item>hello</item>
    </xsl:document>
  </xsl:variable>

  <xsl:template match="/doc">
    <out>
      <xsl:value-of select="$var1 instance of document-node()"/>
    </out>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>true</out>");
}

#[test]
fn test_document_instruction_satisfies_item_return_type() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:my="http://uri.test" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="/">
    <out>
      <xsl:call-template name="a5"/>
    </out>
  </xsl:template>

  <xsl:template name="a5" as="item()">
    <xsl:document>
      <my:item>1</my:item>
    </xsl:document>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out><my:item xmlns:my=\"http://uri.test\">1</my:item></out>"
    );
}

#[test]
fn test_sequence_document_loads_relative_to_stylesheet_base_uri() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!(
        "xee-sequence-1202-{}-{}",
        std::process::id(),
        unique
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(temp_dir.join("sequence-1202a.xml"), "<doc/>").unwrap();

    let mut xot = Xot::new();
    let stylesheet_path = temp_dir.join("sequence-1202.xsl");
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc">
    <out>
      <xsl:text>(((</xsl:text>
      <xsl:sequence select="document('sequence-1202a.xml')"/>
      <xsl:text>)))</xsl:text>
    </out>
  </xsl:template>
</xsl:transform>"#,
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>(((<doc/>)))</out>");

    fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn test_for_each_group_group_by_keeps_first_item_per_key() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><record key='a' n='1'/><record key='a' n='2'/><record key='b' n='3'/></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:for-each-group select="doc/record" group-by="string(@key)">
        <xsl:value-of select="@n"/>
        <xsl:text>|</xsl:text>
      </xsl:for-each-group>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>1|3|</out>");
}

#[test]
fn test_transform_value_of_select() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:value-of select="1 to 4" /></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o>1 2 3 4</o>");
}

#[test]
fn test_transform_value_of_select_separator() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:value-of select="1 to 4" separator="|" /></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o>1|2|3|4</o>");
}

#[test]
fn test_value_of_with_sequence_constructor() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:value-of>Hello</xsl:value-of></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o>Hello</o>");
}

#[test]
fn test_transform_local_variable() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3" >
  <xsl:template match="/">
    <xsl:variable name="foo" select="'FOO'"/>
    <o><xsl:value-of select="$foo"/></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<o>FOO</o>");
}

#[test]
fn test_transform_local_variable_shadow() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <xsl:variable name="foo" select="'FOO'"/>
    <xsl:variable name="foo" select="'BAR'"/>
    <o><xsl:value-of select="$foo"/></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<o>BAR</o>");
}

#[test]
fn test_duplicate_local_template_params_are_rejected() {
    let namespaces = Namespaces::new(
        Namespaces::default_namespaces(),
        "".to_string(),
        FN_NAMESPACE.to_string(),
    );
    let static_context = StaticContext::from_namespaces(namespaces);
    let output = parse(
        static_context,
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <xsl:apply-templates select="doc">
      <xsl:with-param name="mod" select="3"/>
    </xsl:apply-templates>
  </xsl:template>

  <xsl:template match="doc">
    <xsl:param name="mod" select="1"/>
    <xsl:param name="mod" select="2"/>
    <out result="{$mod}"/>
  </xsl:template>
</xsl:transform>"#,
    );

    assert!(matches!(
        output,
        error::SpannedResult::Err(error::SpannedError {
            error: error::Error::XTSE0580,
            span: _
        })
    ));
}

#[test]
fn test_missing_name_attribute_reports_xtse0010() {
    let output = parse(
        StaticContext::default(),
        r#"
  <xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
    <xsl:variable select="'ABC'"/>
  </xsl:transform>"#,
    );

    assert!(matches!(
        output,
        error::SpannedResult::Err(error::SpannedError {
            error: error::Error::XTSE0010,
            span: _
        })
    ));
}

#[test]
fn test_disallowed_with_param_attribute_reports_xtse0090() {
    let output = parse(
        StaticContext::default(),
        r#"
  <xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
    <xsl:template match="/">
    <xsl:call-template name="temp1">
      <xsl:with-param name="par" select="'xyz'" required="yes"/>
    </xsl:call-template>
    </xsl:template>

    <xsl:template name="temp1">
    <xsl:param name="par"/>
    </xsl:template>
  </xsl:transform>"#,
    );

    assert!(matches!(
        output,
        error::SpannedResult::Err(error::SpannedError {
            error: error::Error::XTSE0090,
            span: _
        })
    ));
}

#[test]
fn test_sort_lang_attribute_is_parsed_before_compile_support_check() {
    let error = parse(
        StaticContext::default(),
        r#"
  <xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
    <xsl:template match="/">
      <xsl:for-each select="(3, 1, 2)">
        <xsl:sort select="." lang="en"/>
        <xsl:sequence select="."/>
      </xsl:for-each>
    </xsl:template>
  </xsl:transform>"#,
    )
    .unwrap_err();

    assert!(matches!(
        error.value(),
        error::Error::Unsupported(ref reason) if reason.contains("xsl:sort lang is not supported yet")
    ));
}

#[test]
fn test_invalid_required_attribute_value_reports_xtse0020() {
    let output = parse(
        StaticContext::default(),
        r#"
  <xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
    <xsl:template name="foo">
    <xsl:param name="par1" required="TRUE"/>
    </xsl:template>
  </xsl:transform>"#,
    );

    assert!(matches!(
        output,
        error::SpannedResult::Err(error::SpannedError {
            error: error::Error::XTSE0020,
            span: _
        })
    ));
}

#[test]
fn test_pattern_predicate_position_ignores_whitespace_text_nodes() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<servlet-mapping>
   <servlet-name>MyServlet</servlet-name>
   <url-pattern>/servlet/MyServlet/*</url-pattern>
</servlet-mapping>"#,
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0" expand-text="yes">
  <xsl:template match="/">
    <xsl:apply-templates select="servlet-mapping/url-pattern"/>
  </xsl:template>
  <xsl:template match="url-pattern[position()=last()]">
    <out>{.}</out>
  </xsl:template>
  <xsl:template match="url-pattern"><wrong/></xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>/servlet/MyServlet/*</out>");
}

#[test]
fn test_imported_predicate_match_patterns_restore_state_after_swallowed_error() {
    let temp_dir = unique_temp_dir("import-1201-predicate-patterns");
    let stylesheet_path = temp_dir.join("main.xsl");
    fs::write(
        &stylesheet_path,
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:import href="imp.xsl"/>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("imp.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc">
    <out>
      <xsl:apply-templates/>
    </out>
  </xsl:template>
  <xsl:template match="*[.=117]">
    <g/>
  </xsl:template>
  <xsl:template
    match="*[(not(.=117) and ((position() &gt; 2) and (position() &lt; 6))) and ((@century='yes') or (@foo='nope'))]">
    <c/>
  </xsl:template>
  <xsl:template match="text()"/>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let mut xot = Xot::new();
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc>\n<aaa foo=\"nope\">1</aaa>\n<bbb>2</bbb>\n<qqe>117</qqe>\n<ddd century=\"yes\">4</ddd>\n<eee>5</eee>\n</doc>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out><g/><c/></out>");
}

#[test]
fn test_format_number_uses_default_decimal_format_symbols() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:decimal-format decimal-separator="|" grouping-separator="." minus-sign="~"/>
  <xsl:template match="doc">
    <out><xsl:value-of select="format-number(-12345.6, '#.##0|00')"/></out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>~12.345|60</out>");
}

#[test]
fn test_format_number_invalid_picture_uses_processor_error_code_by_default() {
    let mut xot = Xot::new();
    let error = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc">
    <out>
      <xsl:value-of select="format-number(931.4857, '000.##0')"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap_err();

    assert_eq!(error.value(), error::Error::FODF1310);
}

#[test]
fn test_format_number_invalid_picture_uses_xtde1310_in_xslt20_processor_mode() {
    let mut xot = Xot::new();
    let error = evaluate_with_processor_xslt_version(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc">
    <out>
      <xsl:value-of select="format-number(931.4857, '000.##0')"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
        2,
    )
    .unwrap_err();

    assert_eq!(error.value(), error::Error::XTDE1310);
}

#[test]
fn test_use_when_supports_function_available_and_element_available() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:fn="http://www.w3.org/2005/xpath-functions" version="2.0">
  <xsl:template match="doc">
    <out>
      <xsl:element name="E1" use-when="element-available('xsl:value-of')">
        <xsl:text>Element defined</xsl:text>
      </xsl:element>
      <E2 xsl:use-when="function-available('fn:doc')">
        <xsl:text>Function defined</xsl:text>
      </E2>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(
      xml(&xot, output),
      "<out xmlns:fn=\"http://www.w3.org/2005/xpath-functions\"><E1>Element defined</E1><E2>Function defined</E2></out>"
    );
}

#[test]
fn test_use_when_generate_id_is_disabled_in_xslt20() {
    let mut static_context_builder = StaticContextBuilder::default();
    static_context_builder.processor_xslt_version(Some(2));
    let error = parse(
        static_context_builder.build(),
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc">
    <out>
      <flag xsl:use-when="generate-id(()) = ''">static</flag>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap_err();

    assert_eq!(error.value(), error::Error::XPST0017);
}

#[test]
fn test_use_when_generate_id_is_enabled_in_xslt30() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="doc">
    <out>
      <xsl:value-of select="function-available('generate-id')"/>
      <flag xsl:use-when="function-available('generate-id')">static</flag>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>true<flag>static</flag></out>");
}

#[test]
fn test_use_when_generate_id_is_enabled_in_xslt20_stylesheet_with_xslt30_processor_mode() {
    let mut xot = Xot::new();
    let output = evaluate_with_processor_xslt_version(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc">
    <out>
      <flag xsl:use-when="generate-id(()) = ''">static</flag>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
        3,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out><flag>static</flag></out>");
}

#[test]
fn test_use_when_instance_of_uses_xpath_default_namespace_for_types() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<root><para>p1</para><para>p2</para></root>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0" xpath-default-namespace="http://www.w3.org/2001/XMLSchema">
  <xsl:template match="*">
    <xsl:copy>
      <xsl:apply-templates/>
    </xsl:copy>
  </xsl:template>
  <xsl:template match="*[local-name()='para']" use-when="'abc' instance of string">
    <p><xsl:next-match/></p>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<root><p><para>p1</para></p><p><para>p2</para></p></root>");
}

#[test]
fn test_use_when_sees_static_variable_from_included_module() {
    let temp_dir = unique_temp_dir("use-when-include-static-vars");
    let stylesheet_path = temp_dir.join("main.xsl");
    fs::write(
        &stylesheet_path,
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:variable name="inc" select="true()" static="yes"/>
  <xsl:include href="included.xsl"/>
  <xsl:template name="main" use-when="$oink">
    <xsl:call-template name="action"/>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("included.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template name="action" use-when="$inc">
    <ok/>
  </xsl:template>
  <xsl:variable name="oink" select="true()" static="yes"/>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let mut xot = Xot::new();
    let output = evaluate_named_template_with_stylesheet_base(
        &mut xot,
        "<doc/>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
        "main",
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<ok/>");
}

#[test]
fn test_use_when_reports_xtse3450_for_inconsistent_imported_static_variable() {
    let temp_dir = unique_temp_dir("use-when-import-static-conflict");
    let stylesheet_path = temp_dir.join("main.xsl");
    fs::write(
        &stylesheet_path,
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:import href="imported.xsl"/>
  <xsl:variable name="inc" select="true()" static="yes"/>
  <xsl:template name="main" use-when="$inc">
    <xsl:call-template name="action"/>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("imported.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template name="action">
    <ok/>
  </xsl:template>
  <xsl:variable name="inc" select="false()" static="yes"/>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let mut static_context_builder = StaticContextBuilder::default();
    let stylesheet_uri = format!("file://{}", stylesheet_path.display()).replace(' ', "%20");
    static_context_builder.static_base_uri(Some(stylesheet_uri.try_into().unwrap()));
    let error = parse_with_base_dir(
        static_context_builder.build(),
        &fs::read_to_string(&stylesheet_path).unwrap(),
        stylesheet_path.parent().map(|parent| parent.to_path_buf()),
    )
    .unwrap_err();

    assert_eq!(error.value(), error::Error::XTSE3450);
}

#[test]
fn test_use_when_reports_xtse3450_for_reimported_inconsistent_static_variable() {
    let temp_dir = unique_temp_dir("use-when-reimport-static-conflict");
    let stylesheet_path = temp_dir.join("main.xsl");
    fs::write(
        &stylesheet_path,
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:import href="a.xsl"/>
  <xsl:variable name="one" static="yes" select="$flip-flop"/>
  <xsl:import href="b.xsl"/>
  <xsl:variable name="two" static="yes" select="$flip-flop"/>
  <xsl:import href="a.xsl"/>
  <xsl:variable name="three" static="yes" select="$flip-flop"/>
  <xsl:template name="main" use-when="$one and not($two) and $three">
    <ok/>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("a.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:variable name="flip-flop" select="true()" static="yes"/>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("b.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:variable name="flip-flop" select="false()" static="yes"/>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let mut static_context_builder = StaticContextBuilder::default();
    let stylesheet_uri = format!("file://{}", stylesheet_path.display()).replace(' ', "%20");
    static_context_builder.static_base_uri(Some(stylesheet_uri.try_into().unwrap()));
    let error = parse_with_base_dir(
        static_context_builder.build(),
        &fs::read_to_string(&stylesheet_path).unwrap(),
        stylesheet_path.parent().map(|parent| parent.to_path_buf()),
    )
    .unwrap_err();

    assert_eq!(error.value(), error::Error::XTSE3450);
}

#[test]
fn test_use_when_false_on_include_skips_included_stylesheet() {
    let temp_dir = unique_temp_dir("use-when-include-false");
    let stylesheet_path = temp_dir.join("main.xsl");
    fs::write(
        &stylesheet_path,
        r#"<?xml version="1.0"?>
<t:transform version="2.0" xmlns:t="http://www.w3.org/1999/XSL/Transform">
  <t:include href="include3.xsl" use-when="false()"/>
  <t:template match="doc">
    <out>
      <t:apply-templates/>
    </out>
  </t:template>
  <t:template match="a" use-when="true()">
    <print_a><t:next-match/></print_a>
  </t:template>
</t:transform>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("include3.xsl"),
        r#"<?xml version="1.0"?>
<xsl:transform version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" use-when="true()">
  <xsl:template match="b">
    <print_b><xsl:next-match/></print_b>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    let mut xot = Xot::new();
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc><elem><a>a1</a><a>a2</a><b>b1</b><b>b2</b></elem></doc>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out><print_a>a1</print_a><print_a>a2</print_a>b1b2</out>");
}

#[test]
fn test_use_when_on_stylesheet_does_not_remove_top_level_children() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><elem><a>a1</a><a>a2</a><b>b1</b><b>b2</b></elem></doc>",
        r#"
<t:transform xmlns:t="http://www.w3.org/1999/XSL/Transform" version="2.0" use-when="false()">
  <t:template match="elem">
    <out>
      <t:copy>
        <t:apply-templates/>
      </t:copy>
    </out>
  </t:template>

  <t:template match="a | b" use-when="true()">
    <print>
      <t:next-match/>
    </print>
  </t:template>
</t:transform>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out><elem><print>a1</print><print>a2</print><print>b1</print><print>b2</print></elem></out>"
    );
}

#[test]
fn test_use_when_xsl_attribute_on_lre_reports_xtse0805() {
    let error = parse(
        StaticContextBuilder::default().build(),
        r#"
<t:transform xmlns:t="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <t:template match="doc">
    <out>
      <elem t:use-when="true()" t:if="See what happens!">error</elem>
    </out>
  </t:template>
</t:transform>"#,
    )
    .unwrap_err();

    assert_eq!(error.value(), error::Error::XTSE0805);
}

#[test]
fn test_use_when_invalid_stylesheet_version_reports_xtse0110() {
    let error = parse(
        StaticContextBuilder::default().build(),
        r#"
<t:transform xmlns:t="http://www.w3.org/1999/XSL/Transform"
             version="'2.0'"
             use-when="false()">
  <t:template match="elem">
    <out>
      <t:copy>
        <t:apply-templates/>
      </t:copy>
    </out>
  </t:template>

  <t:template match="a | b" use-when="true()">
    <print>
      <t:next-match/>
    </print>
  </t:template>
</t:transform>"#,
    )
    .unwrap_err();

    assert_eq!(error.value(), error::Error::XTSE0110);
}

#[test]
fn test_use_when_false_on_included_stylesheet_root_skips_module() {
    let temp_dir = unique_temp_dir("use-when-included-root-false");
    let stylesheet_path = temp_dir.join("main.xsl");
    fs::write(
        &stylesheet_path,
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:include href="include.xsl"/>
  <xsl:template match="doc">
    <out><xsl:apply-templates/></out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("include.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0" use-when="false()">
  <xsl:template match="para">
    <p><xsl:next-match/></p>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let mut xot = Xot::new();
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc><para>p1</para><para>p2</para></doc>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>p1p2</out>");
}

#[test]
fn test_use_when_static_base_uri_uses_stylesheet_location() {
    let temp_dir = unique_temp_dir("use-when-static-base-uri");
    let stylesheet_path = temp_dir.join("main.xsl");
    fs::write(
        &stylesheet_path,
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc" use-when="contains(static-base-uri(), 'main.xsl')">
    <ok/>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let mut xot = Xot::new();
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc/>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<ok/>");
}

#[test]
fn test_use_when_static_base_uri_respects_xml_base() {
    let mut xot = Xot::new();
    let output = evaluate_named_template_with_stylesheet_base(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                 version="3.0"
                 xml:base="http://www.example.com/dir/">
  <xsl:variable name="inc"
                static="yes"
                xml:base="../sub"
                select="static-base-uri()"/>
  <xsl:template name="main" use-when="$inc eq 'http://www.example.com/sub'">
    <ok/>
  </xsl:template>
</xsl:stylesheet>"#,
        std::path::Path::new("/tmp/use-when-static-base-uri-xml-base.xsl"),
        "main",
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<ok/>");
}

#[test]
fn test_use_when_masks_unavailable_extension_element_with_extension_attributes() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><para>p1</para><para>p2</para></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                version="2.0"
                extension-element-prefixes="saxon"
                xmlns:saxon="http://not.saxon.sf.net/">
  <xsl:variable name="count-para" select="0" saxon:assignable="yes"/>

  <xsl:template match="/">
    <result>
      <xsl:apply-templates/>
      <count nr="{$count-para}"/>
    </result>
  </xsl:template>

  <xsl:template match="*">
    <xsl:copy>
      <xsl:copy-of select="@*"/>
      <xsl:apply-templates/>
    </xsl:copy>
  </xsl:template>

  <xsl:template match="para">
    <saxon:assign name="count-para"
                  select="$count-para + 1"
                  xsl:use-when="element-available('saxon:assign')"/>
    <xsl:next-match/>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(
      xml(&xot, output),
      "<result xmlns:saxon=\"http://not.saxon.sf.net/\"><doc><para>p1</para><para>p2</para></doc><count nr=\"0\"/></result>"
    );
}

#[test]
fn test_use_when_doc_available_is_false_in_xslt20() {
    let temp_dir = unique_temp_dir("use-when-doc-available");
    let stylesheet_path = temp_dir.join("main.xsl");
    fs::write(
        &stylesheet_path,
        r#"<?xml version="1.0"?>
<t:transform xmlns:t="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <t:variable name="v" as="element()*">
    <a><b/></a>
  </t:variable>

  <t:template match="/">
    <out>
      <t:call-template name="temp"/>
      <t:value-of select="doc-available($v)" use-when="doc-available('')"/>
    </out>
  </t:template>

  <t:template name="temp">
    <t:value-of select="doc-available('')"/>
  </t:template>
</t:transform>"#,
    )
    .unwrap();

    let mut xot = Xot::new();
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc/>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>true</out>");
}

#[test]
fn test_use_when_fallback_is_ignored_for_supported_instruction() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    exclude-result-prefixes="xs"
    version="2.0">
  <xsl:template match="/">
    <result>
      <xsl:value-of>
        <xsl:text>Success</xsl:text>
        <xsl:fallback use-when="function-available('string')">Nothing to fallback on</xsl:fallback>
      </xsl:value-of>
    </result>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<result>Success</result>");
}

#[test]
fn test_use_when_inside_fallback_lre_does_not_emit_content() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    exclude-result-prefixes="xs"
    version="2.0">
  <xsl:template match="/">
    <result>
      <xsl:value-of>
        <xsl:text>Success</xsl:text>
        <xsl:fallback>
          <row xsl:use-when="function-available('string')">Nothing to fallback on</row>
        </xsl:fallback>
      </xsl:value-of>
    </result>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<result>Success</result>");
}

#[test]
fn test_use_when_in_no_namespace_on_lre_does_not_hide_variable_binding_body_error() {
    let error = parse(
        StaticContextBuilder::default().build(),
        r#"
<xslt:transform xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:xslt="http://www.w3.org/1999/XSL/Transform"
                exclude-result-prefixes="xs"
                version="3.0">
  <xslt:variable name="x" as="xs:string" select="'correct'" static="true">
    <not-allowed-but-disabled use-when="system-property('xslt:version') = '10.1'" />
  </xslt:variable>

  <xslt:template match="/">
    <out var="{$x}" />
  </xslt:template>
</xslt:transform>"#,
    )
    .unwrap_err();

    assert_eq!(error.value(), error::Error::XTSE0620);
}

#[test]
fn test_use_when_false_on_static_variable_body_prunes_content_before_xtse0620() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xslt:transform xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:xslt="http://www.w3.org/1999/XSL/Transform"
                exclude-result-prefixes="xs"
                version="3.0">
  <xslt:variable name="x" as="xs:string" select="'correct'" static="true">
    <not-allowed-but-disabled xslt:use-when="system-property('xslt:version') = '10.1'" />
  </xslt:variable>

  <xslt:template match="/">
    <out var="{$x}" />
  </xslt:template>
</xslt:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out var=\"correct\"/>");
}

#[test]
fn test_use_when_default_namespace_on_stylesheet_does_not_make_lre_use_when_special() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<transform xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns="http://www.w3.org/1999/XSL/Transform"
    xmlns:out="urn:out"
    exclude-result-prefixes="xs out"
    version="3.0">
  <template match="/">
    <result xmlns="">
      <out:row select="'raises no error'" use-when="wrong///xpath" xmlns="http://www.w3.org/1999/XSL/Transform" />
    </result>
  </template>
</transform>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
      "<result><out:row xmlns:out=\"urn:out\" select=\"&apos;raises no error&apos;\" use-when=\"wrong///xpath\"/></result>"
    );
}

#[test]
fn test_use_when_sort_without_select_can_sort_nodes() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc>
  <item>television</item>
  <item>radio</item>
  <item>VCR</item>
  <item>Mirror</item>
  <item>Bed</item>
  <item>Closet</item>
  <item>Cabinet</item>
  <item>Carpet</item>
  <item>DVD player</item>
  <item>Desk</item>
  <item>Xbox</item>
  <item>Coffee table</item>
  <item>Sofa</item>
  <item>love seat</item>
  <item>chair</item>
  <item>wine bar</item>
  <item>04</item>
  <item>002</item>
</doc>"#,
        r#"
<t:transform xmlns:t="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <t:template match="doc">
    <out>
      <descending>
        <t:for-each select="item">
          <t:sort order="descending"
                  collation="http://www.w3.org/2005/xpath-functions/collation/codepoint"/>
          <t:copy-of select="."/>
        </t:for-each>
      </descending>
    </out>
  </t:template>
</t:transform>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out><descending><item>wine bar</item><item>television</item><item>radio</item><item>love seat</item><item>chair</item><item>Xbox</item><item>VCR</item><item>Sofa</item><item>Mirror</item><item>Desk</item><item>DVD player</item><item>Coffee table</item><item>Closet</item><item>Carpet</item><item>Cabinet</item><item>Bed</item><item>04</item><item>002</item></descending></out>"
    );
}

#[test]
fn test_evaluate_with_stylesheet_path_resolves_relative_include() {
    let temp_dir = unique_temp_dir("evaluate-stylesheet-path-include");
    let stylesheet_path = temp_dir.join("main.xsl");
    fs::write(
        &stylesheet_path,
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:include href="include.xsl"/>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("include.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="/">
    <ok/>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let mut xot = Xot::new();
    let output = evaluate_with_stylesheet_path(
        &mut xot,
        "<doc/>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<ok/>");
}

#[test]
fn test_evaluate_with_stylesheet_path_reports_xtse0165_for_missing_include() {
  let temp_dir = unique_temp_dir("evaluate-stylesheet-path-missing-include");
  let stylesheet_path = temp_dir.join("main.xsl");
  fs::write(
    &stylesheet_path,
    r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:include href="missing.xsl"/>
</xsl:stylesheet>"#,
  )
  .unwrap();

  let mut xot = Xot::new();
  let error = evaluate_with_stylesheet_path(
    &mut xot,
    "<doc/>",
    &fs::read_to_string(&stylesheet_path).unwrap(),
    &stylesheet_path,
  )
  .unwrap_err();

  assert_eq!(error.value(), error::Error::XTSE0165);
}

#[test]
fn test_evaluate_with_stylesheet_path_supports_simplified_stylesheet_module() {
  let temp_dir = unique_temp_dir("simplified-stylesheet-module");
  let stylesheet_path = temp_dir.join("main.xsl");
  fs::write(
    &stylesheet_path,
    r#"<?xml version="1.0"?>
<out xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xsl:version="2.0">
  <in><xsl:value-of select="'Hi there!'"/></in>
</out>"#,
  )
  .unwrap();

  let mut xot = Xot::new();
  let output = evaluate_with_stylesheet_path(
    &mut xot,
    "<doc/>",
    &fs::read_to_string(&stylesheet_path).unwrap(),
    &stylesheet_path,
  )
  .unwrap();

  assert_eq!(xml(&xot, output), "<out><in>Hi there!</in></out>");
}

#[test]
fn test_evaluate_with_stylesheet_path_reports_xtse0150_for_missing_simplified_version() {
  let temp_dir = unique_temp_dir("simplified-stylesheet-missing-version");
  let stylesheet_path = temp_dir.join("main.xsl");
  fs::write(
    &stylesheet_path,
    r#"<?xml version="1.0"?>
<out xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <in/>
</out>"#,
  )
  .unwrap();

  let mut xot = Xot::new();
  let error = evaluate_with_stylesheet_path(
    &mut xot,
    "<doc/>",
    &fs::read_to_string(&stylesheet_path).unwrap(),
    &stylesheet_path,
  )
  .unwrap_err();

  assert_eq!(error.value(), error::Error::XTSE0150);
}

#[test]
fn test_evaluate_with_stylesheet_path_reports_xtse0180_for_self_include() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!(
        "xee-self-include-{}-{}",
        std::process::id(),
        unique
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let stylesheet = r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:include href="self-include.xsl"/>
  <xsl:template match="/">
    <out/>
  </xsl:template>
</xsl:stylesheet>"#;
    let stylesheet_path = temp_dir.join("self-include.xsl");
    fs::write(&stylesheet_path, stylesheet).unwrap();

    let mut xot = Xot::new();
    let error = evaluate_with_stylesheet_path(&mut xot, "<doc/>", stylesheet, &stylesheet_path)
        .unwrap_err();

    assert_eq!(error.value(), error::Error::XTSE0180);

    fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn test_try_catches_dynamic_error() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:try select="1 div 0">
        <xsl:catch select="'Infinity'"/>
      </xsl:try>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>Infinity</out>");
}

#[test]
fn test_try_matches_specific_error_code() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"
                xmlns:err="http://www.w3.org/2005/xqt-errors"
                exclude-result-prefixes="err">
  <xsl:template match="/">
    <out>
      <xsl:try select="1 div 0">
        <xsl:catch errors="err:FOAR9876" select="'nope'"/>
        <xsl:catch errors="err:FOAR0001" select="'Infinity'"/>
        <xsl:catch errors="*" select="'wrong-catch'"/>
      </xsl:try>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>Infinity</out>");
}

#[test]
fn test_try_does_not_apply_xpath_default_namespace_to_error_qnames() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"
                xmlns:err="http://www.w3.org/2005/xqt-errors"
                exclude-result-prefixes="err">
  <xsl:template match="/" xpath-default-namespace="http://www.w3.org/2005/xqt-errors">
    <out>
      <xsl:try select="1 div 0">
        <xsl:catch errors="FOAR0001" select="'wrong-catch'"/>
        <xsl:catch errors="*" select="'OK'"/>
      </xsl:try>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>OK</out>");
}

#[test]
fn test_try_exposes_error_variables_in_catch() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"
                xmlns:err="http://www.w3.org/2005/xqt-errors"
                exclude-result-prefixes="err">
  <xsl:template match="/">
    <xsl:try select="1 div 0">
      <xsl:catch>
        <out code="{local-name-from-QName($err:code)}"
             described="{contains(lower-case($err:description), 'zero')}"
             line="{$err:line-number}"
             column="{$err:column-number}"/>
      </xsl:catch>
    </xsl:try>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
      "<out code=\"FOAR0001\" described=\"true\" line=\"6\" column=\"22\"/>"
    );
}

#[test]
fn test_try_element_available_respects_processor_xslt_version() {
    let mut xot = Xot::new();
    let output = evaluate_with_processor_xslt_version(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="/">
    <out try="{element-available('xsl:try')}" catch="{element-available('xsl:catch')}"/>
  </xsl:template>
</xsl:stylesheet>"#,
        2,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out try=\"false\" catch=\"false\"/>");
}

#[test]
fn test_try_uses_fallback_in_xslt20_processor_forward_compatibility_mode() {
    let mut xot = Xot::new();
    let output = evaluate_with_processor_xslt_version(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:try>
        <xsl:sequence select="2+2"/>
        <xsl:catch errors="*"/>
        <xsl:fallback>
          <xsl:sequence select="2+3"/>
        </xsl:fallback>
      </xsl:try>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
        2,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>5</out>");
}

#[test]
fn test_try_uses_fallback_when_instruction_version_exceeds_processor_version() {
    let mut xot = Xot::new();
    let output = evaluate_with_processor_xslt_version(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="/">
    <out>
      <xsl:try version="3.0">
        <xsl:sequence select="2+2"/>
        <xsl:catch errors="*"/>
        <xsl:fallback>
          <xsl:sequence select="2+3"/>
        </xsl:fallback>
      </xsl:try>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
        2,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>5</out>");
}

#[test]
fn test_try_does_not_catch_global_variable_errors() {
    let mut xot = Xot::new();
    let error = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet version="3.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:param name="p" select="0"/>
  <xsl:variable name="q" select="22 div $p"/>

  <xsl:template match="/">
    <output>
      <xsl:try>
        <out><xsl:value-of select="$q"/></out>
        <xsl:catch>
          <caught/>
        </xsl:catch>
      </xsl:try>
    </output>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap_err();

    assert_eq!(error.value(), error::Error::FOAR0001);
}

  #[test]
  fn test_try_vendor_002_exposes_module_and_line_information() {
    let mut xot = Xot::new();
    let stylesheet_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("../vendor/xslt-tests/tests/insn/try/try-002.xsl");
    let xslt = fs::read_to_string(&stylesheet_path).unwrap();
    let output = evaluate_named_template_with_stylesheet_base(
      &mut xot,
      "<doc/>",
      &xslt,
      &stylesheet_path,
      "main",
    )
    .unwrap();
    let actual = xml(&xot, output);

    assert!(actual.contains("code=\"err:FOAR0001\""), "{actual}");
    assert!(actual.contains("module=\"try-002.xsl\""), "{actual}");
    assert!(actual.contains("line=\"17\""), "{actual}");
    assert!(actual.to_lowercase().contains("zero"), "{actual}");
  }

  #[test]
  fn test_try_vendor_018_exposes_module_line_and_column_information() {
    let mut xot = Xot::new();
    let stylesheet_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("../vendor/xslt-tests/tests/insn/try/try-018.xsl");
    let xslt = fs::read_to_string(&stylesheet_path).unwrap();
    let output = evaluate_named_template_with_stylesheet_base(
      &mut xot,
      "<doc/>",
      &xslt,
      &stylesheet_path,
      "main",
    )
    .unwrap();
    let actual = xml(&xot, output);

    assert!(actual.contains("local=\"FODC0002\""), "{actual}");
    assert!(actual.contains("try-018-rubbish.xml"), "{actual}");
    assert!(actual.contains("<module>file://"), "{actual}");
    assert!(actual.contains("try-018.xsl</module>"), "{actual}");
    assert!(actual.contains("<line>11</line>"), "{actual}");
  }

  #[test]
  fn test_try_vendor_031_local_variable_error_is_not_caught() {
    let mut xot = Xot::new();
    let stylesheet_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("../vendor/xslt-tests/tests/insn/try/try-031.xsl");
    let xslt = fs::read_to_string(&stylesheet_path).unwrap();
    let error = evaluate_named_template_with_stylesheet_base(
      &mut xot,
      r#"<root><n/><n/><n/></root>"#,
      &xslt,
      &stylesheet_path,
      "main",
    )
    .unwrap_err();

    assert_eq!(error.value(), error::Error::FOAR0001);
  }

  #[test]
  fn test_try_vendor_021_allows_result_document_validation_strip() {
    let mut xot = Xot::new();
    let stylesheet_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("../vendor/xslt-tests/tests/insn/try/try-021.xsl");
    let xslt = fs::read_to_string(&stylesheet_path).unwrap();
    let output = evaluate_named_template_with_stylesheet_base(
      &mut xot,
      "<doc/>",
      &xslt,
      &stylesheet_path,
      "main",
    )
    .unwrap();
    let actual = xml(&xot, output);

    assert!(actual.contains("code=\"err:XTDE1490\""), "{actual}");
    assert!(actual.contains("module=\"try-021.xsl\""), "{actual}");
  }

#[test]
fn test_try_catches_sequence_constructor_error() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"
                xmlns:err="http://www.w3.org/2005/xqt-errors"
                exclude-result-prefixes="err">
  <xsl:template match="/">
    <out>
      <xsl:try>
        <xsl:value-of select="1 div 0"/>
        <xsl:catch errors="err:FOAR0001">
          <xsl:text>Infinity</xsl:text>
        </xsl:catch>
      </xsl:try>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>Infinity</out>");
}

#[test]
fn test_try_variables_are_not_visible_in_catch() {
    let mut xot = Xot::new();
    let error = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"
                xmlns:err="http://www.w3.org/2005/xqt-errors"
                exclude-result-prefixes="err">
  <xsl:template match="/">
    <out>
      <xsl:try>
        <xsl:variable name="pi" select="3.14159"/>
        <xsl:attribute name="value" select="1 div 0"/>
        <xsl:catch errors="err:FOAR0001">
          <xsl:attribute name="value" select="$pi"/>
        </xsl:catch>
      </xsl:try>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap_err();

    assert_eq!(error.value(), error::Error::XPST0008);
}

#[test]
fn test_imported_decimal_format_merges_across_import_precedence() {
    let temp_dir = unique_temp_dir("format-number-import-precedence");
    let stylesheet_path = temp_dir.join("main.xsl");
    fs::write(
        &stylesheet_path,
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:import href="imported.xsl"/>
  <xsl:decimal-format minus-sign="~"/>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("imported.xsl"),
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:decimal-format decimal-separator="|"/>
  <xsl:template match="/">
    <out><xsl:value-of select="format-number(-10000093.7, '0|00')"/></out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let mut xot = Xot::new();
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc/>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>~10000093|70</out>");
}

#[test]
fn test_imported_named_decimal_format_is_visible_in_importing_stylesheet() {
    let temp_dir = unique_temp_dir("format-number-named-import-visibility");
    let stylesheet_path = temp_dir.join("main.xsl");
    fs::write(
        &stylesheet_path,
        r##"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
  <xsl:import href="imported.xsl"/>
  <xsl:template match="doc">
    <out>
      <xsl:value-of select="format-number('NaN','###','decimal3')"/>
      <xsl:text>, </xsl:text>
      <xsl:value-of select="format-number(-13.2,'###.0','decimal3')"/>
      <xsl:text>|</xsl:text>
      <xsl:call-template name="sub"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"##,
    )
    .unwrap();
    fs::write(
        temp_dir.join("imported.xsl"),
        r##"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
  <xsl:decimal-format name="decimal3" digit="#" NaN="not a number"/>
  <xsl:template name="sub">
    <sub>
      <xsl:value-of select="format-number('NaN','###','decimal3')"/>
      <xsl:text>, </xsl:text>
      <xsl:value-of select="format-number(-13.2,'###.0','decimal3')"/>
    </sub>
  </xsl:template>
</xsl:stylesheet>"##,
    )
    .unwrap();

    let mut xot = Xot::new();
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc/>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>not a number, -13.2|<sub>not a number, -13.2</sub></out>");
}

#[test]
fn test_imported_named_decimal_format_merges_identical_declarations() {
    let temp_dir = unique_temp_dir("format-number-named-import-merge");
    let stylesheet_path = temp_dir.join("main.xsl");
    fs::write(
        &stylesheet_path,
        r##"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
  <xsl:import href="imported.xsl"/>
  <xsl:decimal-format name="decimal3" NaN="not a number" decimal-separator="."/>
  <xsl:template match="doc">
    <out>
      <xsl:value-of select="format-number('NaN','###','decimal3')"/>
      <xsl:text>, </xsl:text>
      <xsl:value-of select="format-number(-13.2,'###.0','decimal3')"/>
      <xsl:text>|</xsl:text>
      <xsl:call-template name="sub"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"##,
    )
    .unwrap();
    fs::write(
        temp_dir.join("imported.xsl"),
        r##"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
  <xsl:decimal-format name="decimal3" digit="#" NaN="not a number"/>
  <xsl:template name="sub">
    <sub>
      <xsl:value-of select="format-number('NaN','###','decimal3')"/>
      <xsl:text>, </xsl:text>
      <xsl:value-of select="format-number(-13.2,'###.0','decimal3')"/>
    </sub>
  </xsl:template>
</xsl:stylesheet>"##,
    )
    .unwrap();

    let mut xot = Xot::new();
    let output = evaluate_with_stylesheet_base(
        &mut xot,
        "<doc/>",
        &fs::read_to_string(&stylesheet_path).unwrap(),
        &stylesheet_path,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>not a number, -13.2|<sub>not a number, -13.2</sub></out>");
}

#[test]
fn test_format_number_resolves_prefixed_decimal_format_name_in_expression_context() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:decimal-format name="a:test" decimal-separator="," grouping-separator="."
      xmlns:a="http://aaa.uri/"/>

  <xsl:template match="/">
    <o><xsl:value-of select="format-number(12.34, '0.000,00', 'b:test')" xmlns:b="http://aaa.uri/"/></o>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<o>0.012,34</o>");
}

#[test]
fn test_format_number_accepts_high_precision_decimal_literal() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc">
    <out>
      <xsl:value-of select="format-number(000123456789012345678901234567890.123456789012345678900000,
      '##0.0####################################################')"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out>123456789012345678901234567890.1234567890123456789</out>"
    );
}

#[test]
fn test_format_number_accepts_exponent_separator_in_xpath31_xslt30_processor_mode() {
    let mut xot = Xot::new();
    let output = evaluate_with_processor_versions(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:decimal-format exponent-separator="E" />
  <xsl:template match="doc">
    <out><xsl:value-of select="format-number(123.456, '0.0000E0')"/></out>
  </xsl:template>
</xsl:stylesheet>"#,
        3,
        31,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>1.2346E2</out>");
}

#[test]
fn test_format_number_rejects_exponent_separator_without_xpath31_processor_mode() {
    let mut static_context_builder = StaticContextBuilder::default();
    static_context_builder.processor_xslt_version(Some(3));
    static_context_builder.processor_xpath_version(Some(30));
    let error = parse(
        static_context_builder.build(),
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:decimal-format exponent-separator="E" />
  <xsl:template match="doc">
    <out><xsl:value-of select="format-number(123.456, '0.0000E0')"/></out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap_err();

    assert_eq!(error.value(), error::Error::XTSE0090);
}

#[test]
fn test_format_number_supports_non_ascii_zero_digit_and_literal_ascii_suffix() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:decimal-format digit="!" zero-digit="&#x0660;" />
  <xsl:template match="doc">
    <out>
      <xsl:value-of select="format-number(4030201.0506, '#!!!,!!!,&#x0660;&#x0660;&#x0660;.&#x0660;&#x0660;&#x0660;&#x0660;&#x0660;&#x0660;0')"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
      "<out>#\u{0664},\u{0660}\u{0663}\u{0660},\u{0662}\u{0660}\u{0661}.\u{0660}\u{0665}\u{0660}\u{0666}\u{0660}\u{0660}0</out>"
    );
}

#[test]
fn test_format_number_supports_leading_grouping_separator_pattern() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:decimal-format decimal-separator="&#110000;" grouping-separator="&#110001;" />
  <xsl:template match="doc">
    <out>
      <xsl:value-of select="format-number(1234567890.123456, '&#110001;000&#110000;000')"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out>1\u{1ADB1}234\u{1ADB1}567\u{1ADB1}890\u{1ADB0}123</out>"
    );
}

#[test]
fn test_format_number_supports_non_bmp_zero_digit_output() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:decimal-format zero-digit="&#x104a0;"/>
  <xsl:template match="doc">
    <out>
      <xsl:value-of select="format-number(1234567890.123456, '##########&#x104a0;.&#x104a0;#####')"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out>\u{104A1}\u{104A2}\u{104A3}\u{104A4}\u{104A5}\u{104A6}\u{104A7}\u{104A8}\u{104A9}\u{104A0}.\u{104A1}\u{104A2}\u{104A3}\u{104A4}\u{104A5}\u{104A6}</out>"
    );
}

#[test]
fn test_overloaded_xslt_function_call_inside_function_body() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<root><value1>10</value1><value2>20</value2></root>",
        r#"
<xsl:stylesheet exclude-result-prefixes="xs f"
                version="3.0"
                xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                xmlns:f="http://aimtec.cz/EDI">
  <xsl:template match="root">
    <out>
      <one><xsl:value-of select="f:format-number(value1, '000')"/></one>
      <two><xsl:value-of select="f:format-number(value2, '0000')"/></two>
    </out>
  </xsl:template>

  <xsl:function name="f:format-number" as="xs:string">
    <xsl:param name="number"/>
    <xsl:param name="format" as="xs:string"/>
    <xsl:param name="decimals"/>

    <xsl:choose>
      <xsl:when test="string($decimals) != '' and $number castable as xs:decimal">
        <xsl:sequence select="format-number($number, $format)"/>
      </xsl:when>
      <xsl:when test="$number castable as xs:decimal">
        <xsl:value-of select="format-number($number, $format)"/>
      </xsl:when>
      <xsl:otherwise>
        <xsl:sequence select="''"/>
      </xsl:otherwise>
    </xsl:choose>
  </xsl:function>

  <xsl:function name="f:format-number" as="xs:string">
    <xsl:param name="number"/>
    <xsl:param name="format" as="xs:string"/>
    <xsl:value-of select="f:format-number($number, $format, '')"/>
  </xsl:function>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out><one>010</one><two>0020</two></out>");
}

#[test]
fn test_vendor_format_number_040_stylesheet() {
  let vendor_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("../vendor/xslt-tests/tests/fn/format-number");
  let stylesheet_path = vendor_dir.join("format-number-040.xsl");
  let xslt = fs::read_to_string(&stylesheet_path).unwrap();
  let mut xot = Xot::new();
  let output = evaluate_with_stylesheet_base(&mut xot, "<doc/>", &xslt, &stylesheet_path).unwrap();

  assert_eq!(
    xml(&xot, output),
    "<out>\n<one>not a number, -13.2</one>\n<sub>not a number, -13.2</sub></out>"
  );
}

#[test]
fn test_vendor_format_number_041_stylesheet() {
  let vendor_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("../vendor/xslt-tests/tests/fn/format-number");
  let stylesheet_path = vendor_dir.join("format-number-041.xsl");
  let xslt = fs::read_to_string(&stylesheet_path).unwrap();
  let mut xot = Xot::new();
  let output = evaluate_with_stylesheet_base(&mut xot, "<doc/>", &xslt, &stylesheet_path).unwrap();

  assert_eq!(
    xml(&xot, output),
    "<out>\n<main>not a number, -13.2</main>\n<sub>not a number, -13.2</sub></out>"
  );
}

#[test]
fn test_vendor_format_number_070_stylesheet() {
  let vendor_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("../vendor/xslt-tests/tests/fn/format-number");
  let stylesheet_path = vendor_dir.join("format-number-070.xsl");
  let xslt = fs::read_to_string(&stylesheet_path).unwrap();
  let mut xot = Xot::new();
  let output = evaluate_with_stylesheet_base(
    &mut xot,
    "<root><value1>58</value1><value2>64</value2></root>",
    &xslt,
    &stylesheet_path,
  )
  .unwrap();

  assert_eq!(xml(&xot, output), "<root><format1>058</format1><format2>0000000064</format2><ver>3.0</ver></root>");
}

#[test]
fn test_vendor_format_number_x43import_parses() {
    let stylesheet_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../vendor/xslt-tests/tests/fn/format-number/x43import.xsl");
    let xslt = fs::read_to_string(&stylesheet_path).unwrap();

    parse_xslt_transform(&xslt).unwrap();
}

#[test]
fn test_static_globals_flow_across_sequential_includes() {
    let temp_dir = unique_temp_dir("static-include-scope");
    fs::create_dir_all(&temp_dir).unwrap();

    let main_path = temp_dir.join("main.xsl");
    fs::write(
        temp_dir.join("param.xsl"),
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:param name="debug" static="yes" as="xs:string" xmlns:xs="http://www.w3.org/2001/XMLSchema" select="'keep'"/>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("variable.xsl"),
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:v="urn:test:variables"
    xmlns:xs="http://www.w3.org/2001/XMLSchema"
    version="3.0">
  <xsl:variable name="v:debug" static="yes" as="xs:string*"
      select="tokenize($debug, '[,\s]+') ! normalize-space(.)"/>

  <xsl:template match="/">
    <out>
      <xsl:message use-when="'drop' = $v:debug">drop</xsl:message>
      <ok/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        &main_path,
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:include href="param.xsl"/>
  <xsl:include href="variable.xsl"/>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let xslt = fs::read_to_string(&main_path).unwrap();
    let mut xot = Xot::new();
    let output = evaluate_with_stylesheet_base(&mut xot, "<doc/>", &xslt, &main_path).unwrap();
    let rendered = xml(&xot, output);

    assert!(rendered.starts_with("<out"));
    assert!(rendered.contains("<ok/>"));

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_use_when_in_imported_module_sees_earlier_static_variable_from_including_stylesheet() {
    let temp_dir = unique_temp_dir("static-import-scope");
    fs::create_dir_all(&temp_dir).unwrap();

    let main_path = temp_dir.join("main.xsl");
    fs::write(
        temp_dir.join("params.xsl"),
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:v="urn:test:variables"
    xmlns:xs="http://www.w3.org/2001/XMLSchema"
    version="3.0">
  <xsl:param name="debug" static="yes" as="xs:string" select="'keep'"/>
  <xsl:variable name="v:debug" static="yes" as="xs:string*"
      select="tokenize($debug, '[,\s]+') ! normalize-space(.)"/>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("imported.xsl"),
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:v="urn:test:variables"
    version="3.0">
  <xsl:template name="action" use-when="'keep' = $v:debug">
    <ok/>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        temp_dir.join("host.xsl"),
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:import href="imported.xsl"/>
</xsl:stylesheet>"#,
    )
    .unwrap();
    fs::write(
        &main_path,
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:include href="params.xsl"/>
  <xsl:include href="host.xsl"/>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let xslt = fs::read_to_string(&main_path).unwrap();
    let mut xot = Xot::new();
    let output = evaluate_named_template_with_stylesheet_base(
        &mut xot,
        "<doc/>",
        &xslt,
        &main_path,
        "action",
    )
    .unwrap();

    assert!(xml(&xot, output).starts_with("<ok"));

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_missing_initial_template_uses_xtde0040() {
  let mut xot = Xot::new();
  let stylesheet_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("tests/fixtures/missing-initial-template.xsl");
  let xslt = r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template name="main">
  <out/>
  </xsl:template>
</xsl:stylesheet>"#;

  let error = evaluate_named_template_with_stylesheet_base(
    &mut xot,
    "<doc/>",
    xslt,
    &stylesheet_path,
    "nonsuch",
  )
  .unwrap_err();

  assert_eq!(error.value(), error::Error::XTDE0040);
}

#[test]
fn test_system_property_product_version_is_available() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="doc">
    <out>
      <xsl:value-of select="system-property('xsl:product-version')"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert!(xml(&xot, output).starts_with("<out>"));
}

#[test]
fn test_message_is_ignored_in_result_sequence() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o>
      <xsl:message>debug</xsl:message>
      <a/>
    </o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<o><a/></o>");
}

#[test]
fn test_local_variable_as_type_is_enforced() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
  <xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3"
    xmlns:xs="http://www.w3.org/2001/XMLSchema">
    <xsl:template match="/">
    <xsl:variable name="v" as="xs:integer" select="true()"/>
    <out value="{$v}"/>
    </xsl:template>
  </xsl:transform>"#,
    );

    assert!(matches!(
        output,
        error::SpannedResult::Err(error::SpannedError {
            error: error::Error::XTTE0570,
            span: _
        })
    ));
}

#[test]
fn test_global_variable_sequence_constructor_creates_temporary_tree() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3"
    xmlns:xs="http://www.w3.org/2001/XMLSchema"
    exclude-result-prefixes="xs">
  <xsl:variable name="data">
    <a xmlns:p="http://p.com/ns"/>
  </xsl:variable>

  <xsl:param name="prefix" select="'p'"/>

  <xsl:template match="/">
    <out>
      <xsl:variable name="uri" select="namespace-uri-for-prefix($prefix, $data/*)" as="xs:string"/>
      <xsl:value-of select="$uri"/>
    </out>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>http://p.com/ns</out>");
}

#[test]
fn test_global_variable_is_out_of_scope_within_its_own_declaration() {
    let namespaces = Namespaces::new(
        Namespaces::default_namespaces(),
        "".to_string(),
        FN_NAMESPACE.to_string(),
    );
    let static_context = StaticContext::from_namespaces(namespaces);
    let output = parse(
        static_context,
        r#"
<xsl:stylesheet version="3.0"
  xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
  xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xsl:template match="/">
  <out att="{$gcd(4,2)}"/>
  </xsl:template>

  <xsl:variable name="gcd" as="function(*)"
    select="function($x as xs:integer, $y as xs:integer) {
    if ($y eq 0)
    then abs($x)
    else $gcd($y,$x mod $y)
    }"/>
</xsl:stylesheet>"#,
    );

    assert!(matches!(
        output,
        error::SpannedResult::Err(error::SpannedError {
            error: error::Error::XPST0008,
            span: _
        })
    ));
}

#[test]
fn test_top_level_non_xsl_elements_do_not_break_parse() {
    let namespaces = Namespaces::new(
        Namespaces::default_namespaces(),
        "".to_string(),
        FN_NAMESPACE.to_string(),
    );
    let static_context = StaticContext::from_namespaces(namespaces);
    let output = parse(
        static_context,
        r#"
<xsl:stylesheet version="2.0"
  xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
  xmlns:test="my:test">
  <?spec xslt#with-param?>
  <test:test/>
  <xsl:template match="/">
  <out/>
  </xsl:template>
</xsl:stylesheet>"#,
    );

    assert!(output.is_ok());
}

#[test]
fn test_builtin_template_rule_passes_params_in_xslt_2_mode() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><group><para/></group></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">
  <xsl:template match="doc">
    <out>
      <xsl:apply-templates>
        <xsl:with-param name="x" select="42"/>
      </xsl:apply-templates>
    </out>
  </xsl:template>

  <xsl:template match="para">
    <xsl:param name="x" select="0"/>
    <x><xsl:value-of select="$x"/></x>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out><x>42</x></out>");
}

#[test]
fn test_transform_local_variable_from_sequence_constructor() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <xsl:variable name="foo"><b>B</b></xsl:variable>
    <o><xsl:value-of select="$foo"/></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<o>B</o>");
}

#[test]
fn test_unused_local_variable_does_not_trigger_global_circularity() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:my="http://www.my.com"
               xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
               version="2.0">
  <xsl:variable name="x" select="my:func(1)"/>

  <xsl:function name="my:func">
    <xsl:param name="a"/>
    <xsl:variable name="b" select="$x"/>
    <xsl:sequence select="$a + 2"/>
  </xsl:function>

  <xsl:template match="/doc">
    <out>
      <xsl:value-of select="$x"/>
    </out>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>3</out>");
}

#[test]

fn test_transform_document_order_dynamically_with_variable() {
    let mut xot = Xot::new();

    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <xsl:variable name="foo"><a><b/><b/></a></xsl:variable>
    <o><xsl:for-each select="$foo//node()"><v/></xsl:for-each></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    // The variable content builds a temporary document node whose only child is
    // the literal <a> element created inside xsl:variable. Therefore
    // $foo//node() yields that <a> plus its two <b> children in document order.
    assert_eq!(xml(&xot, output), "<o><v/><v/><v/></o>");
}

#[test]
fn test_transform_if_true() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3" >
  <xsl:template match="/">
    <o><xsl:if test="1"><foo/></xsl:if></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<o><foo/></o>");
}

#[test]
fn test_transform_if_false() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3" >
  <xsl:template match="/">
    <o><xsl:if test="0"><foo/></xsl:if></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<o/>");
}

#[test]
fn test_transform_choose_when() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3" >
  <xsl:template match="/">
    <o><xsl:choose>
      <xsl:when test="1"><foo/></xsl:when>
      <xsl:otherwise><bar/></xsl:otherwise>
    </xsl:choose></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<o><foo/></o>");
}

#[test]
fn test_transform_choose_otherwise() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3" >
  <xsl:template match="/">
    <o><xsl:choose>
      <xsl:when test="0"><foo/></xsl:when>
      <xsl:otherwise><bar/></xsl:otherwise>
    </xsl:choose></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<o><bar/></o>");
}

#[test]
fn test_transform_choose_when_false_no_otherwise() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3" >
  <xsl:template match="/">
    <o><xsl:choose>
      <xsl:when test="0"><foo/></xsl:when>
    </xsl:choose></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<o/>");
}

#[test]
fn test_transform_multiple_when() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:choose>
      <xsl:when test="0"><foo/></xsl:when>
      <xsl:when test="1"><bar/></xsl:when>
      <xsl:otherwise><baz/></xsl:otherwise>
    </xsl:choose></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<o><bar/></o>");
}

#[test]
fn test_transform_multiple_when_with_otherwise() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3" >
  <xsl:template match="/">
    <o><xsl:choose>
      <xsl:when test="0"><foo/></xsl:when>
      <xsl:when test="0"><bar/></xsl:when>
      <xsl:otherwise><baz/></xsl:otherwise>
    </xsl:choose></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<o><baz/></o>");
}

#[test]
fn test_basic_for_each() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><foo/><foo/><foo/></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:for-each select="doc/foo"><bar/></xsl:for-each></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o><bar/><bar/><bar/></o>");
}

#[test]
fn test_for_each_context() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><foo>0</foo><foo>1</foo><foo>2</foo></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:for-each select="doc/foo">
      <bar><xsl:value-of select="string()"/></bar>
    </xsl:for-each></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(
        xml(&xot, output),
        "<o><bar>0</bar><bar>1</bar><bar>2</bar></o>"
    );
}

#[test]
fn test_copy_empty_sequence() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:copy select="()"/></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o/>");
}

#[test]
fn test_copy_not_one_item_fails() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3" >
  <xsl:template match="/">
    <o><xsl:copy select="(1, 2)"/></o>
  </xsl:template>
</xsl:transform>"#,
    );
    // TODO: check the right error value
    assert!(matches!(output, error::SpannedResult::Err(_)));
}

#[test]
fn test_copy_atom() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
                 <xsl:template match="/">
                   <xsl:variable name="foo"><xsl:copy select="1"/></xsl:variable>
                   <o><xsl:value-of select="string($foo)"/></o>
                 </xsl:template>
              </xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o>1</o>");
}

#[test]
fn test_copy_function() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
                 <xsl:template match="/">
                   <xsl:variable name="foo"><xsl:copy select="function() { 1 }"/></xsl:variable>
                   <o><xsl:value-of select="string($foo)"/></o>
                 </xsl:template>
              </xsl:transform>"#,
    );
    // The function item is rejected while constructing the variable's complex
    // content, before string($foo) gets a chance to atomize it.
    assert!(matches!(
        output,
        error::SpannedResult::Err(error::SpannedError {
            error: error::Error::XTDE0450,
            span: _
        })
    ));
}

#[test]
fn test_copy_text() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc>content</doc>",
        r#"<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
                 <xsl:template match="/">
                   <xsl:variable name="foo"><xsl:copy select="doc/child::node()" /></xsl:variable>
                   <o><xsl:value-of select="string($foo)"/></o>
                 </xsl:template>
              </xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o>content</o>");
}

#[test]
fn test_copy_element() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><p>Content</p></doc>",
        r#"<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
                 <xsl:template match="/">
                   <o><xsl:copy select="doc/*" /></o>
                 </xsl:template>
              </xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o><p/></o>");
}

#[test]
fn test_copy_element_with_sequence_constructor() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><p>Content</p></doc>",
        r#"<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
                 <xsl:template match="/">
                   <o><xsl:copy select="doc/*">Constructed</xsl:copy></o>
                 </xsl:template>
              </xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o><p>Constructed</p></o>");
}

#[test]
fn test_copy_of_atom() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o>
      <xsl:variable name="foo"><xsl:copy-of select="'foo'" /></xsl:variable>
      <xsl:value-of select="string($foo)"/>
    </o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o>foo</o>");
}

#[test]
fn test_copy_of_node() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><foo>FOO</foo></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o>
      <xsl:copy-of select="/doc/foo" />
    </o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o><foo>FOO</foo></o>");
}

#[test]
fn test_sequence() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:value-of><xsl:sequence select="1 to 4" /></xsl:value-of></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o>1 2 3 4</o>");
}

#[test]
fn test_complex_content_single_string() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o>
      <xsl:sequence select="'foo'" />
    </o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o>foo</o>");
}

#[test]
fn test_complex_content_multiple_strings() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o>
      <xsl:sequence select="('foo', 'bar')" />
    </o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o>foo bar</o>");
}

#[test]
fn test_complex_content_xml_and_atomic() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o>
      <xsl:sequence select="('foo', 'bar')" />
      <hello>Hello</hello>
      <xsl:sequence select="('baz', 'qux')" />
    </o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(
        xml(&xot, output),
        "<o>foo bar<hello>Hello</hello>baz qux</o>"
    );
}

#[test]
fn test_function_item_in_complex_content() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:sequence select="function() { 1 }" /></o>
  </xsl:template>
</xsl:transform>"#,
    );

    assert!(matches!(
        output,
        error::SpannedResult::Err(error::SpannedError {
            error: error::Error::XTDE0450,
            span: _
        })
    ));
}

#[test]
fn test_source_nodes_complex_content() {
    let mut xot = Xot::new();
    // try this twice, so that we verify no mutation of source takes place and
    // source code nodes are properly copied
    let output = evaluate(
        &mut xot,
        "<doc><hello>Hello</hello></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o>
      <xsl:sequence select="/doc/hello" />
      <xsl:sequence select="/doc/hello" />
    </o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<o><hello>Hello</hello><hello>Hello</hello></o>"
    );
}

#[test]
fn test_transform_predicate() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><foo>1</foo><foo>2</foo></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:apply-templates select="doc/*" /></o>
  </xsl:template>
  <xsl:template match="foo[2]">
    <found><xsl:value-of select="string()" /></found>
  </xsl:template>
  <xsl:template match="text()" />
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o><found>2</found></o>");
}

#[test]
fn test_transform_predicate_with_attribute() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc><foo>1</foo><foo bar="BAR">2</foo></doc>"#,
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:apply-templates select="doc/*" /></o>
  </xsl:template>
  <xsl:template match="foo[@bar]">
    <found><xsl:value-of select="string()" /></found>
  </xsl:template>
  <xsl:template match="text()" />
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o><found>2</found></o>");
}

#[test]
fn test_text_value_template() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc>VALUE</doc>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o>Value: {string()}</o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<o>Value: VALUE</o>");
}

#[test]
fn test_literal_attribute() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><foo bar="baz"/></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o><foo bar="baz"/></o>"#);
}

#[test]
fn test_literal_attributes() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><foo bar="BAR" qux="QUX"/></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o><foo bar="BAR" qux="QUX"/></o>"#);
}

#[test]
fn test_literal_attribute_with_value_template() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc>value</doc>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><foo bar="found: {doc/string()}"/></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o><foo bar="found: value"/></o>"#);
}

#[test]
fn test_xsl_element() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:element name="foo">content</xsl:element></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o><foo>content</foo></o>"#);
}

#[test]
fn test_xsl_element_with_prefixed_name_uses_static_namespace() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:my="http://www.mytest.net" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:element name="my:elem">content</xsl:element></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        r#"<o><my:elem xmlns:my="http://www.mytest.net">content</my:elem></o>"#
    );
}

#[test]
fn test_xsl_text() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:text>content</xsl:text></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o>content</o>"#);
}

#[test]
fn test_xsl_text_empty() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:text/></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o/>"#);
}

#[test]
fn test_xsl_text_value_template() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:text>Content: {"foo"}</xsl:text></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o>Content: foo</o>"#);
}

#[test]
fn test_xsl_attribute_with_select() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:attribute name="foo" select="'FOO'"/></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o foo="FOO"/>"#);
}

#[test]
fn test_xsl_attribute_name_value_template() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:attribute name="{'foo'}" select="'FOO'"/></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o foo="FOO"/>"#);
}

#[test]
fn test_xsl_attribute_with_content() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:attribute name="foo">FOO</xsl:attribute></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o foo="FOO"/>"#);
}

#[test]
fn test_xsl_attribute_content_defaults_to_empty_separator() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o>
      <xsl:attribute name="foo">
        <xsl:sequence select="1, 2, 3"/>
      </xsl:attribute>
    </o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o foo="123"/>"#);
}

#[test]
fn test_namespace() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:namespace name="foo" select="'http://example.com'"/></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o xmlns:foo="http://example.com"/>"#);
}

#[test]
fn test_comment() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:comment>comment</xsl:comment></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o><!--comment--></o>"#);
}

#[test]
fn test_pi_with_text() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:processing-instruction name="foo">bar</xsl:processing-instruction></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o><?foo bar?></o>"#);
}

#[test]
fn test_pi_without_text() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc/>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:processing-instruction name="foo"/></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o><?foo?></o>"#);
}

#[test]
fn test_priority() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc><foo/></doc>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="foo" priority="1">
    <o>foo</o>
  </xsl:template>
  <xsl:template match="foo" priority="2">
    <o>foo2</o>
  </xsl:template>
  <xsl:template match="/">
    <xsl:apply-templates select="doc/foo"/>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o>foo2</o>"#);
}

#[test]
fn test_priority_declaration_order_last_one_wins() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc><foo/></doc>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="foo" priority="1">
    <o>foo</o>
  </xsl:template>
  <xsl:template match="foo" priority="1">
    <o>foo2</o>
  </xsl:template>
  <xsl:template match="/">
    <xsl:apply-templates select="doc/foo"/>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<o>foo2</o>"#);
}

#[test]
fn test_priority_more_specific_default_priority_wins() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc><foo/></doc>"#,
        r#"
<xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="foo">
    <o>foo</o>
  </xsl:template>
  <xsl:template match="*">
    <o>foo2</o>
  </xsl:template>
  <xsl:template match="/">
    <xsl:apply-templates select="doc/foo"/>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    // foo matches as it's more specific
    assert_eq!(xml(&xot, output), r#"<o>foo</o>"#);
}

// TODO: this test has become unreliable afte rI added tdefault
// template rules. It passes sometimes and doesn't pass other times
// and I don't know why yet. This may be related to unreliable tests
// in the XSLT 3.0 test suite.
// #[test]
// fn test_mode_undeclared() {
//     let mut xot = Xot::new();
//     let output = evaluate(
//         &mut xot,
//         r#"<doc><foo/></doc>"#,
//         r#"
// <xsl:transform expand-text="true" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
//   <xsl:template match="/">
//     <o><xsl:apply-templates select="doc/foo" mode="bar"/></o>
//   </xsl:template>
//   <xsl:template match="foo" mode="bar">
//     <bar/>
//   </xsl:template>
// </xsl:transform>"#,
//     )
//     .unwrap();

//     assert_eq!(xml(&xot, output), r#"<o><bar/></o>"#);
// }

#[test]
fn test_generate_text_node() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc>test</doc>"#,
        r#"<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="2.0">

<xsl:template match="/doc">
  <out>
    <xsl:value-of select="./text()"/>
  </out>
</xsl:template>

<xsl:template match="text()">
  <xsl:value-of select="."/>
</xsl:template>

</xsl:stylesheet>
    "#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), r#"<out>test</out>"#);
}

#[test]
fn test_basic_iterate() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><foo/><foo/><foo/></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:iterate select="doc/foo"><bar/></xsl:iterate></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o><bar/><bar/><bar/></o>");
}

#[test]
fn test_basic_iterate_on_complete() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><foo/><foo/><foo/></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:iterate select="doc/foo"><xsl:on-completion><bar/></xsl:on-completion><baz/></xsl:iterate></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o><baz/><baz/><baz/><bar/></o>");
}

#[test]
fn test_basic_iterate_on_complete_break() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><foo/><foo/><foo/></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:iterate select="doc/foo"><xsl:on-completion><bar/></xsl:on-completion><xsl:break/></xsl:iterate></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o/>");
}

#[test]
fn test_basic_iterate_if_break() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><foo/><foo><x/></foo><foo/></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:iterate select="doc/foo"><baz/><xsl:if test="x"><xsl:break select="'exit at ' || position() || ' of ' || last()"/></xsl:if></xsl:iterate></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(xml(&xot, output), "<o><baz/><baz/>exit at 2 of 3</o>");
}
#[test]
fn test_basic_iterate_params() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><foo/><foo/><foo/></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3">
  <xsl:template match="/">
    <o><xsl:iterate select="doc/foo"><xsl:param name="a" select="1"/><baz><xsl:value-of select="$a"/></baz><xsl:next-iteration><xsl:with-param name="a" select="$a * 2"/></xsl:next-iteration></xsl:iterate></o>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();
    assert_eq!(
        xml(&xot, output),
        "<o><baz>1</baz><baz>2</baz><baz>4</baz></o>"
    );
}

#[test]
fn test_vendor_mode_0015_on_no_match_with_attributes() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<dummy/>",
        r##"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"
  xmlns:xs="http://www.w3.org/2001/XMLSchema"
  exclude-result-prefixes="xs">

  <xsl:mode name="c" on-no-match="shallow-copy"/> 
  <xsl:mode name="d" on-no-match="shallow-skip"/>
  <xsl:mode name="s" on-no-match="text-only-copy"/>
  
  <xsl:variable name="temp" as="element()">
    <foo bar="42"/>
  </xsl:variable>
  
  <xsl:template match="/">
    <xsl:call-template name="main"/>
  </xsl:template>
  
  <xsl:template name="main">
    <out>
      <c><xsl:apply-templates select="$temp" mode="c"/></c>
      <d><xsl:apply-templates select="$temp" mode="d"/></d>
      <s><xsl:apply-templates select="$temp" mode="s"/></s>
    </out>
  </xsl:template>
  
  <xsl:template match="@bar" mode="#all">
    <matched/>
  </xsl:template>
</xsl:stylesheet>"##,
    )
    .unwrap();
    
    let result = xml(&xot,output);
    let expected = "<out><c><foo><matched/></foo></c><d><matched/></d><s/></out>";
    assert_eq!(result, expected, "mode-0015: Output mismatch");
}

#[test]
fn test_vendor_mode_0007_direct_attribute_text_only_copy() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<dummy/>",
        r##"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"
  xmlns:xs="http://www.w3.org/2001/XMLSchema"
  exclude-result-prefixes="xs">

  <xsl:mode name="c" on-no-match="shallow-copy"/>
  <xsl:mode name="d" on-no-match="shallow-skip"/>
  <xsl:mode name="s" on-no-match="text-only-copy"/>

  <xsl:variable name="temp" as="attribute()">
    <xsl:attribute name="a">abracadabra</xsl:attribute>
  </xsl:variable>

  <xsl:template match="/">
    <xsl:call-template name="main"/>
  </xsl:template>

  <xsl:template name="main">
    <out>
      <c><xsl:apply-templates select="$temp" mode="c"/></c>
      <d><xsl:apply-templates select="$temp" mode="d"/></d>
      <s><xsl:apply-templates select="$temp" mode="s"/></s>
    </out>
  </xsl:template>
</xsl:stylesheet>"##,
    )
    .unwrap();

    let result = xml(&xot, output);
    let expected = "<out><c a=\"abracadabra\"/><d/><s>abracadabra</s></out>";
    assert_eq!(result, expected, "mode-0007: Output mismatch");
}

#[test]
fn test_xsl_evaluate_uses_dynamic_xpath_string() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><item/></doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:variable name="expr" select="'self::item'"/>
      <xsl:variable name="result" as="element()*">
        <xsl:evaluate xpath="$expr" context-item="/doc/item"/>
      </xsl:variable>
      <xsl:value-of select="name($result[1])"/>
    </out>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>item</out>");
}

#[test]
fn test_xsl_evaluate_uses_with_params_map() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
               xmlns:xs="http://www.w3.org/2001/XMLSchema"
               version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:variable name="result" as="item()*">
        <xsl:evaluate xpath="'$needle'"
                      with-params="map{xs:QName('needle'): 'ok'}"/>
      </xsl:variable>
      <xsl:value-of select="$result"/>
    </out>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(
      xml(&xot, output),
      "<out xmlns:xs=\"http://www.w3.org/2001/XMLSchema\">ok</out>"
    );
}

#[test]
fn test_xsl_evaluate_uses_namespace_context() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<db:doc xmlns:db=\"http://example.com/db\"><db:item/></db:doc>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:variable name="ns" as="element()">
        <ns xmlns:db="http://example.com/db"/>
      </xsl:variable>
      <xsl:variable name="result" as="element()*">
        <xsl:evaluate xpath="'self::db:item'"
                      context-item="/*/*"
                      namespace-context="$ns"/>
      </xsl:variable>
      <xsl:value-of select="name($result[1])"/>
    </out>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>db:item</out>");
}

#[test]
fn test_xsl_number_value_supports_docbook_picture_set() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:number value="12" format="1"/>
      <xsl:text>|</xsl:text>
      <xsl:number value="3" format="a"/>
      <xsl:text>|</xsl:text>
      <xsl:number value="3" format="A"/>
      <xsl:text>|</xsl:text>
      <xsl:number value="4" format="i"/>
      <xsl:text>|</xsl:text>
      <xsl:number value="4" format="I"/>
    </out>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>12|c|C|iv|IV</out>");
}

#[test]
fn test_xsl_number_value_accepts_dynamic_format_value_template() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
               xmlns:xs="http://www.w3.org/2001/XMLSchema"
               version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:variable name="number" as="xs:integer" select="3"/>
      <xsl:variable name="marks" as="xs:string+" select="('1', 'a')"/>
      <xsl:number value="$number" format="{$marks[count($marks)]}"/>
    </out>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(
      xml(&xot, output),
      "<out xmlns:xs=\"http://www.w3.org/2001/XMLSchema\">c</out>"
    );
}

#[test]
fn test_key_basic_lookup() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc><item id="a" val="alpha"/><item id="b" val="beta"/><item id="c" val="gamma"/></doc>"#,
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:key name="items" match="item" use="@id"/>
  <xsl:template match="/">
    <out><xsl:value-of select="key('items', 'b')/@val"/></out>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>beta</out>");
}

#[test]
fn test_key_with_third_argument() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<doc><group><item id="x" val="one"/></group><group><item id="x" val="two"/></group></doc>"#,
        r#"
<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:key name="items" match="item" use="@id"/>
  <xsl:template match="/">
    <out><xsl:value-of select="key('items', 'x', doc/group[2])/@val"/></out>
  </xsl:template>
</xsl:transform>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>two</out>");
}

#[test]
fn test_xsl_map_with_for_each() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<items><item key='a' val='1'/><item key='b' val='2'/></items>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:map="http://www.w3.org/2005/xpath-functions/map"
    version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:variable name="m" as="map(*)">
        <xsl:map>
          <xsl:for-each select="items/item">
            <xsl:map-entry key="string(@key)" select="string(@val)"/>
          </xsl:for-each>
        </xsl:map>
      </xsl:variable>
      <xsl:value-of select="map:get($m, 'a')"/>-<xsl:value-of select="map:get($m, 'b')"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let rendered = xml(&xot, output);
    assert!(rendered.contains(">1-2</out>"));
}

#[test]
fn test_xsl_map_with_conditional_entries() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc flag='true'/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:map="http://www.w3.org/2005/xpath-functions/map"
    version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:variable name="m" as="map(*)">
        <xsl:map>
          <xsl:map-entry key="'always'" select="'yes'"/>
          <xsl:if test="doc/@flag = 'true'">
            <xsl:map-entry key="'conditional'" select="'included'"/>
          </xsl:if>
        </xsl:map>
      </xsl:variable>
      <xsl:value-of select="map:get($m, 'always')"/>-<xsl:value-of select="map:get($m, 'conditional')"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    let rendered = xml(&xot, output);
    assert!(rendered.contains(">yes-included</out>"));
}

#[test]
fn test_xsl_analyze_string_basic() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc>foo123bar456</doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:analyze-string select="doc" regex="[0-9]+">
        <xsl:matching-substring>
          <n><xsl:value-of select="."/></n>
        </xsl:matching-substring>
        <xsl:non-matching-substring>
          <t><xsl:value-of select="."/></t>
        </xsl:non-matching-substring>
      </xsl:analyze-string>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(
        xml(&xot, output),
        "<out><t>foo</t><n>123</n><t>bar</t><n>456</n></out>"
    );
}

#[test]
fn test_xsl_analyze_string_matching_only() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc>abc</doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:analyze-string select="doc" regex="[a-z]">
        <xsl:matching-substring>
          <xsl:value-of select="upper-case(.)"/>
        </xsl:matching-substring>
      </xsl:analyze-string>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>ABC</out>");
}

#[test]
fn test_xsl_number_level_single_default() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><item>A</item><item>B</item><item>C</item></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:for-each select="doc/item">
        <xsl:if test="position() > 1">,</xsl:if>
        <xsl:number/>
        <xsl:text>:</xsl:text>
        <xsl:value-of select="."/>
      </xsl:for-each>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>1:A,2:B,3:C</out>");
}

#[test]
fn test_xsl_number_level_single_zero_padded() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><item>A</item><item>B</item><item>C</item></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:for-each select="doc/item">
        <xsl:if test="position() > 1">,</xsl:if>
        <xsl:number level="single" format="01"/>
      </xsl:for-each>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>01,02,03</out>");
}

#[test]
fn test_xsl_number_level_single_mixed_siblings() {
    // Only siblings with the same element name should be counted
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><item>A</item><other>X</other><item>B</item><other>Y</other><item>C</item></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:for-each select="doc/item">
        <xsl:if test="position() > 1">,</xsl:if>
        <xsl:number/>
      </xsl:for-each>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>1,2,3</out>");
}

#[test]
fn test_xsl_number_format_zero_padded_width_3() {
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc/>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:number value="7" format="001"/>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    assert_eq!(xml(&xot, output), "<out>007</out>");
}

#[test]
fn test_xsl_number_level_single_count_from() {
    // xsl:number with explicit count and from patterns (DocBook segmentedlist pattern)
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        r#"<list>
  <seg><item>A</item><item>B</item><item>C</item></seg>
  <seg><item>D</item><item>E</item></seg>
</list>"#,
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:for-each select="//item">
        <xsl:if test="position() > 1">,</xsl:if>
        <xsl:number from="seg" count="item"/>
        <xsl:text>:</xsl:text>
        <xsl:value-of select="."/>
      </xsl:for-each>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    // Items restart numbering from each <seg> parent
    assert_eq!(xml(&xot, output), "<out>1:A,2:B,3:C,1:D,2:E</out>");
}

#[test]
fn test_xsl_number_level_single_count_only() {
    // xsl:number with count only (no from)
    let mut xot = Xot::new();
    let output = evaluate(
        &mut xot,
        "<doc><a/><b/><a/><b/><a/></doc>",
        r#"
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
  <xsl:template match="/">
    <out>
      <xsl:for-each select="doc/*">
        <xsl:if test="position() > 1">,</xsl:if>
        <xsl:number count="a"/>
        <xsl:text>:</xsl:text>
        <xsl:value-of select="local-name()"/>
      </xsl:for-each>
    </out>
  </xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();

    // Only <a> elements are counted; <b> elements get 0 (no matching ancestor)
    assert_eq!(xml(&xot, output), "<out>1:a,0:b,2:a,0:b,3:a</out>");
}
