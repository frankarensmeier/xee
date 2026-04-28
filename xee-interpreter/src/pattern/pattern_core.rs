use xee_xpath_type::ast::KindTest;
use xot::Xot;

use xee_xpath_ast::pattern;

use crate::function::InlineFunctionId;
use crate::sequence::Item;
use crate::xml;

pub(crate) enum NodeMatch {
    Match(Option<xot::Node>),
    NotMatch,
}

/// Constraint on the root node after all backward steps are consumed.
#[derive(Clone, Copy)]
pub(crate) enum RootConstraint {
    /// No constraint — any ancestor chain is fine.
    None,
    /// The remaining node must be a document node.
    Document,
    /// The remaining node must have a document ancestor.
    UnderDocument,
}

pub(crate) trait PredicateMatcher {
    fn match_predicate_with_context(
        &mut self,
        inline_function_id: InlineFunctionId,
        item: &Item,
        position: usize,
        size: usize,
    ) -> bool;

    fn match_predicate(&mut self, inline_function_id: InlineFunctionId, item: &Item) -> bool {
        self.match_predicate_with_context(inline_function_id, item, 1, 1)
    }

    fn xot(&self) -> &Xot;

    fn rooted_pattern_sequence(
        &mut self,
        _root: &pattern::RootExpr,
    ) -> Option<crate::sequence::Sequence> {
        None
    }

    fn matches(&mut self, pattern: &pattern::Pattern<InlineFunctionId>, item: &Item) -> bool {
        match pattern {
            pattern::Pattern::Expr(expr_pattern) => self.matches_expr_pattern(expr_pattern, item),
            pattern::Pattern::Predicate(predicate_pattern) => {
                self.matches_predicate_pattern(predicate_pattern, item)
            }
        }
    }

    fn matches_expr_pattern(
        &mut self,
        expr_pattern: &pattern::ExprPattern<InlineFunctionId>,
        item: &Item,
    ) -> bool {
        if let Item::Node(node) = item {
            match expr_pattern {
                pattern::ExprPattern::Path(path_expr) => self.matches_path_expr(path_expr, *node),
                pattern::ExprPattern::BinaryExpr(binary_expr) => {
                    self.matches_binary_expr(binary_expr, *node)
                }
            }
        } else {
            false
        }
    }

    fn matches_predicate_pattern(
        &mut self,
        predicate_pattern: &pattern::PredicatePattern<InlineFunctionId>,
        item: &Item,
    ) -> bool {
        for predicate in &predicate_pattern.predicates {
            if !self.match_predicate(*predicate, item) {
                return false;
            }
        }
        true
    }

    fn matches_binary_expr(
        &mut self,
        binary_expr: &pattern::BinaryExpr<InlineFunctionId>,
        node: xot::Node,
    ) -> bool {
        match binary_expr.operator {
            pattern::Operator::Union => {
                self.matches_expr_pattern(&binary_expr.left, &Item::from(node))
                    || self.matches_expr_pattern(&binary_expr.right, &Item::from(node))
            }
            pattern::Operator::Intersect => {
                self.matches_expr_pattern(&binary_expr.left, &Item::from(node))
                    && self.matches_expr_pattern(&binary_expr.right, &Item::from(node))
            }
            pattern::Operator::Except => {
                self.matches_expr_pattern(&binary_expr.left, &Item::from(node))
                    && !self.matches_expr_pattern(&binary_expr.right, &Item::from(node))
            }
        }
    }

    fn matches_path_expr(
        &mut self,
        path_expr: &pattern::PathExpr<InlineFunctionId>,
        node: xot::Node,
    ) -> bool {
        match &path_expr.root {
            pattern::PathRoot::Rooted { root, predicates } => {
                self.matches_rooted_path_expr(root, predicates, &path_expr.steps, node)
            }
            pattern::PathRoot::AbsoluteSlash => self.matches_absolute_steps(&path_expr.steps, node),
            pattern::PathRoot::AbsoluteDoubleSlash => {
                self.matches_absolute_double_slash_steps(&path_expr.steps, node)
            }
            pattern::PathRoot::Relative => {
                match self.matches_relative_steps(&path_expr.steps, node) {
                    NodeMatch::Match(_) => true,
                    NodeMatch::NotMatch => false,
                }
            }
        }
    }

    fn matches_rooted_path_expr(
        &mut self,
        root: &pattern::RootExpr,
        predicates: &[InlineFunctionId],
        steps: &[pattern::StepExpr<InlineFunctionId>],
        node: xot::Node,
    ) -> bool {
        let Some(sequence) = self.rooted_pattern_sequence(root) else {
            return false;
        };
        let matches = sequence.iter().any(|item| {
            let Item::Node(root_node) = item else {
                return false;
            };
            let root_item = Item::Node(root_node);
            if predicates
                .iter()
                .any(|predicate| !self.match_predicate(*predicate, &root_item))
            {
                return false;
            }
            self.matches_forward_steps(root_node, steps, node)
        });
        matches
    }

