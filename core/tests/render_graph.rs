//! Phase 4 render graph: node evaluation, DAG composition, custom nodes, error
//! handling, and — the migration-safety check — **parity with `compose_frame`**.

use ferrox_core::render::{
    ColorNode, CompositeNode, CpuBackend, CustomNode, MaskNode, NodeId, RenderGraph, ResizeNode, SolidNode, SourceNode,
};
use ferrox_core::{
    compose_frame, AscCdl, BlendMode, Clip, ClipSource, ColorGrade, Frame, Mask, PixelFormat, Project, Track, Transform,
};

fn solid_src(w: u32, h: u32, r: u8, g: u8, b: u8) -> ClipSource {
    ClipSource::Solid { width: w, height: h, r, g, b, a: 255 }
}

#[test]
fn single_node_source_evaluates() {
    let backend = CpuBackend;
    let mut g = RenderGraph::new();
    let s = g.add(SourceNode { source: solid_src(2, 2, 10, 20, 30) }, vec![]).unwrap();
    g.set_output(s);
    let out = g.evaluate(&backend).unwrap();
    assert_eq!((out.width, out.height), (2, 2));
    assert_eq!(&out.data[..4], &[10, 20, 30, 255]);
}

#[test]
fn chained_nodes_apply_in_order() {
    let backend = CpuBackend;
    let mut g = RenderGraph::new();
    let s = g.add(SourceNode { source: solid_src(1, 1, 64, 64, 64) }, vec![]).unwrap();
    // slope 2 doubles 64 → 128.
    let c = g.add(ColorNode { grade: ColorGrade::from_cdl(AscCdl { slope: [2.0, 2.0, 2.0], ..Default::default() }) }, vec![s]).unwrap();
    g.set_output(c);
    assert_eq!(g.evaluate(&backend).unwrap().data[0], 128);
}

#[test]
fn composite_node_blends_two_branches() {
    // Two source branches → composite blue over red.
    let backend = CpuBackend;
    let mut g = RenderGraph::new();
    let base = g.add(SolidNode { width: 1, height: 1, rgba: [255, 0, 0, 255] }, vec![]).unwrap();
    let top = g.add(SourceNode { source: solid_src(1, 1, 0, 0, 255) }, vec![]).unwrap();
    let comp = g.add(CompositeNode { x: 0, y: 0, opacity: 1.0, mode: BlendMode::Normal }, vec![base, top]).unwrap();
    g.set_output(comp);
    let out = g.evaluate(&backend).unwrap();
    assert_eq!(&out.data[..4], &[0, 0, 255, 255], "top fully covers base");
}

#[test]
fn custom_node_runs_arbitrary_logic() {
    let backend = CpuBackend;
    let mut g = RenderGraph::new();
    let s = g.add(SourceNode { source: solid_src(1, 1, 100, 100, 100) }, vec![]).unwrap();
    // A custom "invert" node.
    let inv = g
        .add(
            CustomNode::new(1, |inputs, _| {
                let mut f = inputs[0].clone();
                for px in f.data.chunks_exact_mut(4) {
                    px[0] = 255 - px[0];
                    px[1] = 255 - px[1];
                    px[2] = 255 - px[2];
                }
                Ok(f)
            }),
            vec![s],
        )
        .unwrap();
    g.set_output(inv);
    assert_eq!(&g.evaluate(&backend).unwrap().data[..3], &[155, 155, 155]);
}

#[test]
fn wrong_arity_is_rejected() {
    let mut g = RenderGraph::new();
    let s = g.add(SourceNode { source: solid_src(1, 1, 0, 0, 0) }, vec![]).unwrap();
    // ColorNode needs exactly 1 input.
    assert!(g.add(ColorNode { grade: ColorGrade::default() }, vec![s, s]).is_err());
    // Referencing a non-existent node id.
    assert!(g.add(ResizeNode { width: 1, height: 1 }, vec![NodeId(99)]).is_err());
}

#[test]
fn evaluate_without_output_errors() {
    let backend = CpuBackend;
    let mut g = RenderGraph::new();
    g.add(SourceNode { source: solid_src(1, 1, 0, 0, 0) }, vec![]).unwrap();
    assert!(g.evaluate(&backend).is_err());
}

#[test]
fn graph_reproduces_compose_frame_for_a_clip() {
    // The migration-safety proof: a graph built for one clip's pipeline
    // (source → color → resize → mask, composited over the background) matches
    // the linear `compose_frame` output pixel-for-pixel.
    let (w, h) = (8u32, 8u32);
    let src = solid_src(w, h, 200, 40, 40);
    let grade = ColorGrade::from_cdl(AscCdl { slope: [1.0, 1.5, 1.0], ..Default::default() });
    let mask = Mask::Rectangle { x: 0.25, y: 0.0, w: 0.5, h: 1.0, feather: 0.0, invert: false };

    // Reference: the linear compositor.
    let clip = Clip::new(src.clone(), 0.0, 1.0, Transform::default())
        .with_color(grade)
        .with_mask(mask.clone());
    let project = Project::new(w, h, 30.0).with_background(0, 0, 0).with_track(Track::new().with_clip(clip));
    let reference: Frame = compose_frame(&project, 0.0).unwrap();

    // Graph: solid bg ← composite ← (source → color → mask). No scale (scale 1).
    let backend = CpuBackend;
    let mut g = RenderGraph::new();
    let bg = g.add(SolidNode { width: w, height: h, rgba: [0, 0, 0, 255] }, vec![]).unwrap();
    let s = g.add(SourceNode { source: src }, vec![]).unwrap();
    let c = g.add(ColorNode { grade }, vec![s]).unwrap();
    let m = g.add(MaskNode { mask }, vec![c]).unwrap();
    let comp = g.add(CompositeNode { x: 0, y: 0, opacity: 1.0, mode: BlendMode::Normal }, vec![bg, m]).unwrap();
    g.set_output(comp);
    let graph_out = g.evaluate(&backend).unwrap();

    assert_eq!(graph_out.data, reference.data, "render graph matches compose_frame");
    assert_eq!(graph_out.format, PixelFormat::Rgba8);
}
