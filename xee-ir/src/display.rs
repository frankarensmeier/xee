use std::fmt;

use crate::ir;
use xot::xmlname::NameStrInfo;

/// Wrapper that displays IR `Declarations` in a compact, readable tree format
/// instead of the verbose `Debug` output.
pub struct DisplayDeclarations<'a>(pub &'a ir::Declarations);

impl fmt::Display for DisplayDeclarations<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let decl = self.0;

        // Rules
        if !decl.rules.is_empty() {
            writeln!(f, "rules:")?;
            for (i, rule) in decl.rules.iter().enumerate() {
                write!(f, "  [{i}] ")?;
                write_rule(f, rule)?;
                writeln!(f)?;
                write_function_def(f, &rule.function_definition, 4)?;
            }
        }

        // Modes
        if !decl.modes.is_empty() {
            writeln!(f, "modes:")?;
            for (name, mode) in &decl.modes {
                let name_str = match name {
                    Some(n) => format_owned_name(n),
                    None => "#unnamed".to_string(),
                };
                writeln!(
                    f,
                    "  {name_str}: on-no-match={:?} on-multiple-match={:?}",
                    mode.on_no_match, mode.on_multiple_match,
                )?;
            }
        }

        // Functions
        if !decl.functions.is_empty() {
            writeln!(f, "functions:")?;
            for binding in &decl.functions {
                writeln!(f, "  {}:", binding.name.as_str())?;
                write_function_def(f, &binding.main, 4)?;
            }
        }

        // Global variables
        if !decl.global_variables.is_empty() {
            writeln!(f, "global-variables:")?;
            for gv in &decl.global_variables {
                let orig = gv
                    .original_name
                    .as_ref()
                    .map(|n| format!(" ({})", format_owned_name(n)))
                    .unwrap_or_default();
                let flags = match (gv.external, gv.required) {
                    (true, true) => " [external, required]",
                    (true, false) => " [external]",
                    (false, true) => " [required]",
                    _ => "",
                };
                writeln!(f, "  {}{}{}:", gv.name.as_str(), orig, flags)?;
                write_expr(f, &gv.expr, 4)?;
                writeln!(f)?;
            }
        }

        // Keys
        if !decl.keys.is_empty() {
            writeln!(f, "keys:")?;
            for key in &decl.keys {
                writeln!(f, "  {}:", format_owned_name(&key.name))?;
                writeln!(f, "    pattern: {:?}", key.pattern)?;
                write_function_def(f, &key.use_function, 4)?;
            }
        }

        // Accumulators
        if !decl.accumulators.is_empty() {
            writeln!(f, "accumulators:")?;
            for acc in &decl.accumulators {
                writeln!(f, "  {}:", format_owned_name(&acc.name))?;
                for rule in &acc.rules {
                    writeln!(f, "    {:?} pattern={:?}", rule.phase, rule.pattern)?;
                }
            }
        }

        // Main
        writeln!(f, "main:")?;
        write_function_def(f, &decl.main, 2)?;

        Ok(())
    }
}

fn write_rule(f: &mut fmt::Formatter<'_>, rule: &ir::Rule) -> fmt::Result {
    let modes: Vec<String> = rule
        .modes
        .iter()
        .map(|m| match m {
            ir::ModeValue::Named(n) => format_owned_name(n),
            ir::ModeValue::Unnamed => "#unnamed".to_string(),
            ir::ModeValue::All => "#all".to_string(),
        })
        .collect();
    write!(
        f,
        "priority={} modes=[{}] pattern=",
        rule.priority,
        modes.join(", ")
    )?;
    write_pattern(f, &rule.pattern)
}

