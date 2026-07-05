//! WebAssembly bindings for the ferrox editing SDK.
//!
//! Exposes a single `Editor` JS class that mirrors [`ferrox_sdk::Editor`] with
//! identical semantics to the Kotlin/Swift bindings: **opaque handle**, edits
//! flow through the undo/redo command stack, complex objects cross the boundary
//! as **JSON strings**, and rendered frames come back as `Uint8Array` (RGBA).
//!
//! Build with `wasm-pack build --target bundler` (or `web`) to produce an npm
//! package with generated `.d.ts` types.

use wasm_bindgen::prelude::*;

use ferrox_sdk::commands::{
    AddKeyframeCommand, AnimField, MoveClipCommand, RemoveClipCommand, SetBlendModeCommand,
    SetColorGradeCommand, TrimClipCommand,
};
use ferrox_sdk::{BlendMode, ColorGrade, Easing, Editor as SdkEditor};

/// Install a panic hook that forwards Rust panics to the browser console.
#[wasm_bindgen(start)]
pub fn start() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// A handle to an editing session, usable from JavaScript/TypeScript.
#[wasm_bindgen]
pub struct Editor {
    inner: SdkEditor,
}

#[wasm_bindgen]
impl Editor {
    /// Create a new editor with an empty project of the given output spec.
    #[wasm_bindgen(constructor)]
    pub fn new(width: u32, height: u32, fps: f64) -> Editor {
        Editor { inner: SdkEditor::new(width, height, fps) }
    }

    /// Load a project from JSON, replacing the current one (clears history).
    #[wasm_bindgen(js_name = fromJson)]
    pub fn from_json(json: &str) -> Result<Editor, JsError> {
        let inner = SdkEditor::new(1, 1, 30.0);
        inner.load_json(json)?;
        Ok(Editor { inner })
    }

    /// Append an empty video track; returns its index.
    #[wasm_bindgen(js_name = addTrack)]
    pub fn add_track(&self) -> Result<u32, JsError> {
        Ok(self.inner.add_track()? as u32)
    }

    /// Append a clip (as JSON) to a track.
    #[wasm_bindgen(js_name = addClipJson)]
    pub fn add_clip_json(&self, track: u32, clip_json: &str) -> Result<(), JsError> {
        self.inner.add_clip_json(track as usize, clip_json)?;
        Ok(())
    }

    /// Remove a clip from a track.
    #[wasm_bindgen(js_name = removeClip)]
    pub fn remove_clip(&self, track: u32, index: u32) -> Result<(), JsError> {
        self.inner.execute(Box::new(RemoveClipCommand::new(track as usize, index as usize)))?;
        Ok(())
    }

    /// Move a clip's start time (seconds).
    #[wasm_bindgen(js_name = moveClip)]
    pub fn move_clip(&self, track: u32, index: u32, new_start: f64) -> Result<(), JsError> {
        self.inner.execute(Box::new(MoveClipCommand::new(track as usize, index as usize, new_start)))?;
        Ok(())
    }

    /// Trim a clip (start + duration, seconds).
    #[wasm_bindgen(js_name = trimClip)]
    pub fn trim_clip(&self, track: u32, index: u32, start: f64, duration: f64) -> Result<(), JsError> {
        self.inner.execute(Box::new(TrimClipCommand::new(track as usize, index as usize, start, duration)))?;
        Ok(())
    }

    /// Set a clip's blend mode by name (e.g. `"screen"`, `"multiply"`).
    #[wasm_bindgen(js_name = setBlendMode)]
    pub fn set_blend_mode(&self, track: u32, index: u32, mode: &str) -> Result<(), JsError> {
        let blend: BlendMode = serde_json::from_value(serde_json::Value::String(mode.to_string()))
            .map_err(|_| JsError::new(&format!("unknown blend mode '{mode}'")))?;
        self.inner.execute(Box::new(SetBlendModeCommand::new(track as usize, index as usize, blend)))?;
        Ok(())
    }

    /// Set a clip's color grade from JSON (an object like `{"cdl":{...}}`).
    #[wasm_bindgen(js_name = setColorGradeJson)]
    pub fn set_color_grade_json(&self, track: u32, index: u32, grade_json: &str) -> Result<(), JsError> {
        let grade: ColorGrade = serde_json::from_str(grade_json)
            .map_err(|e| JsError::new(&format!("invalid color grade: {e}")))?;
        self.inner.execute(Box::new(SetColorGradeCommand::new(track as usize, index as usize, grade)))?;
        Ok(())
    }

    /// Add a keyframe on a transform field (`"x"`, `"y"`, `"scale"`, `"opacity"`).
    #[wasm_bindgen(js_name = addKeyframe)]
    pub fn add_keyframe(&self, track: u32, index: u32, field: &str, t: f64, value: f32) -> Result<(), JsError> {
        let field = match field {
            "x" => AnimField::X,
            "y" => AnimField::Y,
            "scale" => AnimField::Scale,
            "opacity" => AnimField::Opacity,
            other => return Err(JsError::new(&format!("unknown field '{other}'"))),
        };
        self.inner
            .execute(Box::new(AddKeyframeCommand::new(track as usize, index as usize, field, t, value, Easing::Linear)))?;
        Ok(())
    }

    /// Render the composed frame at time `t` (seconds) as RGBA bytes. Pass
    /// `width`/`height` = 0 to use the project's own size.
    #[wasm_bindgen(js_name = renderFrame)]
    pub fn render_frame(&self, t: f64, width: u32, height: u32) -> Result<Vec<u8>, JsError> {
        Ok(self.inner.render_frame(t, width, height)?)
    }

    /// Undo the last command; returns `true` if something was undone.
    pub fn undo(&self) -> Result<bool, JsError> {
        Ok(self.inner.undo()?)
    }

    /// Redo the last undone command; returns `true` if something was redone.
    pub fn redo(&self) -> Result<bool, JsError> {
        Ok(self.inner.redo()?)
    }

    /// Depth of the undo stack.
    #[wasm_bindgen(js_name = undoDepth)]
    pub fn undo_depth(&self) -> u32 {
        self.inner.undo_depth() as u32
    }

    /// Depth of the redo stack.
    #[wasm_bindgen(js_name = redoDepth)]
    pub fn redo_depth(&self) -> u32 {
        self.inner.redo_depth() as u32
    }

    /// Serialize the current project to JSON.
    #[wasm_bindgen(js_name = saveJson)]
    pub fn save_json(&self) -> Result<String, JsError> {
        Ok(self.inner.save_json()?)
    }

    /// Replace the project from JSON (clears undo/redo history).
    #[wasm_bindgen(js_name = loadJson)]
    pub fn load_json(&self, json: &str) -> Result<(), JsError> {
        self.inner.load_json(json)?;
        Ok(())
    }
}
