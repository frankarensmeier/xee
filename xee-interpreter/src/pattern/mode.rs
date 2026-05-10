use std::rc::Rc;

use ahash::{HashMap, HashMapExt};

use xee_xpath_ast::Pattern;
use xot::Xot;

use crate::function;

use super::pattern_lookup::NameCache;

use super::pattern_lookup::PatternLookup;
use super::pattern_lookup::NodeKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModeId(usize);

impl ModeId {
    pub fn new(id: usize) -> Self {
        ModeId(id)
    }

    pub fn get(&self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModeLookup<V: Clone> {
    pub(crate) modes: HashMap<ModeId, PatternLookup<V>>,
}

impl<V: Clone> ModeLookup<V> {
    pub(crate) fn new() -> Self {
        Self {
            modes: HashMap::new(),
        }
    }

    pub(crate) fn ensure_index(&self, mode: ModeId, xot: &Xot) {
        if let Some(pattern_lookup) = self.modes.get(&mode) {
            pattern_lookup.ensure_index(xot);
        }
    }

    /// Get the pre-resolved name cache for a mode (cheap Rc clone).
    pub(crate) fn name_cache_rc(&self, mode: ModeId) -> Option<Rc<NameCache>> {
        self.modes.get(&mode)?.name_cache_rc()
    }

    pub(crate) fn lookup(
        &self,
        mode: ModeId,
        name_id: Option<xot::NameId>,
        node_kind: Option<NodeKind>,
        matches: impl FnMut(&Pattern<function::InlineFunctionId>) -> bool,
    ) -> Option<&V> {
        let pattern_lookup = self.modes.get(&mode)?;
        pattern_lookup.lookup(name_id, node_kind, matches)
    }

    pub(crate) fn lookup_with_ambiguity(
        &self,
        mode: ModeId,
        name_id: Option<xot::NameId>,
        node_kind: Option<NodeKind>,
        matches: impl FnMut(&Pattern<function::InlineFunctionId>) -> bool,
        same_rank: impl Fn(&V, &V) -> bool,
    ) -> Option<(&V, bool)> {
        let pattern_lookup = self.modes.get(&mode)?;
        pattern_lookup.lookup_with_ambiguity(name_id, node_kind, matches, same_rank)
    }

    pub(crate) fn lookup_after(
        &self,
        mode: ModeId,
        name_id: Option<xot::NameId>,
        node_kind: Option<NodeKind>,
        matches: impl FnMut(&Pattern<function::InlineFunctionId>) -> bool,
        is_current: impl Fn(&V) -> bool,
    ) -> Option<&V> {
        let pattern_lookup = self.modes.get(&mode)?;
        pattern_lookup.lookup_after(name_id, node_kind, matches, is_current)
    }

    pub(crate) fn lookup_after_lower_import_precedence(
        &self,
        mode: ModeId,
        name_id: Option<xot::NameId>,
        node_kind: Option<NodeKind>,
        current_import_precedence: i64,
        matches: impl FnMut(&Pattern<function::InlineFunctionId>) -> bool,
        is_current: impl Fn(&V) -> bool,
        import_precedence_of: impl Fn(&V) -> i64,
        is_eligible: impl Fn(&V) -> bool,
    ) -> Option<&V> {
        let pattern_lookup = self.modes.get(&mode)?;
        pattern_lookup.lookup_after_lower_import_precedence(
            name_id,
            node_kind,
            current_import_precedence,
            matches,
            is_current,
            import_precedence_of,
            is_eligible,
        )
    }

    pub fn add_rules(
        &mut self,
        mode: ModeId,
        rules: Vec<(Pattern<function::InlineFunctionId>, V)>,
    ) {
        let pattern_lookup = self.modes.entry(mode).or_insert_with(PatternLookup::new);

        pattern_lookup.add_rules(rules);
    }
}