fn write_pattern(
    f: &mut fmt::Formatter<'_>,
    pattern: &xee_xpath_ast::Pattern<ir::FunctionDefinition>,
) -> fmt::Result {
    use xee_xpath_ast::pattern::*;
    match pattern {
        Pattern::Expr(ExprPattern::Path(path)) => {
            match &path.root {
                PathRoot::AbsoluteSlash => write!(f, "/")?,
                PathRoot::AbsoluteDoubleSlash => write!(f, "//")?,
                PathRoot::Rooted { root, .. } => write!(f, "{:?}/", root)?,
                PathRoot::Relative => {}
            }
            for (i, step) in path.steps.iter().enumerate() {
                if i > 0 {
                    write!(f, "/")?;
                }
                match step {
                    StepExpr::AxisStep(axis_step) => {
                        write!(f, "{:?}::", axis_step.forward)?;
                        write_node_test(f, &axis_step.node_test)?;
                    }
                    StepExpr::PostfixExpr(pe) => {
                        write!(f, "{:?}", pe.expr)?;
                    }
                }
            }
            Ok(())
        }
        Pattern::Expr(ExprPattern::BinaryExpr(bin)) => {
            write!(f, "({:?} ", bin.operator)?;
            write_pattern_expr(f, &bin.left)?;
            write!(f, " ")?;
            write_pattern_expr(f, &bin.right)?;
            write!(f, ")")
        }
        Pattern::Predicate(pred) => {
            write!(f, "predicate[{}]", pred.predicates.len())
        }
    }
}

fn write_pattern_expr(
    f: &mut fmt::Formatter<'_>,
    expr: &xee_xpath_ast::pattern::ExprPattern<ir::FunctionDefinition>,
) -> fmt::Result {
    use xee_xpath_ast::pattern::*;
    match expr {
        ExprPattern::Path(path) => write_pattern(f, &Pattern::Expr(ExprPattern::Path(path.clone()))),
        ExprPattern::BinaryExpr(bin) => write_pattern(f, &Pattern::Expr(ExprPattern::BinaryExpr(bin.clone()))),
    }
}

fn write_node_test(f: &mut fmt::Formatter<'_>, test: &xee_xpath_ast::ast::NodeTest) -> fmt::Result {
    use xee_xpath_ast::ast::{NodeTest, NameTest};
    match test {
        NodeTest::NameTest(name_test) => match name_test {
            NameTest::Name(name) => {
                let owned = &name.value;
                let ns = owned.namespace();
                let local = owned.local_name();
                if ns.is_empty() {
                    write!(f, "{local}")
                } else {
                    write!(f, "Q{{{ns}}}{local}")
                }
            }
            NameTest::Star => write!(f, "*"),
            NameTest::LocalName(local) => write!(f, "*:{local}"),
            NameTest::Namespace(ns) => write!(f, "{ns}:*"),
        },
        NodeTest::KindTest(kind) => write!(f, "{:?}", kind),
    }
}

fn write_function_def(
    f: &mut fmt::Formatter<'_>,
    fd: &ir::FunctionDefinition,
    indent: usize,
) -> fmt::Result {
    let pad = " ".repeat(indent);
    let param_strs: Vec<String> = fd
        .params
        .iter()
        .map(|p| {
            let mut s = format!("${}", p.name.as_str());
            if let Some(orig) = &p.original_name {
                s.push_str(&format!(" as {orig}"));
            }
            if p.tunnel {
                s.push_str(" [tunnel]");
            }
            if p.required {
                s.push_str(" [required]");
            }
            s
        })
        .collect();
    if let Some(rt) = &fd.return_type {
        writeln!(f, "{pad}({}) as {:?} =>", param_strs.join(", "), rt)?;
    } else {
        writeln!(f, "{pad}({}) =>", param_strs.join(", "))?;
    }
    write_expr(f, &fd.body, indent + 2)?;
    writeln!(f)
}