    fn matches_forward_steps(
        &mut self,
        anchor: xot::Node,
        steps: &[pattern::StepExpr<InlineFunctionId>],
        target: xot::Node,
    ) -> bool {
        if steps.is_empty() {
            return anchor == target;
        }
        let step = &steps[0];
        let remaining_steps = &steps[1..];
        match step {
            pattern::StepExpr::AxisStep(axis_step) => {
                for candidate in self.forward_axis_nodes(anchor, axis_step.forward) {
                    if !self.matches_axis_node_test(axis_step, candidate) {
                        continue;
                    }
                    let item = Item::Node(candidate);
                    let (position, size) =
                        self.forward_axis_predicate_context(axis_step, anchor, candidate);
                    if axis_step.predicates.iter().any(|predicate| {
                        !self.match_predicate_with_context(*predicate, &item, position, size)
                    }) {
                        continue;
                    }
                    if self.matches_forward_steps(candidate, remaining_steps, target) {
                        return true;
                    }
                }
                false
            }
            pattern::StepExpr::PostfixExpr(postfix_expr) => {
                self.matches_postfix_expr(postfix_expr, anchor)
                    && self.matches_forward_steps(anchor, remaining_steps, target)
            }
        }
    }

    fn forward_axis_nodes(&self, anchor: xot::Node, axis: pattern::ForwardAxis) -> Vec<xot::Node> {
        match axis {
            pattern::ForwardAxis::Child => self.xot().children(anchor).collect(),
            pattern::ForwardAxis::Descendant => self.xot().descendants(anchor).collect(),
            pattern::ForwardAxis::Attribute => self
                .xot()
                .attributes(anchor)
                .keys()
                .filter_map(|name| self.xot().attributes(anchor).get_node(name))
                .collect(),
            pattern::ForwardAxis::Self_ => vec![anchor],
            pattern::ForwardAxis::DescendantOrSelf => std::iter::once(anchor)
                .chain(self.xot().descendants(anchor))
                .collect(),
            pattern::ForwardAxis::Namespace => Vec::new(),
        }
    }

    fn forward_axis_predicate_context(
        &self,
        step: &pattern::AxisStep<InlineFunctionId>,
        anchor: xot::Node,
        candidate: xot::Node,
    ) -> (usize, usize) {
        let matching_nodes = self
            .forward_axis_nodes(anchor, step.forward)
            .into_iter()
            .filter(|possible| self.matches_axis_node_test(step, *possible))
            .collect::<Vec<_>>();

        let position = matching_nodes
            .iter()
            .position(|possible| *possible == candidate)
            .map(|index| index + 1)
            .unwrap_or(1);
        let size = matching_nodes.len().max(1);
        (position, size)
    }

    fn matches_absolute_steps(
        &mut self,
        steps: &[pattern::StepExpr<InlineFunctionId>],
        node: xot::Node,
    ) -> bool {
        let node_match = self.matches_relative_steps_inner(
            steps, Some(node), pattern::ForwardAxis::Child, None, RootConstraint::Document,
        );
        matches!(node_match, NodeMatch::Match(_))
    }

    fn matches_absolute_double_slash_steps(
        &mut self,
        steps: &[pattern::StepExpr<InlineFunctionId>],
        node: xot::Node,
    ) -> bool {
        let node_match = self.matches_relative_steps_inner(
            steps, Some(node), pattern::ForwardAxis::Child, None, RootConstraint::UnderDocument,
        );
        matches!(node_match, NodeMatch::Match(_))
    }

    fn matches_relative_steps(
        &mut self,
        steps: &[pattern::StepExpr<InlineFunctionId>],
        node: xot::Node,
    ) -> NodeMatch {
        self.matches_relative_steps_inner(
            steps, Some(node), pattern::ForwardAxis::Child, None, RootConstraint::None,
        )
    }

