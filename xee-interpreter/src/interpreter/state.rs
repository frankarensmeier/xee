use std::cell::RefCell;
use std::rc::Rc;

use ahash::HashMap;
use ahash::HashMapExt;
use xot::Xot;

use crate::atomic;
use crate::error;
use crate::function;
use crate::sequence;
use crate::stack;
use super::program::Program;

const FRAMES_LIMIT: usize = 16_384;

/// Stack watchpoint for debugging stack corruption.
///
/// Activated by setting XEE_WATCH_STACK=<position> environment variable.
/// Logs every mutation that affects the watched stack position, including
/// the operation, old/new values, and the full frame chain.
#[derive(Debug)]
pub(crate) struct Watchpoint {
    position: usize,
    previous_len: usize,
}

impl Watchpoint {
    fn from_env() -> Option<Self> {
        std::env::var("XEE_WATCH_STACK").ok().and_then(|s| {
            s.parse::<usize>().ok().map(|pos| Watchpoint {
                position: pos,
                previous_len: 0,
            })
        })
    }

    fn check(
        &mut self,
        operation: &str,
        stack: &[stack::Value],
        frames: &[Frame],
    ) {
        let new_len = stack.len();

        // Report if the watched position was affected
        if self.position < new_len {
            // Position exists — report current value
            let value = &stack[self.position];
            let item_count = match value {
                stack::Value::Absent => 0,
                stack::Value::Sequence(s) => s.len(),
            };

            // If stack grew to include this position, or shrank past it and came back
            if self.previous_len <= self.position && new_len > self.position {
                eprintln!(
                    "[WATCH] stack[{}] CREATED by {}: {:?} ({} items) | stack_len: {} | frames: {}",
                    self.position,
                    operation,
                    Self::value_summary(value),
                    item_count,
                    new_len,
                    Self::frame_summary(frames),
                );
            }
        } else if self.previous_len > self.position && new_len <= self.position {
            // Position was destroyed (truncated away)
            eprintln!(
                "[WATCH] stack[{}] DESTROYED by {} | stack_len: {} -> {} | frames: {}",
                self.position,
                operation,
                self.previous_len,
                new_len,
                Self::frame_summary(frames),
            );
        }

        self.previous_len = new_len;
    }

    fn check_overwrite(
        &self,
        operation: &str,
        position: usize,
        old_value: &stack::Value,
        new_value: &stack::Value,
        stack: &[stack::Value],
        frames: &[Frame],
    ) {
        if position != self.position {
            return;
        }
        let old_count = match old_value {
            stack::Value::Absent => 0,
            stack::Value::Sequence(s) => s.len(),
        };
        let new_count = match new_value {
            stack::Value::Absent => 0,
            stack::Value::Sequence(s) => s.len(),
        };
        eprintln!(
            "[WATCH] stack[{}] OVERWRITTEN by {}: {:?} ({} items) -> {:?} ({} items) | stack_len: {} | frames: {}",
            self.position,
            operation,
            Self::value_summary(old_value),
            old_count,
            Self::value_summary(new_value),
            new_count,
            stack.len(),
            Self::frame_summary(frames),
        );
    }

    fn value_summary(value: &stack::Value) -> String {
        match value {
            stack::Value::Absent => "Absent".to_string(),
            stack::Value::Sequence(s) => {
                let len = s.len();
                if len == 0 {
                    "Empty".to_string()
                } else {
                    // Show first item type and total count
                    let first = s.iter().next().map(|item| format!("{:?}", item)).unwrap_or_default();
                    if first.len() > 80 {
                        format!("{}... ({} items)", &first[..80], len)
                    } else if len == 1 {
                        first
                    } else {
                        format!("{} (+{} more)", first, len - 1)
                    }
                }
            }
        }
    }