fn write_expr(f: &mut fmt::Formatter<'_>, expr: &ir::ExprS, indent: usize) -> fmt::Result {
    let pad = " ".repeat(indent);
    match &expr.value {
        ir::Expr::Atom(atom) => {
            write!(f, "{pad}")?;
            write_atom(f, atom)
        }
        ir::Expr::Let(lt) => {
            write!(f, "{pad}let ${} = ", lt.name.as_str())?;
            write_expr_inline(f, &lt.var_expr, indent + 2)?;
            writeln!(f)?;
            write_expr(f, &lt.return_expr, indent)
        }
        ir::Expr::If(if_) => {
            write!(f, "{pad}if ")?;
            write_atom(f, &if_.condition)?;
            writeln!(f)?;
            writeln!(f, "{pad}then")?;
            write_expr(f, &if_.then, indent + 2)?;
            writeln!(f)?;
            writeln!(f, "{pad}else")?;
            write_expr(f, &if_.else_, indent + 2)
        }
        ir::Expr::Binary(bin) => {
            write!(f, "{pad}")?;
            write_atom(f, &bin.left)?;
            write!(f, " {:?} ", bin.op)?;
            write_atom(f, &bin.right)
        }
        ir::Expr::Unary(un) => {
            write!(f, "{pad}{:?} ", un.op)?;
            write_atom(f, &un.atom)
        }
        ir::Expr::FunctionDefinition(fd) => {
            writeln!(f, "{pad}function")?;
            write_function_def(f, fd, indent + 2)
        }
        ir::Expr::FunctionCall(call) => {
            write!(f, "{pad}call ")?;
            write_atom(f, &call.atom)?;
            write!(f, "(")?;
            for (i, arg) in call.args.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write_atom(f, arg)?;
            }
            write!(f, ")")
        }
        ir::Expr::Step(step) => {
            write!(f, "{pad}step {:?}::", step.step.axis)?;
            write_node_test(f, &step.step.node_test)?;
            write!(f, " from ")?;
            write_atom(f, &step.context)
        }
        ir::Expr::Deduplicate(inner) => {
            writeln!(f, "{pad}deduplicate")?;
            write_expr(f, inner, indent + 2)
        }
        ir::Expr::Map(map) => {
            write!(f, "{pad}map ")?;
            write_atom(f, &map.var_atom)?;
            writeln!(f, " =>")?;
            write_expr(f, &map.return_expr, indent + 2)
        }
        ir::Expr::Filter(filter) => {
            write!(f, "{pad}filter ")?;
            write_atom(f, &filter.var_atom)?;
            writeln!(f, " =>")?;
            write_expr(f, &filter.return_expr, indent + 2)
        }
        ir::Expr::Iterate(it) => {
            write!(f, "{pad}iterate ")?;
            write_atom(f, &it.var_atom)?;
            writeln!(f)?;
            write_expr(f, &it.expr, indent + 2)
        }
        ir::Expr::IterateBreak(brk) => {
            writeln!(f, "{pad}break {}", brk.loop_name.as_str())?;
            write_expr(f, &brk.return_expr, indent + 2)
        }
        ir::Expr::IterateLetNext(next) => {
            writeln!(f, "{pad}next-iteration")?;
            write_expr(f, &next.return_expr, indent + 2)
        }
        ir::Expr::Quantified(q) => {
            write!(f, "{pad}{:?} ", q.quantifier)?;
            write_atom(f, &q.var_atom)?;
            writeln!(f, " satisfies")?;
            write_expr(f, &q.satisifies_expr, indent + 2)
        }
        ir::Expr::PatternPredicate(pp) => {
            write!(f, "{pad}pattern-predicate ")?;
            write_atom(f, &pp.var_atom)?;
            writeln!(f)?;
            write_expr(f, &pp.expr, indent + 2)
        }
        ir::Expr::Cast(cast) => {
            write!(f, "{pad}cast ")?;
            write_atom(f, &cast.atom)?;
            write!(f, " as {:?}", cast.xs)?;
            if cast.empty_sequence_allowed {
                write!(f, "?")?;
            }
            Ok(())
        }
        ir::Expr::Castable(c) => {
            write!(f, "{pad}castable ")?;
            write_atom(f, &c.atom)?;
            write!(f, " as {:?}", c.xs)?;
            if c.empty_sequence_allowed {
                write!(f, "?")?;
            }
            Ok(())
        }
        ir::Expr::InstanceOf(io) => {
            write!(f, "{pad}instance-of ")?;
            write_atom(f, &io.atom)?;
            write!(f, " as {:?}", io.sequence_type)
        }
        ir::Expr::Treat(t) => {
            write!(f, "{pad}treat ")?;
            write_atom(f, &t.atom)?;
            write!(f, " as {:?}", t.sequence_type)
        }
        ir::Expr::ConvertSequence(cs) => {
            write!(f, "{pad}convert-sequence ")?;
            write_atom(f, &cs.atom)?;
            write!(f, " as {:?}", cs.sequence_type)
        }
        ir::Expr::MapConstructor(mc) => {
            writeln!(f, "{pad}map {{")?;
            for (k, v) in &mc.members {
                write!(f, "{pad}  ")?;
                write_atom(f, k)?;
                write!(f, ": ")?;
                write_atom(f, v)?;
                writeln!(f)?;
            }
            write!(f, "{pad}}}")
        }
        ir::Expr::ArrayConstructor(ac) => match ac {
            ir::ArrayConstructor::Square(items) => {
                write!(f, "{pad}[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write_atom(f, item)?;
                }
                write!(f, "]")
            }
            ir::ArrayConstructor::Curly(atom) => {
                write!(f, "{pad}array{{")?;
                write_atom(f, atom)?;
                write!(f, "}}")
            }
        },
        ir::Expr::Lookup(lu) => {
            write!(f, "{pad}lookup ")?;
            write_atom(f, &lu.atom)?;
            write!(f, "?")?;
            write_atom(f, &lu.arg_atom)
        }
        ir::Expr::WildcardLookup(lu) => {
            write!(f, "{pad}lookup ")?;
            write_atom(f, &lu.atom)?;
            write!(f, "?*")
        }
        ir::Expr::XmlName(xn) => {
            write!(f, "{pad}xml-name(")?;
            write_atom(f, &xn.local_name)?;
            write!(f, ", ")?;
            write_atom(f, &xn.namespace)?;
            write!(f, ")")
        }
        ir::Expr::XmlDocument(_) => {
            write!(f, "{pad}xml-document")
        }
        ir::Expr::XmlElement(el) => {
            write!(f, "{pad}xml-element(")?;
            write_atom(f, &el.name)?;
            write!(f, ")")
        }
        ir::Expr::XmlAttribute(attr) => {
            write!(f, "{pad}xml-attribute(")?;
            write_atom(f, &attr.name)?;
            write!(f, ", ")?;
            write_atom(f, &attr.value)?;
            write!(f, ")")
        }
        ir::Expr::XmlNamespace(ns) => {
            write!(f, "{pad}xml-namespace(")?;
            write_atom(f, &ns.prefix)?;
            write!(f, ", ")?;
            write_atom(f, &ns.namespace)?;
            write!(f, ")")
        }
        ir::Expr::XmlText(txt) => {
            write!(f, "{pad}xml-text(")?;
            write_atom(f, &txt.value)?;
            write!(f, ")")
        }
        ir::Expr::XmlComment(c) => {
            write!(f, "{pad}xml-comment(")?;
            write_atom(f, &c.value)?;
            write!(f, ")")
        }
        ir::Expr::XmlProcessingInstruction(pi) => {
            write!(f, "{pad}xml-pi(")?;
            write_atom(f, &pi.target)?;
            write!(f, ", ")?;
            write_atom(f, &pi.content)?;
            write!(f, ")")
        }
        ir::Expr::XmlAppend(app) => {
            write!(f, "{pad}xml-append(")?;
            write_atom(f, &app.parent)?;
            write!(f, ", ")?;
            write_atom(f, &app.child)?;
            write!(f, ")")
        }
        ir::Expr::ApplyTemplates(at) => {
            let mode = match &at.mode {
                ir::ApplyTemplatesModeValue::Named(n) => format_owned_name(n),
                ir::ApplyTemplatesModeValue::Unnamed => "#unnamed".to_string(),
                ir::ApplyTemplatesModeValue::Current => "#current".to_string(),
            };
            write!(f, "{pad}apply-templates mode={mode} select=")?;
            write_atom(f, &at.select)?;
            if !at.params.is_empty() {
                write_with_params(f, &at.params, indent + 2)?;
            }
            Ok(())
        }
        ir::Expr::CallTemplate(ct) => {
            write!(f, "{pad}call-template {}", ct.name.as_str())?;
            if !ct.params.is_empty() {
                write_with_params(f, &ct.params, indent + 2)?;
            }
            Ok(())
        }
        ir::Expr::ContinueTemplate(ct) => {
            let kind = match ct.behavior {
                ir::ContinueBehavior::NextMatch => "next-match",
                ir::ContinueBehavior::ApplyImports => "apply-imports",
            };
            write!(f, "{pad}{kind}")?;
            if !ct.params.is_empty() {
                write_with_params(f, &ct.params, indent + 2)?;
            }
            Ok(())
        }
        ir::Expr::CopyShallow(cs) => {
            write!(f, "{pad}copy-shallow ")?;
            write_atom(f, &cs.select)
        }
        ir::Expr::CopyDeep(cd) => {
            write!(f, "{pad}copy-deep ")?;
            write_atom(f, &cd.select)
        }
        ir::Expr::RaiseError(err) => {
            write!(f, "{pad}raise-error {:?}", err)
        }
    }
}