    /// Recursive backward matching through pattern steps.
    ///
    /// Walks the steps in reverse, matching each against the current node and
    /// moving up the tree. Supports backtracking for DescendantOrSelf axis
    /// (from `//`): when a step matches but remaining steps fail, continues
    /// walking up ancestors to try alternative matches.
    fn matches_relative_steps_inner(
        &mut self,
        steps: &[pattern::StepExpr<InlineFunctionId>],
        node: Option<xot::Node>,
        axis: pattern::ForwardAxis,
        deferred: Option<(xot::Node, &pattern::AxisStep<InlineFunctionId>)>,
        root_constraint: RootConstraint,
    ) -> NodeMatch {
        if steps.is_empty() {
            // All steps consumed. Check any remaining deferred predicate.
            if let Some((target_node, deferred_step)) = deferred {
                if let Some(anchor) = node {
                    let (position, size) =
                        self.forward_axis_predicate_context(deferred_step, anchor, target_node);
                    let item = Item::Node(target_node);
                    if !deferred_step.predicates.iter().all(|predicate| {
                        self.match_predicate_with_context(*predicate, &item, position, size)
                    }) {
                        return NodeMatch::NotMatch;
                    }
                }
            }
            // Check root constraint.
            match root_constraint {
                RootConstraint::None => return NodeMatch::Match(node),
                RootConstraint::Document => {
                    if let Some(n) = node {
                        if self.xot().is_document(n) {
                            return NodeMatch::Match(node);
                        }
                    }
                    return NodeMatch::NotMatch;
                }
                RootConstraint::UnderDocument => {
                    if let Some(mut n) = node {
                        loop {
                            if self.xot().is_document(n) {
                                return NodeMatch::Match(Some(n));
                            }
                            if let Some(parent) = self.xot().parent(n) {
                                n = parent;
                            } else {
                                return NodeMatch::NotMatch;
                            }
                        }
                    }
                    return NodeMatch::NotMatch;
                }
            }
        }

        let step = steps.last().unwrap();
        let remaining = &steps[..steps.len() - 1];

        // Detect the synthetic DescendantOrSelf::node() step from `//`.
        // Just propagate the axis without consuming a tree level.
        if Self::is_descendant_or_self_node_step(step) {
            return self.matches_relative_steps_inner(
                remaining,
                node,
                pattern::ForwardAxis::DescendantOrSelf,
                deferred,
                root_constraint,
            );
        }

        let Some(n) = node else {
            return NodeMatch::NotMatch;
        };

        match axis {
            pattern::ForwardAxis::Descendant | pattern::ForwardAxis::DescendantOrSelf => {
                // Walk up ancestors trying each as a match candidate.
                // On match, recurse for remaining steps; on failure, backtrack.
                let mut current = Some(n);
                while let Some(c) = current {
                    let (matched, new_axis) =
                        self.matches_step_expr_no_descendant_predicates(step, c);
                    if matched {
                        // Check deferred predicate: the current node's parent
                        // is the anchor for the deferred descendant predicate.
                        let deferred_ok = if let Some((target_node, deferred_step)) = &deferred {
                            // The anchor for the deferred predicate is the
                            // parent of the node where the descendant step
                            // matched. Since we're in the DescendantOrSelf
                            // branch walking up, `c` is where the current
                            // step matched. The deferred step's anchor is
                            // the parent of `c` (which is the context for
                            // the descendant axis).
                            let (position, size) = self
                                .forward_axis_predicate_context(deferred_step, c, *target_node);
                            let item = Item::Node(*target_node);
                            deferred_step.predicates.iter().all(|predicate| {
                                self.match_predicate_with_context(
                                    *predicate, &item, position, size,
                                )
                            })
                        } else {
                            true
                        };
                        if deferred_ok {
                            let new_deferred = Self::make_deferred(step, c);
                            let next_node = if matches!(new_axis, pattern::ForwardAxis::Self_) {
                                Some(c)
                            } else {
                                self.xot().parent(c)
                            };
                            let result = self.matches_relative_steps_inner(
                                remaining,
                                next_node,
                                new_axis,
                                new_deferred,
                                root_constraint,
                            );
                            if matches!(result, NodeMatch::Match(_)) {
                                return result;
                            }
                        }
                    }
                    current = self.xot().parent(c);
                }
                NodeMatch::NotMatch
            }
            pattern::ForwardAxis::Self_ => {
                let (matched, new_axis) =
                    self.matches_step_expr_no_descendant_predicates(step, n);
                if !matched {
                    return NodeMatch::NotMatch;
                }
                let new_deferred = Self::make_deferred(step, n).or(deferred);
                // Self axis: don't consume a tree level.
                self.matches_relative_steps_inner(remaining, Some(n), new_axis, new_deferred, root_constraint)
            }
            pattern::ForwardAxis::Namespace => NodeMatch::NotMatch,
            _ => {
                // Child/Attribute: must match exactly.
                let (matched, new_axis) =
                    self.matches_step_expr_no_descendant_predicates(step, n);
                if !matched {
                    return NodeMatch::NotMatch;
                }
                // Check deferred predicate with current node as anchor context.
                if let Some((target_node, deferred_step)) = &deferred {
                    let (position, size) =
                        self.forward_axis_predicate_context(deferred_step, n, *target_node);
                    let item = Item::Node(*target_node);
                    if !deferred_step.predicates.iter().all(|predicate| {
                        self.match_predicate_with_context(*predicate, &item, position, size)
                    }) {
                        return NodeMatch::NotMatch;
                    }
                }
                let new_deferred = Self::make_deferred(step, n);
                let next_node = self.xot().parent(n);
                self.matches_relative_steps_inner(remaining, next_node, new_axis, new_deferred, root_constraint)
            }
        }
    }

