//! [`RenderGraph`] — a DAG of [`RenderNode`]s evaluated through a
//! [`RenderBackend`] to produce a frame. Replaces the linear compositor with
//! arbitrary node composition.
//!
//! **Acyclic by construction:** a node's inputs may only reference nodes added
//! before it, so cycles are impossible and evaluation needs no topological sort.
//! Evaluation is memoized and lazy — only nodes the output depends on run.

use crate::error::{Error, Result};
use crate::frame::Frame;

use super::backend::RenderBackend;
use super::nodes::RenderNode;

/// A handle to a node within a [`RenderGraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeId(pub usize);

struct Entry {
    node: Box<dyn RenderNode>,
    inputs: Vec<NodeId>,
}

/// A render graph: nodes wired into a DAG with one output.
#[derive(Default)]
pub struct RenderGraph {
    entries: Vec<Entry>,
    output: Option<NodeId>,
}

impl RenderGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node wired to `inputs`. Errors if the input count is wrong or an
    /// input references a node that hasn't been added yet (which also guarantees
    /// acyclicity).
    pub fn add<N: RenderNode + 'static>(&mut self, node: N, inputs: Vec<NodeId>) -> Result<NodeId> {
        if inputs.len() != node.input_count() {
            return Err(Error::Filter(format!("node '{}' takes {} inputs, got {}", node.name(), node.input_count(), inputs.len())));
        }
        for i in &inputs {
            if i.0 >= self.entries.len() {
                return Err(Error::Filter(format!("input node {} does not exist yet", i.0)));
            }
        }
        let id = NodeId(self.entries.len());
        self.entries.push(Entry { node: Box::new(node), inputs });
        Ok(id)
    }

    /// Designate the graph's output node.
    pub fn set_output(&mut self, id: NodeId) {
        self.output = Some(id);
    }

    /// Number of nodes.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Evaluate the graph to its output frame using `backend`.
    pub fn evaluate(&self, backend: &dyn RenderBackend) -> Result<Frame> {
        let out = self.output.ok_or_else(|| Error::Filter("render graph has no output".into()))?;
        let mut cache: Vec<Option<Frame>> = (0..self.entries.len()).map(|_| None).collect();
        self.eval_into(out, backend, &mut cache)?;
        cache[out.0].take().ok_or_else(|| Error::Filter("output not produced".into()))
    }

    /// Recursively evaluate `id` and its (already-earlier) dependencies, memoized.
    fn eval_into(&self, id: NodeId, backend: &dyn RenderBackend, cache: &mut [Option<Frame>]) -> Result<()> {
        if cache[id.0].is_some() {
            return Ok(());
        }
        let entry = &self.entries[id.0];
        for input in &entry.inputs {
            self.eval_into(*input, backend, cache)?;
        }
        let result = {
            let inputs: Vec<&Frame> = entry.inputs.iter().map(|i| cache[i.0].as_ref().expect("dependency evaluated")).collect();
            entry.node.eval(&inputs, backend)?
        };
        cache[id.0] = Some(result);
        Ok(())
    }
}