fn write_expr_inline(f: &mut fmt::Formatter<'_>, expr: &ir::ExprS, indent: usize) -> fmt::Result {
    match &expr.value {
        ir::Expr::Atom(atom) => write_atom(f, atom),
        ir::Expr::FunctionCall(call) => {
            write!(f, "call ")?;
            write_atom(f, &call.atom)?;
            write!(f, "(")?;
            for (i, arg) in call.args.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write_atom(f, arg)?;
            }
            write!(f, ")")
        }
        ir::Expr::Binary(bin) => {
            write_atom(f, &bin.left)?;
            write!(f, " {:?} ", bin.op)?;
            write_atom(f, &bin.right)
        }
        ir::Expr::Unary(un) => {
            write!(f, "{:?} ", un.op)?;
            write_atom(f, &un.atom)
        }
        ir::Expr::XmlName(xn) => {
            write!(f, "xml-name(")?;
            write_atom(f, &xn.local_name)?;
            write!(f, ", ")?;
            write_atom(f, &xn.namespace)?;
            write!(f, ")")
        }
        ir::Expr::XmlElement(el) => {
            write!(f, "xml-element(")?;
            write_atom(f, &el.name)?;
            write!(f, ")")
        }
        ir::Expr::XmlText(txt) => {
            write!(f, "xml-text(")?;
            write_atom(f, &txt.value)?;
            write!(f, ")")
        }
        ir::Expr::XmlAppend(app) => {
            write!(f, "xml-append(")?;
            write_atom(f, &app.parent)?;
            write!(f, ", ")?;
            write_atom(f, &app.child)?;
            write!(f, ")")
        }
        ir::Expr::Cast(cast) => {
            write!(f, "cast ")?;
            write_atom(f, &cast.atom)?;
            write!(f, " as {:?}", cast.xs)?;
            if cast.empty_sequence_allowed {
                write!(f, "?")?;
            }
            Ok(())
        }
        // For anything else inline, fall back to the indented form on a new line
        _ => {
            writeln!(f)?;
            write_expr(f, expr, indent)
        }
    }
}