    /// If the step is a descendant axis step with predicates, return it
    /// paired with the matched node for deferred predicate evaluation.
    fn make_deferred<'a>(
        step: &'a pattern::StepExpr<InlineFunctionId>,
        matched_node: xot::Node,
    ) -> Option<(xot::Node, &'a pattern::AxisStep<InlineFunctionId>)> {
        if let pattern::StepExpr::AxisStep(axis_step) = step {
            if matches!(
                axis_step.forward,
                pattern::ForwardAxis::Descendant | pattern::ForwardAxis::DescendantOrSelf
            ) && !axis_step.predicates.is_empty()
            {
                return Some((matched_node, axis_step));
            }
        }
        None
    }

    /// Matches a step expression, but skips positional predicates for
    /// descendant axis steps (those are deferred for evaluation against the
    /// correct anchor).
    fn matches_step_expr_no_descendant_predicates(
        &mut self,
        step: &pattern::StepExpr<InlineFunctionId>,
        node: xot::Node,
    ) -> (bool, pattern::ForwardAxis) {
        match step {
            pattern::StepExpr::AxisStep(axis_step) => {
                if matches!(
                    axis_step.forward,
                    pattern::ForwardAxis::Descendant | pattern::ForwardAxis::DescendantOrSelf
                ) && !axis_step.predicates.is_empty()
                {
                    // Only check node test, skip predicates (deferred)
                    if !self.matches_axis_node_test(axis_step, node) {
                        return (false, axis_step.forward);
                    }
                    (true, axis_step.forward)
                } else {
                    self.matches_axis_step(axis_step, node)
                }
            }
            pattern::StepExpr::PostfixExpr(postfix_expr) => {
                let matched = self.matches_postfix_expr(postfix_expr, node);
                let axis = if matched {
                    Self::effective_axis_of_expr(&postfix_expr.expr)
                } else {
                    pattern::ForwardAxis::Child
                };
                (matched, axis)
            }
        }
    }

    fn matches_axis_step(
        &mut self,
        step: &pattern::AxisStep<InlineFunctionId>,
        node: xot::Node,
    ) -> (bool, pattern::ForwardAxis) {
        if !self.matches_axis_node_test(step, node) {
            return (false, step.forward);
        }
        if step.predicates.is_empty() {
            return (true, step.forward);
        }
        // Self axis: the context is just the node itself (position=1, size=1).
        // No sibling-based position computation needed.
        if step.forward == pattern::ForwardAxis::Self_ {
            let item = Item::Node(node);
            for predicate in &step.predicates {
                if !self.match_predicate_with_context(*predicate, &item, 1, 1) {
                    return (false, step.forward);
                }
            }
            return (true, step.forward);
        }
        // Evaluate predicates sequentially: each predicate narrows the
        // candidate set, and position/size are computed within the filtered
        // set from previous predicates.
        let Some(parent) = self.xot().parent(node) else {
            let item = Item::Node(node);
            for predicate in &step.predicates {
                if !self.match_predicate_with_context(*predicate, &item, 1, 1) {
                    return (false, step.forward);
                }
            }
            return (true, step.forward);
        };
        let mut candidates = self.axis_sibling_nodes(step, parent);
        for predicate in &step.predicates {
            let position = candidates
                .iter()
                .position(|c| *c == node)
                .map(|i| i + 1);
            let Some(position) = position else {
                // Node not in candidate set after previous predicate filtering
                return (false, step.forward);
            };
            let size = candidates.len();
            let item = Item::Node(node);
            if !self.match_predicate_with_context(*predicate, &item, position, size) {
                return (false, step.forward);
            }
            // Filter candidates to those passing this predicate for the next round
            let mut next_candidates = Vec::new();
            for (i, &candidate) in candidates.iter().enumerate() {
                let cand_item = Item::Node(candidate);
                if self.match_predicate_with_context(*predicate, &cand_item, i + 1, size) {
                    next_candidates.push(candidate);
                }
            }
            candidates = next_candidates;
        }
        (true, step.forward)
    }

    /// Collects sibling nodes on the step's axis that match the node test.
    fn axis_sibling_nodes(
        &self,
        step: &pattern::AxisStep<InlineFunctionId>,
        parent: xot::Node,
    ) -> Vec<xot::Node> {
        match step.forward {
            pattern::ForwardAxis::Child => self
                .xot()
                .children(parent)
                .filter(|candidate| self.matches_axis_node_test(step, *candidate))
                .collect(),
            pattern::ForwardAxis::Attribute => self
                .xot()
                .attributes(parent)
                .keys()
                .filter_map(|name| self.xot().attributes(parent).get_node(name))
                .filter(|candidate| self.matches_axis_node_test(step, *candidate))
                .collect(),
            pattern::ForwardAxis::Descendant => self
                .xot()
                .descendants(parent)
                .filter(|candidate| self.matches_axis_node_test(step, *candidate))
                .collect(),
            pattern::ForwardAxis::DescendantOrSelf => std::iter::once(parent)
                .chain(self.xot().descendants(parent))
                .filter(|candidate| self.matches_axis_node_test(step, *candidate))
                .collect(),
            pattern::ForwardAxis::Self_ => {
                if self.matches_axis_node_test(step, parent) {
                    vec![parent]
                } else {
                    Vec::new()
                }
            }
            pattern::ForwardAxis::Namespace => Vec::new(),
        }
    }

    fn matches_axis_node_test(
        &self,
        step: &pattern::AxisStep<InlineFunctionId>,
        node: xot::Node,
    ) -> bool {
        // Name tests use the principal node kind of the axis: attributes for
        // attribute::, otherwise elements.
        if matches!(step.node_test, pattern::NodeTest::NameTest(_)) {
            if step.forward == pattern::ForwardAxis::Attribute {
                if !self.xot().is_attribute_node(node) {
                    return false;
                }
            } else if !self.xot().is_element(node) {
                return false;
            }
        } else if self.xot().is_attribute_node(node) {
            if step.forward != pattern::ForwardAxis::Attribute {
                return false;
            }
        } else if step.forward == pattern::ForwardAxis::Attribute {
            return false;
        }
        // Namespace nodes only match on the namespace axis
        if self.xot().is_namespace_node(node) {
            if step.forward != pattern::ForwardAxis::Namespace {
                return false;
            }
        } else if step.forward == pattern::ForwardAxis::Namespace {
            return false;
        }
        Self::matches_node_test(&step.node_test, node, self.xot())
    }

    fn axis_predicate_context(
        &self,
        step: &pattern::AxisStep<InlineFunctionId>,
        node: xot::Node,
    ) -> (usize, usize) {
        let Some(parent) = self.xot().parent(node) else {
            return (1, 1);
        };

        let matching_nodes = match step.forward {
            pattern::ForwardAxis::Child => self
                .xot()
                .children(parent)
                .filter(|candidate| self.matches_axis_node_test(step, *candidate))
                .collect::<Vec<_>>(),
            pattern::ForwardAxis::Attribute => self
                .xot()
                .attributes(parent)
                .keys()
                .filter_map(|name| self.xot().attributes(parent).get_node(name))
                .filter(|candidate| self.matches_axis_node_test(step, *candidate))
                .collect::<Vec<_>>(),
            pattern::ForwardAxis::Descendant => self
                .xot()
                .descendants(parent)
                .filter(|candidate| self.matches_axis_node_test(step, *candidate))
                .collect::<Vec<_>>(),
            pattern::ForwardAxis::DescendantOrSelf => std::iter::once(parent)
                .chain(self.xot().descendants(parent))
                .filter(|candidate| self.matches_axis_node_test(step, *candidate))
                .collect::<Vec<_>>(),
            pattern::ForwardAxis::Self_ => {
                if self.matches_axis_node_test(step, node) {
                    vec![node]
                } else {
                    Vec::new()
                }
            }
            pattern::ForwardAxis::Namespace => return (1, 1),
        };

        let position = matching_nodes
            .iter()
            .position(|candidate| *candidate == node)
            .map(|index| index + 1)
            .unwrap_or(1);
        let size = matching_nodes.len().max(1);

        (position, size)
    }

    fn matches_postfix_expr(
        &mut self,
        postfix_expr: &pattern::PostfixExpr<InlineFunctionId>,
        node: xot::Node,
    ) -> bool {
        if !self.matches_expr_pattern(&postfix_expr.expr, &Item::from(node)) {
            return false;
        }
        let item = Item::Node(node);
        for predicate in &postfix_expr.predicates {
            if !self.match_predicate(*predicate, &item) {
                return false;
            }
        }
        true
    }

    fn matches_node_test(node_test: &pattern::NodeTest, node: xot::Node, xot: &Xot) -> bool {
        match node_test {
            pattern::NodeTest::NameTest(name_test) => Self::matches_name_test(name_test, node, xot),
            pattern::NodeTest::KindTest(kind_test) => Self::matches_kind_test(kind_test, node, xot),
        }
    }

    fn matches_name_test(name_test: &pattern::NameTest, node: xot::Node, xot: &Xot) -> bool {
        match name_test {
            pattern::NameTest::Name(expected_name) => {
                // Compare NameId directly — avoids node_name_ref() which walks
                // ancestors to resolve prefixes (O(depth) per comparison).
                // NameId encodes local-name + namespace, which is all that
                // matters for pattern matching. This also correctly matches
                // elements created by xsl:element with dynamic namespaces
                // that lack in-scope prefix declarations.
                if let Some(expected_ref) = expected_name.value.maybe_to_ref(xot) {
                    let expected_name_id = expected_ref.name_id();
                    xot.node_name(node) == Some(expected_name_id)
                } else {
                    false
                }
            }
            pattern::NameTest::Star => true,
            pattern::NameTest::LocalName(expected_local_name) => {
                if let Some(name) = xot.node_name(node) {
                    xot.local_name_str(name) == expected_local_name
                } else {
                    false
                }
            }
            pattern::NameTest::Namespace(ns) => {
                if let Some(name) = xot.node_name(node) {
                    let namespace_uri = xot.uri_str(name);
                    namespace_uri == ns
                } else {
                    false
                }
            }
        }
    }

    fn matches_kind_test(kind_test: &KindTest, node: xot::Node, xot: &Xot) -> bool {
        match kind_test {
            KindTest::Any => !xot.is_document(node),
            _ => xml::kind_test(kind_test, xot, node),
        }
    }

    /// Determines the effective axis for backward pattern matching from a
    /// parenthesized expression. Returns the widest axis found — if any
    /// branch uses Descendant, the backward walk needs ancestor traversal.
    fn effective_axis_of_expr(
        expr: &pattern::ExprPattern<InlineFunctionId>,
    ) -> pattern::ForwardAxis {
        match expr {
            pattern::ExprPattern::Path(path) => {
                // The last step's axis determines the traversal mode
                if let Some(last_step) = path.steps.last() {
                    match last_step {
                        pattern::StepExpr::AxisStep(axis_step) => axis_step.forward,
                        pattern::StepExpr::PostfixExpr(postfix) => {
                            Self::effective_axis_of_expr(&postfix.expr)
                        }
                    }
                } else {
                    pattern::ForwardAxis::Child
                }
            }
            pattern::ExprPattern::BinaryExpr(binary) => {
                let left = Self::effective_axis_of_expr(&binary.left);
                let right = Self::effective_axis_of_expr(&binary.right);
                // Use the widest axis
                if Self::axis_width(left) >= Self::axis_width(right) {
                    left
                } else {
                    right
                }
            }
        }
    }

    fn axis_width(axis: pattern::ForwardAxis) -> u8 {
        match axis {
            pattern::ForwardAxis::Self_ => 0,
            pattern::ForwardAxis::Attribute => 1,
            pattern::ForwardAxis::Child => 1,
            pattern::ForwardAxis::Namespace => 1,
            pattern::ForwardAxis::Descendant => 2,
            pattern::ForwardAxis::DescendantOrSelf => 2,
        }
    }

    /// Detects the synthetic `descendant-or-self::node()` step that the
    /// parser inserts for `//` between path steps. This step only sets the
    /// axis for backward matching and should not consume a tree level.
    fn is_descendant_or_self_node_step(step: &pattern::StepExpr<InlineFunctionId>) -> bool {
        matches!(
            step,
            pattern::StepExpr::AxisStep(pattern::AxisStep {
                forward: pattern::ForwardAxis::DescendantOrSelf,
                node_test: pattern::NodeTest::KindTest(KindTest::Any),
                predicates,
            }) if predicates.is_empty()
        )
    }
}