    fn frame_summary(frames: &[Frame]) -> String {
        frames
            .iter()
            .map(|f| format!("fn{}@base{}", f.function.get(), f.base))
            .collect::<Vec<_>>()
            .join(" > ")
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StateCheckpoint {
    stack_len: usize,
    build_stack_len: usize,
    build_item_lens: Vec<usize>,
    mutation_count: usize,
    frames_len: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct Frame {
    function: function::InlineFunctionId,
    base: usize,
    pub(crate) ip: usize,
    owned_program: Option<Rc<Program>>,
}

impl Frame {
    pub(crate) fn function(&self) -> function::InlineFunctionId {
        self.function
    }
    pub(crate) fn base(&self) -> usize {
        self.base
    }

    pub(crate) fn owned_program(&self) -> Option<&Rc<Program>> {
        self.owned_program.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RegexKey {
    pattern: String,
    flags: String,
}

#[derive(Debug)]
pub struct State<'a> {
    stack: Vec<stack::Value>,
    build_stack: Vec<BuildStackEntry>,
    mutation_count: usize,
    frames: Vec<Frame>,
    regex_cache: RefCell<HashMap<RegexKey, Rc<regexml::Regex>>>,
    regex_groups: Vec<Vec<String>>,
    current_group_stack: Vec<sequence::Sequence>,
    current_grouping_key_stack: Vec<Option<atomic::Atomic>>,
    pub(crate) xot: &'a mut Xot,
    /// Maps namespace nodes to their parent element node.
    /// Populated when namespace axis is traversed; needed because xot
    /// namespace nodes created via new_namespace_node() are orphaned.
    pub(crate) namespace_parents: HashMap<xot::Node, xot::Node>,
    watchpoint: Option<Watchpoint>,
}

#[derive(Debug)]
struct ItemBuildStackEntry {
    build_stack: Vec<sequence::Item>,
}

#[derive(Debug)]
struct BuildStackEntry {
    item: ItemBuildStackEntry,
}

impl BuildStackEntry {
    fn new() -> Self {
        Self {
            item: ItemBuildStackEntry {
                build_stack: Vec::new(),
            },
        }
    }

    fn push(&mut self, item: sequence::Item) {
        self.item.build_stack.push(item);
    }

    fn extend<I: Iterator<Item = sequence::Item>>(
        &mut self,
        items: impl IntoIterator<Item = sequence::Item, IntoIter = I>,
    ) {
        self.item.build_stack.extend(items);
    }
}

impl From<BuildStackEntry> for sequence::Sequence {
    fn from(build: BuildStackEntry) -> Self {
        sequence::Sequence::new(build.item.build_stack)
    }
}

impl<'a> State<'a> {
    pub(crate) fn new(xot: &'a mut Xot) -> Self {
        let watchpoint = Watchpoint::from_env();
        if let Some(ref wp) = watchpoint {
            eprintln!("[WATCH] Stack watchpoint active on position {}", wp.position);
        }
        Self {
            stack: vec![],
            build_stack: vec![],
            mutation_count: 0,
            frames: Vec::new(),
            regex_cache: RefCell::new(HashMap::new()),
            regex_groups: vec![],
            current_group_stack: vec![],
            current_grouping_key_stack: vec![],
            xot,
            namespace_parents: HashMap::new(),
            watchpoint,
        }
    }

    pub(crate) fn push<T>(&mut self, sequence: T)
    where
        T: Into<sequence::Sequence>,
    {
        let sequence: sequence::Sequence = sequence.into();
        self.stack.push(sequence.into());
        if let Some(ref mut wp) = self.watchpoint {
            wp.check("push", &self.stack, &self.frames);
        }
    }

    pub(crate) fn push_value<T>(&mut self, value: T)
    where
        T: Into<stack::Value>,
    {
        self.stack.push(value.into());
        if let Some(ref mut wp) = self.watchpoint {
            wp.check("push_value", &self.stack, &self.frames);
        }
    }

    pub(crate) fn build_new(&mut self) {
        self.build_stack.push(BuildStackEntry::new());
    }

    pub(crate) fn build_push(&mut self) -> error::Result<()> {
        let value = self.pop()?;
        let build = self.build_stack.last_mut().unwrap();
        match value {
            sequence::Sequence::Empty(_) => {}
            sequence::Sequence::One(item) => build.push(item.into_item()),
            // any other sequence
            sequence => build.extend(sequence.iter()),
        }
        Ok(())
    }

    pub(crate) fn build_complete(&mut self) {
        let build = self.build_stack.pop().unwrap();
        self.stack.push(build.into());
        if let Some(ref mut wp) = self.watchpoint {
            wp.check("build_complete", &self.stack, &self.frames);
        }
    }

    pub(crate) fn push_var(&mut self, index: usize) {
        self.stack
            .push(self.stack[self.frame().base + index].clone());
        if let Some(ref mut wp) = self.watchpoint {
            wp.check(&format!("push_var({})", index), &self.stack, &self.frames);
        }
    }

    pub(crate) fn var_is_absent(&self, index: usize) -> bool {
        self.stack[self.frame().base + index].is_absent()
    }

    pub(crate) fn push_closure_var(&mut self, index: usize) -> error::Result<()> {
        let function = self.function()?;
        let closure_vars = function.closure_vars();
        self.stack.push(closure_vars[index].clone());
        if let Some(ref mut wp) = self.watchpoint {
            wp.check(&format!("push_closure_var({})", index), &self.stack, &self.frames);
        }
        Ok(())
    }

    pub(crate) fn set_var(&mut self, index: usize) {
        let base = self.frame().base;
        let pos = base + index;
        if let Some(ref wp) = self.watchpoint {
            let old_value = &self.stack[pos];
            let new_value = self.stack.last().unwrap();
            wp.check_overwrite(
                &format!("set_var({})", index),
                pos,
                old_value,
                new_value,
                &self.stack,
                &self.frames,
            );
        }
        self.stack[pos] = self.stack.pop().unwrap();
    }

    #[inline]
    pub(crate) fn pop(&mut self) -> error::Result<sequence::Sequence> {
        let result = self.pop_value();
        if let Some(ref mut wp) = self.watchpoint {
            wp.check("pop", &self.stack, &self.frames);
        }
        result.try_into()
    }

    #[inline]
    pub(crate) fn pop_value(&mut self) -> stack::Value {
        let value = self.stack.pop().unwrap();
        if let Some(ref mut wp) = self.watchpoint {
            wp.check("pop_value", &self.stack, &self.frames);
        }
        value
    }

    pub(crate) fn function(&self) -> error::Result<function::Function> {
        // the function is always just below the base
        let value = &self.stack[self.frame().base - 1];
        match value {
            stack::Value::Sequence(sequence) => sequence.clone().try_into(),
            stack::Value::Absent => Err(error::Error::XPDY0002),
        }
    }

    pub(crate) fn push_start_frame(&mut self, function_id: function::InlineFunctionId) {
        self.frames.push(Frame {
            function: function_id,
            ip: 0,
            base: 0,
            owned_program: None,
        });
    }

    pub(crate) fn checkpoint(&self) -> StateCheckpoint {
        StateCheckpoint {
            stack_len: self.stack.len(),
            build_stack_len: self.build_stack.len(),
            build_item_lens: self
                .build_stack
                .iter()
                .map(|entry| entry.item.build_stack.len())
                .collect(),
            mutation_count: self.mutation_count,
            frames_len: self.frames.len(),
        }
    }

    pub(crate) fn restore(&mut self, checkpoint: StateCheckpoint) {
        for (entry, len) in self
            .build_stack
            .iter_mut()
            .zip(checkpoint.build_item_lens.iter().copied())
        {
            entry.item.build_stack.truncate(len);
        }
        self.stack.truncate(checkpoint.stack_len);
        if let Some(ref mut wp) = self.watchpoint {
            wp.check("restore", &self.stack, &self.frames);
        }
        self.build_stack.truncate(checkpoint.build_stack_len);
        self.mutation_count = checkpoint.mutation_count;
        while self.frames.len() > checkpoint.frames_len {
            self.frames.pop();
        }
    }

    pub(crate) fn output_changed_since(&self, checkpoint: &StateCheckpoint) -> bool {
        if self.mutation_count != checkpoint.mutation_count {
            return true;
        }

        if self.build_stack.len() != checkpoint.build_stack_len {
            return true;
        }

        self.build_stack
            .iter()
            .zip(checkpoint.build_item_lens.iter())
            .any(|(entry, saved_len)| entry.item.build_stack.len() != *saved_len)
    }

    pub(crate) fn record_output_mutation(&mut self) {
        self.mutation_count += 1;
    }

    pub(crate) fn push_frame(
        &mut self,
        function_id: function::InlineFunctionId,
        arity: usize,
        owned_program: Option<Rc<Program>>,
    ) -> error::Result<()> {
        if self.frames.len() >= FRAMES_LIMIT {
            return Err(error::Error::StackOverflow);
        }
        self.frames.push(Frame {
            function: function_id,
            ip: 0,
            base: self.stack.len() - arity,
            owned_program,
        });
        Ok(())
    }

    pub(crate) fn frame(&self) -> &Frame {
        self.frames.last().unwrap()
    }

    pub(crate) fn frame_mut(&mut self) -> &mut Frame {
        self.frames.last_mut().unwrap()
    }

    pub(crate) fn jump(&mut self, displacement: i32) {
        self.frame_mut().ip = (self.frame().ip as i32 + displacement) as usize;
    }

    pub(crate) fn callable(&self, arity: usize) -> error::Result<function::Function> {
        let value = &self.stack[self.stack.len() - (arity + 1)];
        match value {
            stack::Value::Sequence(sequence) => sequence.clone().try_into(),
            stack::Value::Absent => Err(error::Error::XPDY0002),
        }
    }

    pub(crate) fn arguments(&self, arity: usize) -> &[stack::Value] {
        &self.stack[self.stack.len() - arity..]
    }

    pub(crate) fn truncate_arguments(&mut self, arity: usize) {
        self.stack.truncate(self.stack.len() - arity);
        if let Some(ref mut wp) = self.watchpoint {
            wp.check("truncate_arguments", &self.stack, &self.frames);
        }
    }

    pub(crate) fn inline_return(&mut self, start_base: usize) -> bool {
        let return_value = self.stack.pop().unwrap();

        // truncate the stack to the base
        let base = self.frame().base;

        if let Some(ref wp) = self.watchpoint {
            if wp.position >= base && wp.position < self.stack.len() {
                eprintln!(
                    "[WATCH] stack[{}] will be TRUNCATED by Return (base={}, stack_len={}) | returning fn{} | return_value: {:?} ({} items) | frames: {}",
                    wp.position,
                    base,
                    self.stack.len(),
                    self.frame().function.get(),
                    Watchpoint::value_summary(&return_value),
                    match &return_value {
                        stack::Value::Absent => 0,
                        stack::Value::Sequence(s) => s.len(),
                    },
                    Watchpoint::frame_summary(&self.frames),
                );
            }
        }

        self.stack.truncate(base);

        // pop off the function id we just called
        // for the outer main function this is the context item
        if !self.stack.is_empty() {
            self.stack.pop();
        }

        // push back return value
        self.stack.push(return_value);

        if let Some(ref mut wp) = self.watchpoint {
            wp.check("inline_return", &self.stack, &self.frames);
        }

        // now pop off the frame
        self.frames.pop();

        // if the start base is the same as the base we just popped off,
        // we are done
        base == start_base
    }

    pub(crate) fn top(&self) -> error::Result<sequence::Sequence> {
        self.stack.last().unwrap().try_into()
    }

    pub fn stack(&self) -> &[stack::Value] {
        &self.stack
    }

    pub(crate) fn frames_debug(&self) -> &[Frame] {
        &self.frames
    }

    pub fn regex(&self, pattern: &str, flags: &str) -> error::Result<Rc<regexml::Regex>> {
        // TODO: would be nice if we could not do to_string here but use &str
        // but unfortunately otherwise lifetime issues bubble up all the way to
        // the library bindings if we do so
        let key = RegexKey {
            pattern: pattern.to_string(),
            flags: flags.to_string(),
        };
        let mut cache = self.regex_cache.borrow_mut();
        let entry = cache.entry(key);
        match entry {
            std::collections::hash_map::Entry::Occupied(entry) => Ok(entry.get().clone()),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let v = entry.insert(Rc::new(regexml::Regex::xpath(pattern, flags)?));
                Ok(v.clone())
            }
        }
    }

    pub fn xot(&self) -> &Xot {
        self.xot
    }

    pub fn xot_mut(&mut self) -> &mut Xot {
        self.xot
    }

    pub(crate) fn push_regex_groups(&mut self, groups: Vec<String>) {
        self.regex_groups.push(groups);
    }

    pub(crate) fn pop_regex_groups(&mut self) {
        self.regex_groups.pop();
    }

    pub(crate) fn regex_group(&self, n: usize) -> String {
        if let Some(groups) = self.regex_groups.last() {
            groups.get(n).cloned().unwrap_or_default()
        } else {
            String::new()
        }
    }

    pub(crate) fn push_current_group(
        &mut self,
        group: sequence::Sequence,
        key: Option<atomic::Atomic>,
    ) {
        self.current_group_stack.push(group);
        self.current_grouping_key_stack.push(key);
    }

    pub(crate) fn pop_current_group(&mut self) {
        self.current_group_stack.pop();
        self.current_grouping_key_stack.pop();
    }

    pub(crate) fn current_group(&self) -> sequence::Sequence {
        self.current_group_stack.last().cloned().unwrap_or_default()
    }

    pub(crate) fn current_grouping_key(&self) -> sequence::Sequence {
        if let Some(Some(key)) = self.current_grouping_key_stack.last() {
            sequence::Item::Atomic(key.clone()).into()
        } else {
            sequence::Sequence::default()
        }
    }
}