fn write_atom(f: &mut fmt::Formatter<'_>, atom: &ir::AtomS) -> fmt::Result {
    match &atom.value {
        ir::Atom::Const(c) => write_const(f, c),
        ir::Atom::Variable(name) => write!(f, "${}", name.as_str()),
    }
}

fn write_const(f: &mut fmt::Formatter<'_>, c: &ir::Const) -> fmt::Result {
    match c {
        ir::Const::Integer(i) => write!(f, "{i}"),
        ir::Const::String(s) => write!(f, "{s:?}"),
        ir::Const::Double(d) => write!(f, "{d}"),
        ir::Const::Decimal(d) => write!(f, "{d}"),
        ir::Const::StaticFunctionReference(id, _) => write!(f, "fn#{}", id.as_u16()),
        ir::Const::EmptySequence => write!(f, "()"),
    }
}

fn write_with_params(
    f: &mut fmt::Formatter<'_>,
    params: &[ir::WithParam],
    indent: usize,
) -> fmt::Result {
    let pad = " ".repeat(indent);
    for p in params {
        let tunnel = if p.tunnel { " [tunnel]" } else { "" };
        write!(f, "\n{pad}with ${}{tunnel} = ", p.name.as_str())?;
        if let Some(select) = &p.select {
            write_atom(f, select)?;
        } else if let Some(body) = &p.sequence_constructor {
            writeln!(f)?;
            write_expr(f, body, indent + 2)?;
        }
    }
    Ok(())
}

fn format_owned_name(name: &xot::xmlname::OwnedName) -> String {
    match name.namespace() {
        "" => name.local_name().to_string(),
        ns => format!("Q{{{ns}}}{}", name.local_name()),
    }
}