#[cfg(test)]
mod tests {
    use xee_xpath_ast::pattern::transform_pattern;

    use crate::atomic::Atomic;
    use crate::sequence::Sequence;

    use super::*;

    fn parse_pattern(pattern: &str) -> pattern::Pattern<InlineFunctionId> {
        let namespaces = xee_name::Namespaces::default();
        parse_pattern_namespaces(pattern, &namespaces)
    }

    fn parse_pattern_namespaces(
        pattern: &str,
        namespaces: &xee_name::Namespaces,
    ) -> pattern::Pattern<InlineFunctionId> {
        let variable_names = xee_name::VariableNames::default();
        let pattern = pattern::Pattern::parse(pattern, namespaces, &variable_names).unwrap();

        transform_pattern(&pattern, |_expr| dummy_inline_function_id()).unwrap()
    }

    fn dummy_inline_function_id() -> Result<InlineFunctionId, ()> {
        Ok(InlineFunctionId::new(0))
    }

    struct BasicPredicateMatcher<'a> {
        xot: &'a Xot,
        predicate_matches: bool,
        rooted_sequences: Vec<(pattern::RootExpr, Sequence)>,
    }

    impl<'a> BasicPredicateMatcher<'a> {
        fn new(xot: &'a Xot) -> Self {
            Self {
                xot,
                predicate_matches: false,
                rooted_sequences: Vec::new(),
            }
        }

        fn matching(xot: &'a Xot) -> Self {
            Self {
                xot,
                predicate_matches: true,
                rooted_sequences: Vec::new(),
            }
        }

        fn with_root_sequence(mut self, root: pattern::RootExpr, sequence: Sequence) -> Self {
            self.rooted_sequences.push((root, sequence));
            self
        }
    }

    impl PredicateMatcher for BasicPredicateMatcher<'_> {
        fn match_predicate_with_context(
            &mut self,
            _inline_function_id: InlineFunctionId,
            _item: &Item,
            _position: usize,
            _size: usize,
        ) -> bool {
            self.predicate_matches
        }

        fn xot(&self) -> &Xot {
            self.xot
        }

        fn rooted_pattern_sequence(&mut self, root: &pattern::RootExpr) -> Option<Sequence> {
            self.rooted_sequences
                .iter()
                .find(|(stored_root, _)| stored_root == root)
                .map(|(_, sequence)| sequence.clone())
        }
    }

    #[test]
    fn test_match_name() {
        let mut xot = Xot::new();

        let root = xot.parse(r#"<root><foo/></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();

        let item: Item = node.into();

        let pattern = parse_pattern("foo");

        let mut pm = BasicPredicateMatcher::new(&xot);
        assert!(pm.matches(&pattern, &item));
    }

    #[test]
    fn test_match_star() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<root><foo/></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let item: Item = node.into();

        let pattern = parse_pattern("*");

        let mut pm = BasicPredicateMatcher::new(&xot);
        assert!(pm.matches(&pattern, &item));
    }

    #[test]
    fn test_match_local_name() {
        let mut xot = Xot::new();
        let root = xot
            .parse(r#"<root><foo xmlns="different"/></root>"#)
            .unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let item: Item = node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("*:foo");
        assert!(pm.matches(&pattern, &item));
        let pattern = parse_pattern("foo");
        assert!(!pm.matches(&pattern, &item));
    }

    #[test]
    fn test_match_namespace_name() {
        let mut xot = Xot::new();
        let root = xot
            .parse(r#"<root><foo xmlns="different"/></root>"#)
            .unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let item: Item = node.into();

        let namespaces = xee_name::Namespaces::new(
            vec![
                ("d".to_string(), "different".to_string()),
                ("o".to_string(), "other".to_string()),
            ]
            .into_iter()
            .collect(),
            "".to_string(),
            "".to_string(),
        );

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern_namespaces("d:*", &namespaces);
        assert!(pm.matches(&pattern, &item));
        let pattern = parse_pattern_namespaces("d:foo", &namespaces);
        assert!(pm.matches(&pattern, &item));
        let pattern = parse_pattern_namespaces("o:*", &namespaces);
        assert!(!pm.matches(&pattern, &item));
    }

    #[test]
    fn test_not_match_name() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<root><foo/></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let item: Item = node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("notfound");
        assert!(!pm.matches(&pattern, &item));
    }

    #[test]
    fn test_match_name_nested() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<root><bar><foo/></bar></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let node = xot.first_child(node).unwrap();
        let item: Item = node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("bar/foo");
        assert!(pm.matches(&pattern, &item));
    }

    #[test]
    fn test_match_rooted_variable_pattern() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<doc><foo><baz/></foo></doc>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let baz = xot
            .children(document_element)
            .find(|node| xot.is_element(*node))
            .and_then(|foo| xot.first_child(foo))
            .unwrap();

        let pattern = parse_pattern("$v//baz");
        let variable_name = xee_name::Name::new("v".to_string(), "".to_string(), "".to_string());
        let root_expr = pattern::RootExpr::VarRef(variable_name);
        let variable_sequence = Sequence::from(vec![Item::from(document_element)]);

        let mut pm =
            BasicPredicateMatcher::new(&xot).with_root_sequence(root_expr, variable_sequence);
        assert!(pm.matches(&pattern, &Item::from(baz)));
    }

    #[test]
    fn test_match_rooted_variable_pattern_without_steps_matches_document_node() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<doc><YYY/></doc>"#).unwrap();

        let pattern = parse_pattern("$v");
        let variable_name = xee_name::Name::new("v".to_string(), "".to_string(), "".to_string());
        let root_expr = pattern::RootExpr::VarRef(variable_name);
        let variable_sequence = Sequence::from(vec![Item::from(root)]);

        let mut pm =
            BasicPredicateMatcher::new(&xot).with_root_sequence(root_expr, variable_sequence);
        assert!(pm.matches(&pattern, &Item::from(root)));
    }

    #[test]
    fn test_not_match_name_nested() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<root><bar><foo/></bar></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let item: Item = node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("notfound/foo");
        assert!(!pm.matches(&pattern, &item));
    }

    #[test]
    fn test_match_name_nested_with_explicit_descendant_axis() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<root><bar><foo/></bar></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let node = xot.first_child(node).unwrap();
        let item: Item = node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("bar/descendant::foo");
        assert!(pm.matches(&pattern, &item));
    }

    #[test]
    fn test_not_match_name_nested_with_explicit_descendant_axis() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<root><bar><foo/></bar></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let node = xot.first_child(node).unwrap();
        let item: Item = node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("notfound/descendant::foo");
        assert!(!pm.matches(&pattern, &item));
    }

    #[test]
    fn test_match_name_nested_actually_with_explicit_descendant_axis() {
        let mut xot = Xot::new();
        let root = xot
            .parse(r#"<root><qux><bar><foo/></bar></qux></root>"#)
            .unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let node = xot.first_child(node).unwrap();
        let node = xot.first_child(node).unwrap();
        let item: Item = node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("qux/descendant::foo");
        assert!(pm.matches(&pattern, &item));
    }

    #[test]
    fn test_not_match_name_nested_actually_with_explicit_descendant_axis() {
        let mut xot = Xot::new();
        let root = xot
            .parse(r#"<root><qux><bar><foo/></bar></qux></root>"#)
            .unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let node = xot.first_child(node).unwrap();
        let node = xot.first_child(node).unwrap();
        let item: Item = node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("notfound/descendant::foo");
        assert!(!pm.matches(&pattern, &item));
    }

    #[test]
    fn test_match_name_attribute() {
        let mut xot = Xot::new();
        let bar_name = xot.add_name("bar");
        let root = xot.parse(r#"<root><foo bar="BAR"/></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let node = xot.attributes(node).get_node(bar_name).unwrap();
        let item: Item = node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("@bar");
        assert!(pm.matches(&pattern, &item));
    }

    #[test]
    fn test_not_match_name_attribute() {
        let mut xot = Xot::new();
        let bar_name = xot.add_name("bar");
        let root = xot.parse(r#"<root><foo bar="BAR"/></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let node = xot.attributes(node).get_node(bar_name).unwrap();
        let item: Item = node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("@qux");
        assert!(!pm.matches(&pattern, &item));
    }

    #[test]
    fn test_not_match_name_element_because_its_attribute() {
        let mut xot = Xot::new();
        let bar_name = xot.add_name("bar");
        let root = xot.parse(r#"<root><foo bar="BAR"/></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let node = xot.attributes(node).get_node(bar_name).unwrap();
        let item: Item = node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("bar");
        assert!(!pm.matches(&pattern, &item));
    }

    #[test]
    fn test_not_match_name_element_descendant_because_its_attribute() {
        let mut xot = Xot::new();
        let bar_name = xot.add_name("bar");
        let root = xot.parse(r#"<root><foo bar="BAR"/></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let node = xot.attributes(node).get_node(bar_name).unwrap();
        let item: Item = node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);

        let pattern = parse_pattern("root//bar");
        assert!(!pm.matches(&pattern, &item));
    }

    #[test]
    fn test_not_match_name_attribute_because_its_element() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<root><bar /></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let item: Item = node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("@bar");
        assert!(!pm.matches(&pattern, &item));
    }

    #[test]
    fn test_matches_kind_test_any() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<root><foo bar="BAR" /></root>"#).unwrap();
        let document_item: Item = root.into();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let element_node = node;
        let element_item: Item = element_node.into();
        let attribute_name = xot.add_name("bar");
        let attribute_node = xot.attributes(node).get_node(attribute_name).unwrap();
        let attribute_item: Item = attribute_node.into();

        // the axis determines whether we match. This is a bit
        // counter intuitive, but the spec affirms it.

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("node()");
        assert!(!pm.matches(&pattern, &document_item));
        assert!(pm.matches(&pattern, &element_item));
        assert!(!pm.matches(&pattern, &attribute_item));
        let pattern = parse_pattern("attribute::node()");
        assert!(!pm.matches(&pattern, &element_item));
        assert!(pm.matches(&pattern, &attribute_item));
    }

    #[test]
    fn test_matches_kind_test_element() {
        let mut xot = Xot::new();
        let root = xot
            .parse(r#"<root><foo bar="BAR">text</foo></root>"#)
            .unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let element_node = node;
        let element_item: Item = element_node.into();
        let attribute_name = xot.add_name("bar");
        let attribute_node = xot.attributes(node).get_node(attribute_name).unwrap();
        let attribute_item: Item = attribute_node.into();
        let text_node = xot.first_child(node).unwrap();
        let text_item: Item = text_node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("element()");
        assert!(pm.matches(&pattern, &element_item));
        assert!(!pm.matches(&pattern, &text_item));
        assert!(!pm.matches(&pattern, &attribute_item));
    }

    #[test]
    fn test_matches_absolute_slash() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<root/>"#).unwrap();
        let item: Item = root.into();
        let document_element = xot.document_element(root).unwrap();
        let document_element_item: Item = document_element.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("/");
        assert!(pm.matches(&pattern, &item));
        assert!(!pm.matches(&pattern, &document_element_item));
    }

    #[test]
    fn test_matches_absolute_slash_with_element() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<root/>"#).unwrap();
        let item: Item = root.into();

        let document_element = xot.document_element(root).unwrap();
        let document_element_item: Item = document_element.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("/root");
        assert!(!pm.matches(&pattern, &item));
        assert!(pm.matches(&pattern, &document_element_item));
    }

    #[test]
    fn test_matches_name_test_star_not_document() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<root/>"#).unwrap();
        let item: Item = root.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("*");
        assert!(!pm.matches(&pattern, &item));
    }

    #[test]
    fn test_axis_predicate_context_ignores_non_matching_children() {
        let mut xot = Xot::new();
        let root = xot
            .parse(
                r#"<servlet-mapping>
   <servlet-name>MyServlet</servlet-name>
   <url-pattern>/servlet/MyServlet/*</url-pattern>
</servlet-mapping>"#,
            )
            .unwrap();
        let document_element = xot.document_element(root).unwrap();
        let url_pattern = xot
            .children(document_element)
            .find(|node| {
                xot.is_element(*node)
                    && xot
                        .node_name(*node)
                        .map(|name| xot.local_name_str(name) == "url-pattern")
                        .unwrap_or(false)
            })
            .unwrap();
        let pattern = parse_pattern("url-pattern[position()=last()]");

        let mut pm = BasicPredicateMatcher::matching(&xot);
        assert!(pm.matches(&pattern, &Item::from(url_pattern)));
        assert_eq!(
            pm.axis_predicate_context(
                match &pattern {
                    pattern::Pattern::Expr(pattern::ExprPattern::Path(path_expr)) =>
                        match path_expr.steps.first().unwrap() {
                            pattern::StepExpr::AxisStep(axis_step) => axis_step,
                            _ => panic!("expected axis step"),
                        },
                    _ => panic!("expected path pattern"),
                },
                url_pattern,
            ),
            (1, 1)
        );
    }

    #[test]
    fn test_matches_absolute_double_slash() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<root/>"#).unwrap();
        let item: Item = root.into();

        let document_element = xot.document_element(root).unwrap();
        let document_element_item: Item = document_element.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("//root");
        assert!(!pm.matches(&pattern, &item));
        assert!(pm.matches(&pattern, &document_element_item));
    }

    #[test]
    fn test_matches_absolute_double_slash_nesting() {
        let mut xot = Xot::new();
        let root = xot.parse(r#"<root><foo/></root>"#).unwrap();
        let item: Item = root.into();
        let document_element = xot.document_element(root).unwrap();
        let document_element_item: Item = document_element.into();
        let foo_node = xot.first_child(document_element).unwrap();
        let foo_item: Item = foo_node.into();

        let mut pm = BasicPredicateMatcher::new(&xot);
        let pattern = parse_pattern("//root/foo");
        assert!(!pm.matches(&pattern, &item));
        assert!(!pm.matches(&pattern, &document_element_item));
        assert!(pm.matches(&pattern, &foo_item));
    }

    #[test]
    fn test_predicate_pattern_matches() {
        let xot = Xot::new();
        let mut pm = BasicPredicateMatcher::matching(&xot);
        let atom: Atomic = 1.into();
        let item: Item = atom.into();
        let pattern = parse_pattern(".[. instance of xs:integer]");
        assert!(pm.matches(&pattern, &item));
    }

    #[test]
    fn test_match_name_with_predicate() {
        let mut xot = Xot::new();

        let root = xot.parse(r#"<root><foo/></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let node = xot.first_child(document_element).unwrap();
        let item: Item = node.into();

        let pattern = parse_pattern("foo[1]");

        let mut pm = BasicPredicateMatcher::matching(&xot);
        assert!(pm.matches(&pattern, &item));
    }

    #[test]
    fn test_binary_expr_union() {
        let mut xot = Xot::new();

        let root = xot.parse(r#"<root><foo/><bar/></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let foo_node = xot.first_child(document_element).unwrap();
        let bar_node = xot.next_sibling(foo_node).unwrap();
        let foo_item: Item = foo_node.into();
        let bar_item: Item = bar_node.into();

        let pattern = parse_pattern("foo | bar");

        let mut pm = BasicPredicateMatcher::new(&xot);
        assert!(pm.matches(&pattern, &foo_item));
        assert!(pm.matches(&pattern, &bar_item));
    }

    #[test]
    fn test_binary_expr_intersection() {
        let mut xot = Xot::new();

        let root = xot.parse(r#"<root><foo/><bar/></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let foo_node = xot.first_child(document_element).unwrap();
        let bar_node = xot.next_sibling(foo_node).unwrap();
        let foo_item: Item = foo_node.into();
        let bar_item: Item = bar_node.into();

        let pattern = parse_pattern("(foo | bar) intersect foo");

        let mut pm = BasicPredicateMatcher::new(&xot);
        assert!(pm.matches(&pattern, &foo_item));
        assert!(!pm.matches(&pattern, &bar_item));
    }

    #[test]
    fn test_binary_expr_except() {
        let mut xot = Xot::new();

        let root = xot.parse(r#"<root><foo/><bar/></root>"#).unwrap();
        let document_element = xot.document_element(root).unwrap();
        let foo_node = xot.first_child(document_element).unwrap();
        let bar_node = xot.next_sibling(foo_node).unwrap();
        let foo_item: Item = foo_node.into();
        let bar_item: Item = bar_node.into();

        let pattern = parse_pattern("(foo | bar) except foo");

        let mut pm = BasicPredicateMatcher::new(&xot);
        assert!(!pm.matches(&pattern, &foo_item));
        assert!(pm.matches(&pattern, &bar_item));
    }
}
